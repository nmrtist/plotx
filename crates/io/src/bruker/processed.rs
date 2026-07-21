//! Bruker processed-data reader (`1r`/`1i`/`2rr`) built from the `procs`
//! parameter files under a `pdata` directory.

use super::*;

/// Identify a processed payload from a proc directory, a `1r`/`1i`/`2rr`
/// file, a `pdata` directory, or an experiment directory containing `pdata`.
pub fn detect_processed(path: &Path) -> Option<DataFormat> {
    let resolved = resolve_processed(path)?;
    Some(if resolved.two_d {
        DataFormat::BrukerProcessed2D
    } else {
        DataFormat::BrukerProcessed1D
    })
}

struct ProcessedPaths {
    proc_dir: PathBuf,
    data_path: PathBuf,
    two_d: bool,
}

fn resolve_processed(path: &Path) -> Option<ProcessedPaths> {
    if path.is_file() {
        let name = path.file_name()?.to_str()?;
        if matches!(name, "1r" | "1i" | "2rr") {
            let proc_dir = path.parent()?.to_path_buf();
            let two_d = name == "2rr" || proc_dir.join("2rr").is_file();
            let data_path = if two_d {
                proc_dir.join("2rr")
            } else {
                proc_dir.join("1r")
            };
            return (proc_dir.join("procs").is_file() && data_path.is_file()).then_some(
                ProcessedPaths {
                    proc_dir,
                    data_path,
                    two_d,
                },
            );
        }
        return None;
    }
    if !path.is_dir() {
        return None;
    }

    if path.join("procs").is_file() {
        return processed_in_proc_dir(path);
    }
    let pdata = if path.file_name().and_then(|s| s.to_str()) == Some("pdata") {
        path.to_path_buf()
    } else {
        path.join("pdata")
    };
    let mut proc_dirs: Vec<PathBuf> = std::fs::read_dir(pdata)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    proc_dirs.sort_by_key(|p| {
        let procno = p
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(u64::MAX);
        (procno != 1, procno, p.clone())
    });
    proc_dirs.iter().find_map(|p| processed_in_proc_dir(p))
}

fn processed_in_proc_dir(proc_dir: &Path) -> Option<ProcessedPaths> {
    if !proc_dir.join("procs").is_file() {
        return None;
    }
    let (data_path, two_d) = if proc_dir.join("2rr").is_file() && proc_dir.join("proc2s").is_file()
    {
        (proc_dir.join("2rr"), true)
    } else if proc_dir.join("1r").is_file() {
        (proc_dir.join("1r"), false)
    } else {
        return None;
    };
    Some(ProcessedPaths {
        proc_dir: proc_dir.to_path_buf(),
        data_path,
        two_d,
    })
}

pub fn load_processed(path: &Path) -> Result<LoadResult, IoError> {
    let resolved = resolve_processed(path).ok_or_else(|| {
        IoError::Unsupported(format!(
            "no complete Bruker processed dataset at {}",
            path.display()
        ))
    })?;
    let procs_path = resolved.proc_dir.join("procs");
    let procs = JcampParams::parse(&std::fs::read_to_string(&procs_path)?);
    let mut warnings = Vec::new();
    let (acquisition, format, mut parameter_paths) = if resolved.two_d {
        let proc2s_path = resolved.proc_dir.join("proc2s");
        let proc2s = JcampParams::parse(&std::fs::read_to_string(&proc2s_path)?);
        (
            Acquisition::D2(Box::new(read_processed_2d(
                &resolved.proc_dir,
                &resolved.data_path,
                &procs,
                &proc2s,
            )?)),
            DataFormat::BrukerProcessed2D,
            vec![procs_path, proc2s_path],
        )
    } else {
        let imag_path = resolved.proc_dir.join("1i");
        if !imag_path.is_file() {
            warnings.push(LoadWarning {
                code: LoadWarningCode::OptionalImaginaryMissing,
                message: "Bruker 1i is absent; phase correction is limited to the real channel"
                    .into(),
                path: Some(imag_path.clone()),
            });
        }
        (
            Acquisition::D1(read_processed_1d(
                &resolved.proc_dir,
                &resolved.data_path,
                imag_path.is_file().then_some(imag_path.as_path()),
                &procs,
            )?),
            DataFormat::BrukerProcessed1D,
            vec![procs_path],
        )
    };
    if let Some(acqus) = acquisition_params_for(&resolved.proc_dir, "acqus") {
        parameter_paths.push(acqus);
    }
    if resolved.two_d
        && let Some(acqu2s) = acquisition_params_for(&resolved.proc_dir, "acqu2s")
    {
        parameter_paths.push(acqu2s);
    }
    Ok(LoadResult {
        acquisition,
        format,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path: resolved.data_path,
            parameter_paths,
        },
        warnings,
    })
}

