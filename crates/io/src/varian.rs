//! Varian and Agilent VNMR/VnmrJ raw acquisition reader.

mod fid;
mod procpar;

use crate::{
    Acquisition, DataFormat, Dim, Domain, IoError, LoadResult, NmrData, NmrData2D, Provenance,
    QuadMode,
};
use procpar::Procpar;
use std::path::{Path, PathBuf};

pub fn is_varian(path: &Path) -> bool {
    resolve(path).is_some_and(|(_, fid, procpar)| fid.is_file() && procpar.is_file())
}

fn resolve(path: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else if path.file_name()?.to_str()? == "fid" {
        path.parent()?.to_path_buf()
    } else {
        return None;
    };
    Some((dir.clone(), dir.join("fid"), dir.join("procpar")))
}

pub fn load_raw(path: &Path) -> Result<LoadResult, IoError> {
    let (dir, data_path, procpar_path) = resolve(path)
        .ok_or_else(|| IoError::InvalidVarian("select a .fid directory or its fid file".into()))?;
    if !data_path.is_file() || !procpar_path.is_file() {
        return Err(IoError::InvalidVarian(
            "a VnmrJ dataset requires sibling fid and procpar files".into(),
        ));
    }
    let params = Procpar::parse(&std::fs::read_to_string(&procpar_path)?)?;
    let raw = fid::parse(&std::fs::read(&data_path)?)?;
    reject_unsupported(&params)?;
    let acquisition = assemble(&dir, &params, raw)?;
    Ok(LoadResult {
        scientific_identity: crate::ImportedScientificIdentity {
            subject: sample_name(&dir, &params),
            acquisition: experiment_name(&params),
            source_label: dir
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled NMR")
                .to_owned(),
        },
        acquisition,
        format: DataFormat::VarianAgilentRaw,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path,
            parameter_paths: vec![procpar_path],
            companion_paths: Vec::new(),
        },
        warnings: Vec::new(),
    })
}

fn reject_unsupported(p: &Procpar) -> Result<(), IoError> {
    if p.number("ni2").unwrap_or(0.0) > 1.0 || p.number("ni3").unwrap_or(0.0) > 1.0 {
        return Err(IoError::UnsupportedVarian(
            "3D and 4D acquisitions are not supported".into(),
        ));
    }
    if p.string("apptype")
        .is_some_and(|s| s.to_ascii_lowercase().contains("imaging"))
    {
        return Err(IoError::UnsupportedVarian(
            "MRI and imaging acquisitions are not supported".into(),
        ));
    }
    if ["sampling", "nus", "nuslist"].iter().any(|name| {
        p.string(name)
            .is_some_and(|s| !s.is_empty() && !s.eq_ignore_ascii_case("n"))
    }) {
        return Err(IoError::UnsupportedVarian(
            "non-uniform sampling is not supported".into(),
        ));
    }
    Ok(())
}

fn assemble(dir: &Path, p: &Procpar, raw: fid::FidData) -> Result<Acquisition, IoError> {
    let procpar_np = exact_positive_usize(p.number("np"))
        .ok_or_else(|| IoError::InvalidVarian("procpar is missing positive integer np".into()))?;
    if procpar_np != raw.np {
        return Err(IoError::InvalidVarian(format!(
            "dimension mismatch: procpar np is {procpar_np}, but the fid header declares {}",
            raw.np
        )));
    }
    let direct = direct_dim(p)?;
    let total = raw.traces.len();
    let ni = exact_positive_usize(p.number("ni"));
    let (phase_count, quad) = phase_layout(p)?;
    let array = p.string("array").unwrap_or("").trim();
    if ni.unwrap_or(1) == 1 && total == 1 && array.is_empty() {
        let source = description(dir, p, &direct, None);
        return Ok(Acquisition::D1(NmrData {
            points: raw.traces.into_iter().next().unwrap(),
            domain: Domain::Time,
            spectral_width_hz: direct.spectral_width_hz,
            observe_freq_mhz: direct.observe_freq_mhz,
            carrier_ppm: direct.carrier_ppm,
            nucleus: direct.nucleus,
            source,
            group_delay: 0.0,
        }));
    }
    let ni = ni.ok_or_else(|| {
        IoError::UnsupportedVarian("multiple traces require a positive integer ni".into())
    })?;
    if !array.is_empty() && array != "phase" {
        return Err(IoError::UnsupportedVarian(format!(
            "parameter arrays other than phase are not supported (array={array})"
        )));
    }
    let expected = ni
        .checked_mul(phase_count)
        .ok_or_else(|| IoError::InvalidVarian("2D trace count overflow".into()))?;
    if total != expected {
        return Err(IoError::UnsupportedVarian(format!(
            "trace layout mismatch: fid contains {total} traces, but ni × phase_count is {expected}"
        )));
    }
    let seq = p
        .string("seqfil")
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let indirect = indirect_dim(p, seq.as_deref(), &direct)?;
    let cols = raw.np / 2;
    let source = description(dir, p, &direct, Some(&indirect));
    Ok(Acquisition::D2(Box::new(NmrData2D {
        data: raw.traces.into_iter().flatten().collect(),
        rows: total,
        cols,
        domain: Domain::Time,
        direct,
        indirect,
        quad,
        indirect_conjugate: false,
        experiment: seq,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source,
    })))
}

