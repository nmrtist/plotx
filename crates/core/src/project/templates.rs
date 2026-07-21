use super::*;

pub const TEMPLATE_EXTENSION: &str = "plotxproc";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInfo {
    pub name: String,
    pub path: PathBuf,
}

pub fn templates_dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|dir| dir.join("templates"))
}

pub fn validate_template_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProjectError::Invalid("template name is empty".to_owned()));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err(ProjectError::Invalid(
            "template name cannot start or end with a dot".to_owned(),
        ));
    }
    if name.chars().any(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return Err(ProjectError::Invalid(
            "template name contains characters not allowed in file names".to_owned(),
        ));
    }
    // Windows device names (CON, COM1, …) are reserved even with an extension.
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if device {
        return Err(ProjectError::Invalid(
            "template name is a reserved device name".to_owned(),
        ));
    }
    Ok(name)
}

pub fn template_path(dir: &Path, name: &str) -> Result<PathBuf> {
    let name = validate_template_name(name)?;
    Ok(dir.join(format!("{name}.{TEMPLATE_EXTENSION}")))
}

pub fn template_exists(dir: &Path, name: &str) -> bool {
    template_path(dir, name)
        .map(|p| p.exists())
        .unwrap_or(false)
}

pub fn list_templates(dir: &Path) -> Vec<TemplateInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut templates: Vec<TemplateInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_template = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(TEMPLATE_EXTENSION))
                .unwrap_or(false);
            if !is_template || !path.is_file() {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_owned();
            Some(TemplateInfo { name, path })
        })
        .collect();
    templates.sort_by_key(|template| template.name.to_lowercase());
    templates
}

pub fn save_template(dir: &Path, name: &str, dataset: &Dataset) -> Result<PathBuf> {
    let path = template_path(dir, name)?;
    std::fs::create_dir_all(dir)?;
    save_scheme(&path, dataset)?;
    Ok(path)
}

pub fn load_template(dir: &Path, name: &str) -> Result<ProcessingScheme> {
    load_scheme(&template_path(dir, name)?)
}

pub fn delete_template(dir: &Path, name: &str) -> Result<()> {
    std::fs::remove_file(template_path(dir, name)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::NmrDataset;
    use num_complex::Complex64;
    use plotx_io::{Domain, NmrData};

    fn temp_templates_dir(name: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("plotx-templates-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn dataset_1d() -> Dataset {
        let points = (0..64)
            .map(|k| Complex64::from_polar((-(k as f64) / 16.0).exp(), 0.4 * k as f64))
            .collect();
        Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points,
            domain: Domain::Time,
            spectral_width_hz: 4000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 5.0,
            nucleus: "1H".to_owned(),
            source: "synthetic".to_owned(),
            group_delay: 0.0,
        })))
    }

    #[test]
    fn save_list_load_delete_roundtrip() {
        let dir = temp_templates_dir("roundtrip");
        let dataset = dataset_1d();

        assert!(list_templates(&dir).is_empty());
        let path = save_template(&dir, "Standard 1H", &dataset).unwrap();
        assert_eq!(path.extension().unwrap(), TEMPLATE_EXTENSION);
        assert!(template_exists(&dir, "Standard 1H"));

        let listed = list_templates(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Standard 1H");
        assert_eq!(listed[0].path, path);

        let scheme = load_template(&dir, "Standard 1H").unwrap();
        assert_eq!(scheme.dimension_count, 1);
        assert!(apply_scheme(&scheme, &dataset).is_ok());

        delete_template(&dir, "Standard 1H").unwrap();
        assert!(!template_exists(&dir, "Standard 1H"));
        assert!(list_templates(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_sorts_case_insensitively_and_ignores_foreign_files() {
        let dir = temp_templates_dir("listing");
        let dataset = dataset_1d();
        save_template(&dir, "beta", &dataset).unwrap();
        save_template(&dir, "Alpha", &dataset).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let names: Vec<String> = list_templates(&dir).into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["Alpha".to_owned(), "beta".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_names_are_rejected_before_touching_the_filesystem() {
        let dir = temp_templates_dir("names");
        for bad in [
            "", "   ", "a/b", "a\\b", "a:b", "up?", ".hidden", "dot.", "con", "PRN", "Nul", "com1",
            "LPT9", "aux.old",
        ] {
            assert!(template_path(&dir, bad).is_err(), "accepted {bad:?}");
        }
        assert!(!dir.exists());
        assert_eq!(
            validate_template_name("  Standard 1H  ").unwrap(),
            "Standard 1H"
        );
    }

    #[test]
    fn saving_an_existing_name_overwrites_in_place() {
        let dir = temp_templates_dir("overwrite");
        let dataset = dataset_1d();
        save_template(&dir, "one", &dataset).unwrap();
        let first = load_template(&dir, "one").unwrap();
        let mut modified = dataset.clone();
        if let Dataset::Nmr(n) = &mut modified {
            n.group_delay_correct = false;
        }
        save_template(&dir, "one", &modified).unwrap();
        let second = load_template(&dir, "one").unwrap();
        assert!(first.group_delay_correct);
        assert!(!second.group_delay_correct);
        assert_eq!(list_templates(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