fn acquisition_params_for(proc_dir: &Path, name: &str) -> Option<PathBuf> {
    proc_dir
        .parent()?
        .parent()
        .map(|experiment| experiment.join(name))
        .filter(|p| p.is_file())
}

fn processed_sample(params: &JcampParams) -> SampleFmt {
    match params.i64("DTYPP").unwrap_or(0) {
        2 => SampleFmt::F64,
        _ => SampleFmt::I32,
    }
}

fn processed_endian(params: &JcampParams) -> Endian {
    match params.i64("BYTORDP").unwrap_or(0) {
        1 => Endian::Big,
        _ => Endian::Little,
    }
}

fn read_processed_values(
    path: &Path,
    count: usize,
    params: &JcampParams,
) -> Result<Vec<f64>, IoError> {
    let sample = processed_sample(params);
    let bytes = std::fs::read(path)?;
    let need = count
        .checked_mul(sample.size())
        .ok_or_else(|| IoError::Unsupported("processed SI overflow".into()))?;
    if bytes.len() < need {
        return Err(IoError::Truncated {
            offset: 0,
            needed: need,
            have: bytes.len(),
        });
    }
    let reader = Reader {
        bytes: &bytes,
        endian: processed_endian(params),
    };
    let scale = 2.0f64.powi(
        params
            .i64("NC_proc")
            .unwrap_or(0)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
    );
    Ok((0..count)
        .map(|i| reader.real(i * sample.size(), sample) * scale)
        .collect())
}

fn processed_dim(params: &JcampParams) -> Dim {
    let observe_freq_mhz = params
        .f64("SF")
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or(400.0);
    let spectral_width_hz = params
        .f64("SW_p")
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(observe_freq_mhz * 20.0);
    let offset = params.f64("OFFSET").unwrap_or(0.0);
    let nucleus = params
        .string("AXNUC")
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|s| !s.is_empty() && s != "off")
        .unwrap_or_else(|| guess_nucleus(observe_freq_mhz));
    Dim {
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm: offset - spectral_width_hz / (2.0 * observe_freq_mhz),
        nucleus,
        group_delay: 0.0,
    }
}

fn read_processed_1d(
    proc_dir: &Path,
    real_path: &Path,
    imag_path: Option<&Path>,
    params: &JcampParams,
) -> Result<NmrData, IoError> {
    let si = params
        .usize("SI")
        .filter(|n| *n > 0)
        .ok_or_else(|| IoError::Unsupported("Bruker procs has no positive SI".into()))?;
    let real = read_processed_values(real_path, si, params)?;
    let imag = imag_path
        .map(|p| read_processed_values(p, si, params))
        .transpose()?;
    let mut points: Vec<Complex64> = (0..si)
        .map(|i| Complex64::new(real[i], imag.as_ref().map_or(0.0, |v| v[i])))
        .collect();
    points.reverse();
    let dim = processed_dim(params);
    Ok(NmrData {
        points,
        domain: Domain::Frequency,
        spectral_width_hz: dim.spectral_width_hz,
        observe_freq_mhz: dim.observe_freq_mhz,
        carrier_ppm: dim.carrier_ppm,
        nucleus: dim.nucleus,
        source: format!(
            "{} (Bruker TopSpin processed 1D, {si} pts)",
            processed_source_prefix(proc_dir)
        ),
        group_delay: 0.0,
    })
}

fn read_processed_2d(
    proc_dir: &Path,
    data_path: &Path,
    f2: &JcampParams,
    f1: &JcampParams,
) -> Result<NmrData2D, IoError> {
    let cols = f2
        .usize("SI")
        .filter(|n| *n > 0)
        .ok_or_else(|| IoError::Unsupported("Bruker procs has no positive SI".into()))?;
    let rows = f1
        .usize("SI")
        .filter(|n| *n > 0)
        .ok_or_else(|| IoError::Unsupported("Bruker proc2s has no positive SI".into()))?;
    let stored = read_processed_values(
        data_path,
        rows.checked_mul(cols)
            .ok_or_else(|| IoError::Unsupported("processed 2D SI overflow".into()))?,
        f2,
    )?;
    let mut data = Vec::with_capacity(stored.len());
    for r in (0..rows).rev() {
        for c in (0..cols).rev() {
            data.push(Complex64::new(stored[r * cols + c], 0.0));
        }
    }
    Ok(NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Frequency,
        direct: processed_dim(f2),
        indirect: processed_dim(f1),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: format!(
            "{} (Bruker TopSpin processed 2D, {cols}x{rows})",
            processed_source_prefix(proc_dir)
        ),
    })
}

fn processed_source_prefix(proc_dir: &Path) -> String {
    let experiment = proc_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent);
    experiment
        .map(source_prefix)
        .unwrap_or_else(|| proc_dir.display().to_string())
}