fn phase_layout(p: &Procpar) -> Result<(usize, QuadMode), IoError> {
    match p.numbers("phase").as_deref() {
        None | Some([1.0]) => Ok((1, QuadMode::Complex)),
        Some([1.0, 2.0]) => Ok((2, QuadMode::States)),
        Some(values) => Err(IoError::UnsupportedVarian(format!(
            "unsupported phase table {values:?}; only phase=1 and States phase=1,2 are supported"
        ))),
    }
}

fn direct_dim(p: &Procpar) -> Result<Dim, IoError> {
    dim(p, "sw", "sfrq", "tof", "tn")
}
fn indirect_dim(p: &Procpar, seq: Option<&str>, direct: &Dim) -> Result<Dim, IoError> {
    let homo = seq.is_some_and(|s| {
        ["cosy", "tocsy", "noesy", "roesy"]
            .iter()
            .any(|name| s.contains(name))
    });
    let hetero = seq.is_some_and(|s| ["hsqc", "hmqc", "hmbc"].iter().any(|name| s.contains(name)));
    if homo {
        return dim_with_sw1(p, direct);
    }
    if hetero {
        return dim(p, "sw1", "dfrq", "dof", "dn");
    }
    let tn = normalize_nucleus(p.string("tn").unwrap_or("X"));
    let dn = normalize_nucleus(p.string("dn").unwrap_or("X"));
    if dn != "X" && dn != tn {
        dim(p, "sw1", "dfrq", "dof", "dn")
    } else if dn == "X" || dn == tn {
        dim_with_sw1(p, direct)
    } else {
        Err(IoError::UnsupportedVarian(
            "unknown sequence has ambiguous indirect channel".into(),
        ))
    }
}
fn dim_with_sw1(p: &Procpar, direct: &Dim) -> Result<Dim, IoError> {
    Ok(Dim {
        spectral_width_hz: required_positive(p, "sw1")?,
        observe_freq_mhz: direct.observe_freq_mhz,
        carrier_ppm: direct.carrier_ppm,
        nucleus: direct.nucleus.clone(),
        group_delay: 0.0,
    })
}
fn dim(p: &Procpar, sw: &str, freq: &str, offset: &str, nucleus: &str) -> Result<Dim, IoError> {
    let spectral_width_hz = required_positive(p, sw)?;
    let observe_freq_mhz = required_positive(p, freq)?;
    let carrier_ppm = p
        .number(offset)
        .filter(|v| v.is_finite())
        .ok_or_else(|| IoError::InvalidVarian(format!("procpar is missing finite {offset}")))?
        / observe_freq_mhz;
    Ok(Dim {
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm,
        nucleus: normalize_nucleus(p.string(nucleus).unwrap_or("X")),
        group_delay: 0.0,
    })
}
fn required_positive(p: &Procpar, name: &str) -> Result<f64, IoError> {
    p.number(name)
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| IoError::InvalidVarian(format!("procpar is missing positive finite {name}")))
}
fn exact_positive_usize(v: Option<f64>) -> Option<usize> {
    let v = v?;
    if v.is_finite() && v > 0.0 && v.fract() == 0.0 && v <= usize::MAX as f64 {
        Some(v as usize)
    } else {
        None
    }
}
fn normalize_nucleus(value: &str) -> String {
    let s = value.trim().trim_matches('"').replace(' ', "");
    let upper = s.to_ascii_uppercase();
    match upper.as_str() {
        "H1" | "1H" | "PROTON" => "1H".into(),
        "C13" | "13C" => "13C".into(),
        "N15" | "15N" => "15N".into(),
        "F19" | "19F" => "19F".into(),
        "P31" | "31P" => "31P".into(),
        "" | "OFF" | "NONE" => "X".into(),
        _ => s,
    }
}
fn description(dir: &Path, p: &Procpar, direct: &Dim, indirect: Option<&Dim>) -> String {
    let nuclei = match indirect {
        Some(indirect) => format!("{}/{}", direct.nucleus, indirect.nucleus),
        None => direct.nucleus.clone(),
    };
    sample_name(dir, p)
        .into_iter()
        .chain(std::iter::once(nuclei))
        .chain(experiment_name(p))
        .collect::<Vec<_>>()
        .join(" — ")
}

fn sample_name(dir: &Path, p: &Procpar) -> Option<String> {
    ["samplename", "sample", "name", "filename"]
        .into_iter()
        .find_map(|name| p.string(name).and_then(nonempty))
        .map(str::to_owned)
        .or_else(|| {
            dir.file_stem()
                .and_then(|name| name.to_str())
                .and_then(nonempty)
                .map(str::to_owned)
        })
}

fn experiment_name(p: &Procpar) -> Option<String> {
    ["pslabel", "seqfil"]
        .into_iter()
        .find_map(|name| p.string(name).and_then(nonempty))
        .map(str::to_owned)
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
#[path = "varian/tests.rs"]
mod tests;
