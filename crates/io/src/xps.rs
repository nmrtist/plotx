use crate::{
    Acquisition, DataFormat, IoError, LoadResult, LoadWarning, LoadWarningCode, Provenance,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

mod vamas;

const VAMAS_MAGIC: &str = "VAMAS Surface Chemical Analysis Standard Data Transfer Format";
const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct XpsMeasurementId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct XpsRegionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XpsEnergyKind {
    Binding,
    Kinetic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedXpsPeak {
    pub label: String,
    pub position_ev: f64,
    pub fwhm_ev: f64,
    pub area: f64,
    pub lineshape: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedXpsFit {
    pub background_cps: Vec<f64>,
    pub envelope_cps: Vec<f64>,
    pub components_cps: Vec<Vec<f64>>,
    pub peaks: Vec<ImportedXpsPeak>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpsRegion {
    pub id: XpsRegionId,
    pub measurement: XpsMeasurementId,
    pub name: String,
    pub native_energy_kind: XpsEnergyKind,
    pub native_energy_ev: Vec<f64>,
    pub binding_energy_ev: Option<Vec<f64>>,
    pub intensity_cps: Vec<f64>,
    pub counts: Option<Vec<f64>>,
    pub photon_energy_ev: Option<f64>,
    pub dwell_time_s: Option<f64>,
    pub sweeps: Option<u32>,
    pub imported_fit: Option<ImportedXpsFit>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpsMeasurement {
    pub id: XpsMeasurementId,
    pub label: String,
    pub position_mm: Option<[f64; 3]>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpsExperiment {
    pub source: String,
    pub measurements: Vec<XpsMeasurement>,
    pub regions: Vec<XpsRegion>,
    pub metadata: BTreeMap<String, String>,
    pub import_warnings: Vec<String>,
}

impl XpsExperiment {
    pub fn validate(&self) -> Result<(), String> {
        if self.regions.is_empty() {
            return Err("experiment has no readable XPS regions".into());
        }
        let mut measurement_ids = std::collections::BTreeSet::new();
        for measurement in &self.measurements {
            if measurement.id.0 == 0 || !measurement_ids.insert(measurement.id) {
                return Err("experiment has invalid or duplicate measurement IDs".into());
            }
            if measurement
                .position_mm
                .is_some_and(|position| position.iter().any(|value| !value.is_finite()))
            {
                return Err(format!(
                    "measurement {} contains an invalid position",
                    measurement.label
                ));
            }
        }
        let mut region_ids = std::collections::BTreeSet::new();
        for region in &self.regions {
            if region.id.0 == 0 || !region_ids.insert(region.id) {
                return Err("experiment has invalid or duplicate region IDs".into());
            }
            if !measurement_ids.contains(&region.measurement) {
                return Err(format!(
                    "region {} references a missing measurement",
                    region.name
                ));
            }
            let n = region.native_energy_ev.len();
            if n < 2 || region.intensity_cps.len() != n {
                return Err(format!("region {} has inconsistent arrays", region.name));
            }
            if region
                .binding_energy_ev
                .as_ref()
                .is_some_and(|values| values.len() != n)
                || region
                    .counts
                    .as_ref()
                    .is_some_and(|values| values.len() != n)
                || region.native_energy_ev.iter().any(|v| !v.is_finite())
                || region.intensity_cps.iter().any(|v| !v.is_finite())
                || region
                    .binding_energy_ev
                    .as_ref()
                    .is_some_and(|values| values.iter().any(|v| !v.is_finite()))
                || region
                    .counts
                    .as_ref()
                    .is_some_and(|values| values.iter().any(|v| !v.is_finite()))
                || region
                    .photon_energy_ev
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || region
                    .dwell_time_s
                    .is_some_and(|value| !value.is_finite() || value <= 0.0)
                || region.sweeps == Some(0)
            {
                return Err(format!("region {} contains invalid values", region.name));
            }
            if let Some(fit) = &region.imported_fit {
                let arrays_valid = fit.background_cps.len() == n
                    && fit.envelope_cps.len() == n
                    && fit.components_cps.len() == fit.peaks.len()
                    && fit.components_cps.iter().all(|values| values.len() == n)
                    && fit
                        .background_cps
                        .iter()
                        .chain(&fit.envelope_cps)
                        .chain(fit.components_cps.iter().flatten())
                        .all(|value| value.is_finite());
                let peaks_valid = fit.peaks.iter().all(|peak| {
                    peak.position_ev.is_finite()
                        && peak.fwhm_ev.is_finite()
                        && peak.fwhm_ev > 0.0
                        && peak.area.is_finite()
                        && peak.area >= 0.0
                });
                if !arrays_valid || !peaks_valid {
                    return Err(format!(
                        "region {} has an invalid imported fit",
                        region.name
                    ));
                }
            }
        }
        Ok(())
    }
}

fn read_text(path: &Path) -> Result<String, IoError> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_TEXT_BYTES {
        return Err(IoError::InvalidXps(format!(
            "{} exceeds the 128 MiB input limit",
            path.display()
        )));
    }
    String::from_utf8(std::fs::read(path)?)
        .map_err(|_| IoError::InvalidXps("XPS text is not valid UTF-8".into()))
}

fn prefix(path: &Path, max: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = vec![0; max];
    let read = file.read(&mut bytes).ok()?;
    bytes.truncate(read);
    String::from_utf8(bytes).ok()
}

pub fn is_vamas_xps(path: &Path) -> bool {
    path.is_file() && prefix(path, 256).is_some_and(|text| is_vamas_content(&text))
}

pub fn is_casaxps_text(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(text) = prefix(path, 16 * 1024) else {
        return false;
    };
    is_casaxps_content(&text)
}

pub fn is_vamas_content(text: &str) -> bool {
    text.starts_with(VAMAS_MAGIC)
}

pub fn is_casaxps_content(text: &str) -> bool {
    let lines = text.lines().take(8).collect::<Vec<_>>();
    lines.len() == 8
        && lines[0].starts_with("Cycle ")
        && lines[2].starts_with("Name\t")
        && lines[3].starts_with("Position\t")
        && lines[4].starts_with("FWHM\t")
        && lines[5].starts_with("Area\t")
        && lines[6].starts_with("Lineshape\t")
        && lines[7].contains("\tB.E.\tCPS\t")
}

pub fn load_vamas(path: &Path) -> Result<LoadResult, IoError> {
    let text = read_text(path)?;
    let experiment = parse_vamas(&text, path.display().to_string())?;
    load_result(path, DataFormat::VamasXps, experiment)
}

pub fn load_casaxps(path: &Path) -> Result<LoadResult, IoError> {
    let text = read_text(path)?;
    let experiment = parse_casaxps(&text, path.display().to_string())?;
    load_result(path, DataFormat::CasaXpsText, experiment)
}

fn load_result(
    path: &Path,
    format: DataFormat,
    experiment: XpsExperiment,
) -> Result<LoadResult, IoError> {
    experiment.validate().map_err(IoError::InvalidXps)?;
    let warnings = experiment
        .import_warnings
        .iter()
        .map(|message| LoadWarning {
            code: LoadWarningCode::UnsupportedFunction,
            message: message.clone(),
            path: Some(path.to_owned()),
        })
        .collect();
    Ok(LoadResult {
        scientific_identity: crate::ImportedScientificIdentity {
            subject: experiment
                .measurements
                .first()
                .map(|item| item.label.clone()),
            acquisition: None,
            source_label: crate::ImportedScientificIdentity::from_path(path).source_label,
        },
        acquisition: Acquisition::Xps(Box::new(experiment)),
        format,
        provenance: Provenance {
            selected_path: path.into(),
            data_path: path.into(),
            parameter_paths: Vec::new(),
            companion_paths: Vec::new(),
        },
        warnings,
    })
}

fn parse_numbers(line: &str, label: &str) -> Result<Vec<f64>, IoError> {
    line.split('\t')
        .skip(1)
        .filter(|v| !v.trim().is_empty())
        .map(|value| {
            value
                .trim()
                .parse::<f64>()
                .map_err(|_| IoError::InvalidXps(format!("invalid {label} value {value:?}")))
        })
        .collect()
}

pub fn parse_casaxps(text: &str, source: String) -> Result<XpsExperiment, IoError> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() < 9 || !lines[0].starts_with("Cycle ") || !lines[7].contains("\tB.E.\tCPS\t") {
        return Err(IoError::InvalidXps(
            "not a structured CasaXPS text export".into(),
        ));
    }
    let labels = lines[2]
        .split('\t')
        .skip(2)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let positions = parse_numbers(lines[3], "peak position")?;
    let fwhms = parse_numbers(lines[4], "FWHM")?;
    let areas = parse_numbers(lines[5], "area")?;
    let shapes = lines[6]
        .split('\t')
        .skip(2)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let n_peaks = positions.len();
    if fwhms.len() != n_peaks
        || areas.len() != n_peaks
        || labels.len() != n_peaks
        || shapes.len() != n_peaks
    {
        return Err(IoError::InvalidXps(
            "CasaXPS peak header lengths disagree".into(),
        ));
    }
    let header = lines[7].split('\t').collect::<Vec<_>>();
    let be_col = header
        .iter()
        .position(|v| *v == "B.E.")
        .ok_or_else(|| IoError::InvalidXps("CasaXPS B.E. column is missing".into()))?;
    let cps_col = be_col + 1;
    let comp_start = cps_col + 1;
    let bg_col = comp_start + n_peaks;
    let env_col = bg_col + 1;
    let mut be = Vec::new();
    let mut cps = Vec::new();
    let mut bg = Vec::new();
    let mut envelope = Vec::new();
    let mut components = vec![Vec::new(); n_peaks];
    for (row, line) in lines[8..].iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() <= env_col {
            return Err(IoError::InvalidXps(format!(
                "CasaXPS data row {} is truncated",
                row + 1
            )));
        }
        let value = |col: usize| {
            fields[col].trim().parse::<f64>().map_err(|_| {
                IoError::InvalidXps(format!(
                    "invalid CasaXPS numeric value on data row {}",
                    row + 1
                ))
            })
        };
        be.push(value(be_col)?);
        cps.push(value(cps_col)?);
        bg.push(value(bg_col)?);
        envelope.push(value(env_col)?);
        for (index, component) in components.iter_mut().enumerate() {
            component.push(value(comp_start + index)?);
        }
    }
    if be.len() < 2 {
        return Err(IoError::InvalidXps(
            "CasaXPS export contains fewer than two data rows".into(),
        ));
    }
    let region_name = labels.first().cloned().unwrap_or_else(|| {
        lines[0]
            .rsplit(':')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("XPS")
            .to_owned()
    });
    let peaks = positions
        .into_iter()
        .enumerate()
        .map(|(i, position_ev)| ImportedXpsPeak {
            label: labels
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("Peak {}", i + 1)),
            position_ev,
            fwhm_ev: fwhms[i],
            area: areas[i],
            lineshape: shapes.get(i).cloned(),
        })
        .collect();
    Ok(XpsExperiment {
        source,
        measurements: vec![XpsMeasurement {
            id: XpsMeasurementId(1),
            label: "CasaXPS export".into(),
            position_mm: None,
            metadata: BTreeMap::new(),
        }],
        regions: vec![XpsRegion {
            id: XpsRegionId(1),
            measurement: XpsMeasurementId(1),
            name: region_name,
            native_energy_kind: XpsEnergyKind::Binding,
            native_energy_ev: be.clone(),
            binding_energy_ev: Some(be),
            intensity_cps: cps,
            counts: None,
            photon_energy_ev: None,
            dwell_time_s: None,
            sweeps: None,
            imported_fit: Some(ImportedXpsFit {
                background_cps: bg,
                envelope_cps: envelope,
                components_cps: components,
                peaks,
            }),
            metadata: BTreeMap::new(),
        }],
        metadata: BTreeMap::new(),
        import_warnings: Vec::new(),
    })
}

pub use vamas::parse_vamas;

#[cfg(test)]
mod tests {
    use super::*;

    fn vamas_block(technique: &str, photon: &str, payload: &[&str]) -> String {
        format!(
            "C 1s\n1\n2025\n1\n1\n0\n0\n0\n0\n0\n{technique}\nAl\n{photon}\n75\n1e+037\n1e+037\n1e+037\n1e+037\nFAT\n20\n1e+037\n-4.5\n1e+037\n1e+037\n1e+037\n1e+037\n1e+037\nC\n1s\n-1\nKinetic energy\neV\n1200\n1\n2\nIntensity\nd\nTransmission\nd\npulse counting\n1\n1\n0\n1e+037\n1e+037\n1e+037\n0\n4\n0\n20\n1\n1\n{}\n",
            payload.join("\n")
        )
    }

    fn vamas(blocks: &str) -> String {
        let block_count = blocks.matches("\n1\n2025\n").count();
        format!(
            "{VAMAS_MAGIC} 1988 May 4\nsynthetic\ninstrument\noperator\nexperiment\n0\nNORM\nREGULAR\n1\n0\n0\n0\n0\n0\n{block_count}\n{blocks}"
        )
    }

    #[test]
    fn rejects_generic_delimited_text_as_casaxps() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("plotx-xps-generic-{}.txt", std::process::id()));
        std::fs::write(&path, "energy,intensity\n1,2\n").unwrap();
        assert!(!is_casaxps_text(&path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn casaxps_rejects_truncated_data_rows() {
        let text = "Cycle 1:1:C 1s\nmetadata\nName\t\tC 1s\nPosition\t\t284.8\nFWHM\t\t1.2\nArea\t\t10\nLineshape\t\tGL(30)\nK.E.\tCounts\tC 1s\tBackground\tEnvelope\t\tB.E.\tCPS\tC 1s\tBackground CPS\tEnvelope CPS\n1200\t10\n";
        assert!(parse_casaxps(text, "truncated.txt".into()).is_err());
    }

    #[test]
    fn vamas_ke_without_photon_energy_remains_kinetic_only() {
        let experiment = parse_vamas(
            &vamas(&vamas_block("XPS", "1e+037", &["10", "1", "20", "1"])),
            "memory.vms".into(),
        )
        .unwrap();
        assert_eq!(
            experiment.regions[0].native_energy_kind,
            XpsEnergyKind::Kinetic
        );
        assert!(experiment.regions[0].binding_energy_ev.is_none());
    }

    #[test]
    fn vamas_rejects_truncation_and_non_finite_ordinates() {
        let truncated = vamas(&vamas_block("XPS", "1486.69", &["10", "1"]));
        assert!(parse_vamas(&truncated, "truncated.vms".into()).is_err());
        let non_finite = vamas(&vamas_block("XPS", "1486.69", &["NaN", "1", "20", "1"]));
        assert!(parse_vamas(&non_finite, "nan.vms".into()).is_err());
    }

    #[test]
    fn vamas_skips_unknown_technique_but_requires_one_readable_xps_block() {
        let unknown = vamas_block("AES", "1486.69", &["10", "1", "20", "1"]);
        assert!(parse_vamas(&vamas(&unknown), "aes.vms".into()).is_err());
        let mixed = format!(
            "{unknown}{}",
            vamas_block("XPS", "1486.69", &["10", "1", "20", "1"])
        );
        let experiment = parse_vamas(&vamas(&mixed), "mixed.vms".into()).unwrap();
        assert_eq!(experiment.regions.len(), 1);
        assert_eq!(experiment.import_warnings.len(), 1);
    }

    #[test]
    #[ignore = "requires PLOTX_XPS_REFERENCE_DIR"]
    fn reads_external_reference_files() {
        let root = std::env::var_os("PLOTX_XPS_REFERENCE_DIR").expect("reference directory");
        let root = Path::new(&root);
        let vamas = load_vamas(&root.join("WBG250331.vms")).unwrap();
        let Acquisition::Xps(experiment) = vamas.acquisition else {
            panic!("expected XPS");
        };
        eprintln!(
            "measurements={} regions={} points={:?}",
            experiment.measurements.len(),
            experiment.regions.len(),
            experiment
                .regions
                .iter()
                .map(|r| (&r.name, r.intensity_cps.len()))
                .collect::<Vec<_>>()
        );
        assert_eq!(experiment.measurements.len(), 5);
        assert!(
            experiment
                .regions
                .iter()
                .any(|region| region.name == "Survey")
        );
        assert!(
            experiment
                .regions
                .iter()
                .all(|region| region.photon_energy_ev == Some(1486.69))
        );
        assert_eq!(
            experiment.regions[0]
                .metadata
                .get("anode")
                .map(String::as_str),
            Some("Al")
        );
        assert_eq!(experiment.regions[0].counts.as_ref().unwrap()[0], 2078.0);
        assert!((experiment.regions[0].intensity_cps[0] - 2078.0 / 0.088_495).abs() < 1e-9);
        assert_eq!(
            experiment.measurements[0]
                .metadata
                .get("sample")
                .map(String::as_str),
            Some("WBG")
        );

        let casa = load_casaxps(&root.join("cof_1_0001.txt")).unwrap();
        let Acquisition::Xps(experiment) = casa.acquisition else {
            panic!("expected XPS");
        };
        let region = &experiment.regions[0];
        assert_eq!(
            region.intensity_cps.len(),
            region.binding_energy_ev.as_ref().unwrap().len()
        );
        assert!(
            region
                .imported_fit
                .as_ref()
                .is_some_and(|fit| !fit.peaks.is_empty())
        );
        let raw_casa = load_casaxps(&root.join("cof_2_0002.txt")).unwrap();
        let Acquisition::Xps(raw_experiment) = raw_casa.acquisition else {
            panic!("expected XPS");
        };
        assert_eq!(raw_experiment.regions[0].name, "N 1s");
        assert!(
            raw_experiment.regions[0]
                .imported_fit
                .as_ref()
                .is_some_and(|fit| fit.peaks.is_empty())
        );
    }
}
