//! Bruker TopSpin acquisition reader — a directory of a binary `fid` plus a
//! text `acqus` parameter file.

use crate::{
    Acquisition, DataFormat, Dim, Domain, IoError, LoadResult, LoadWarning, LoadWarningCode,
    NmrData, NmrData2D, Provenance, QuadMode,
};
use num_complex::Complex64;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// Each 1D FID within a `ser` file is padded so its byte length is a multiple of
// this block size (256 four-byte words).
const SER_BLOCK_BYTES: usize = 1024;

mod processed;

pub use processed::{detect_processed, load_processed};

/// A directory holding a Bruker acquisition: an `acqus` parameter file next to a
/// binary `fid` (1D) or `ser` (nD) data file.
pub fn is_bruker_dir(path: &Path) -> bool {
    path.is_dir()
        && path.join("acqus").is_file()
        && (path.join("fid").is_file() || path.join("ser").is_file())
}

/// A Bruker acquisition selected either as its directory or directly as the
/// `fid`/`ser` data file inside it (whose parent holds the `acqus`).
pub fn is_bruker(path: &Path) -> bool {
    if path.is_dir() {
        return is_bruker_dir(path);
    }
    matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some("fid" | "ser")
    ) && path
        .parent()
        .map(|d| d.join("acqus").is_file())
        .unwrap_or(false)
}

// Resolve a user-selected path to (acquisition dir, binary data file). A
// directory prefers `fid` over `ser`; a file is taken as-is with its parent as
// the acquisition dir.
fn resolve_bruker(path: &Path) -> (PathBuf, PathBuf) {
    if path.is_dir() {
        let fid = path.join("fid");
        let data = if fid.is_file() { fid } else { path.join("ser") };
        (path.to_path_buf(), data)
    } else {
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (dir, path.to_path_buf())
    }
}

// A readable dataset label for an acquisition dir `<sample>/<expno>`: the
// processed-data title (or the sample folder as a fallback), always tagged with
// the numeric expno since one sample folder holds many experiments.
fn source_prefix(dir: &Path) -> String {
    let expno = dir.file_name().and_then(|s| s.to_str());
    let name = pdata_title(dir).or_else(|| {
        dir.parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .map(str::to_owned)
    });
    match (name, expno) {
        (Some(name), Some(expno)) => format!("{name} (expno {expno})"),
        (Some(name), None) => name,
        (None, Some(expno)) => expno.to_owned(),
        (None, None) => "<bruker>".to_owned(),
    }
}

// The first non-empty line of a processed-data `title` file, preferring proc no.
// 1 and otherwise the lowest-numbered proc dir carrying a non-empty title.
fn pdata_title(dir: &Path) -> Option<String> {
    let mut procs: Vec<PathBuf> = std::fs::read_dir(dir.join("pdata"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    procs.sort_by_key(|p| {
        p.file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    procs.iter().find_map(|proc| {
        let text = std::fs::read_to_string(proc.join("title")).ok()?;
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_owned)
    })
}

pub fn read_bruker(path: &Path) -> Result<Acquisition, IoError> {
    let (dir, data_path) = resolve_bruker(path);
    let acqus_path = dir.join("acqus");
    let params = JcampParams::parse(&std::fs::read_to_string(&acqus_path)?);

    // A `ser` file alongside an `acqu2s` is a 2D (or nD) acquisition.
    let acqu2s_path = dir.join("acqu2s");
    let is_ser = data_path.file_name().and_then(|s| s.to_str()) == Some("ser");
    if is_ser && acqu2s_path.is_file() {
        return read_bruker_2d(&dir, &data_path, &params, &acqu2s_path)
            .map(|d| Acquisition::D2(Box::new(d)));
    }

    read_bruker_1d(&dir, &data_path, &params).map(Acquisition::D1)
}

pub fn load_raw(path: &Path) -> Result<LoadResult, IoError> {
    let (dir, data_path) = resolve_bruker(path);
    let mut parameter_paths = vec![dir.join("acqus")];
    if data_path.file_name().and_then(|s| s.to_str()) == Some("ser") && dir.join("acqu2s").is_file()
    {
        parameter_paths.push(dir.join("acqu2s"));
    }
    Ok(LoadResult {
        acquisition: read_bruker(path)?,
        format: DataFormat::BrukerRaw,
        provenance: Provenance {
            selected_path: path.to_path_buf(),
            data_path,
            parameter_paths,
            companion_paths: Vec::new(),
        },
        warnings: Vec::new(),
    })
}

fn read_bruker_1d(dir: &Path, fid_path: &Path, params: &JcampParams) -> Result<NmrData, IoError> {
    // TD counts individual real values, so complex points = TD/2.
    let td = params.usize("TD").unwrap_or(0);
    if td < 2 {
        return Err(IoError::Unsupported(format!(
            "acqus reports TD={td} (need at least one complex point)"
        )));
    }
    let n_complex = td / 2;

    let byte_order = match params.i64("BYTORDA").unwrap_or(0) {
        1 => Endian::Big,
        _ => Endian::Little,
    };
    let sample = match params.i64("DTYPA").unwrap_or(0) {
        2 => SampleFmt::F64,
        _ => SampleFmt::I32,
    };
    let stride = sample.size();

    let bytes = std::fs::read(fid_path)?;
    let need = n_complex
        .checked_mul(2 * stride)
        .ok_or_else(|| IoError::Unsupported("TD overflow".into()))?;
    if bytes.len() < need {
        return Err(IoError::Truncated {
            offset: 0,
            needed: need,
            have: bytes.len(),
        });
    }

    // De-interleave (re, im, re, im, …) into complex points.
    let r = Reader {
        bytes: &bytes,
        endian: byte_order,
    };
    let points: Vec<Complex64> = (0..n_complex)
        .map(|i| {
            let base = i * 2 * stride;
            Complex64::new(r.real(base, sample), r.real(base + stride, sample))
        })
        .collect();

    let spectral_width_hz = params
        .f64("SW_h")
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(0.0);
    // SFO1 is the observed (Larmor) frequency; BF1 is the 0-ppm reference.
    let observe_freq_mhz = params
        .f64("SFO1")
        .or_else(|| params.f64("BF1"))
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or(400.0);
    let bf1 = params
        .f64("BF1")
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or(observe_freq_mhz);
    let carrier_ppm = params.f64("O1").map(|o1| o1 / bf1).unwrap_or(0.0);

    let nucleus = params
        .string("NUC1")
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|s| !s.is_empty() && s != "off")
        .unwrap_or_else(|| guess_nucleus(observe_freq_mhz));

    let group_delay = group_delay(params);

    let source = format!(
        "{} (Bruker TopSpin, {sample:?}, {n_complex} pts)",
        source_prefix(dir)
    );

    Ok(NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: if spectral_width_hz > 0.0 {
            spectral_width_hz
        } else {
            observe_freq_mhz * 20.0
        },
        observe_freq_mhz,
        carrier_ppm,
        nucleus,
        source,
        group_delay,
    })
}

fn read_bruker_2d(
    dir: &Path,
    ser_path: &Path,
    f2: &JcampParams,
    acqu2s_path: &Path,
) -> Result<NmrData2D, IoError> {
    let f1 = JcampParams::parse(&std::fs::read_to_string(acqu2s_path)?);

    let td2 = f2.usize("TD").unwrap_or(0);
    let rows = f1.usize("TD").unwrap_or(0);
    if td2 < 2 || rows == 0 {
        return Err(IoError::Unsupported(format!(
            "acqus/acqu2s report TD={td2}, TD1={rows} (need a non-empty 2D)"
        )));
    }
    let cols = td2 / 2;

    let byte_order = match f2.i64("BYTORDA").unwrap_or(0) {
        1 => Endian::Big,
        _ => Endian::Little,
    };
    let sample = match f2.i64("DTYPA").unwrap_or(0) {
        2 => SampleFmt::F64,
        _ => SampleFmt::I32,
    };
    let stride = sample.size();

    // Each stored row is `td2` reals padded up to a whole number of blocks.
    let row_bytes = td2
        .checked_mul(stride)
        .map(|b| b.div_ceil(SER_BLOCK_BYTES) * SER_BLOCK_BYTES)
        .ok_or_else(|| IoError::Unsupported("TD overflow".into()))?;
    let bytes = std::fs::read(ser_path)?;
    let need = rows
        .checked_mul(row_bytes)
        .ok_or_else(|| IoError::Unsupported("ser size overflow".into()))?;
    if bytes.len() < need {
        return Err(IoError::Truncated {
            offset: 0,
            needed: need,
            have: bytes.len(),
        });
    }

    let r = Reader {
        bytes: &bytes,
        endian: byte_order,
    };
    let mut data = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        let base = row * row_bytes;
        for i in 0..cols {
            let off = base + i * 2 * stride;
            data.push(Complex64::new(
                r.real(off, sample),
                r.real(off + stride, sample),
            ));
        }
    }

    let quad = match f1.i64("FnMODE").unwrap_or(0) {
        4 => QuadMode::States,
        5 => QuadMode::StatesTppi,
        6 => QuadMode::EchoAntiecho,
        _ => QuadMode::Complex,
    };

    let direct = dim_from(f2, group_delay(f2));
    let indirect = dim_from(&f1, 0.0);

    let experiment = f2
        .string("PULPROG")
        .map(|s| {
            s.trim_matches(|c| c == '<' || c == '>')
                .to_ascii_lowercase()
        })
        .filter(|s| !s.is_empty());

    let source = format!(
        "{} (Bruker TopSpin 2D, {sample:?}, {cols}×{rows})",
        source_prefix(dir)
    );

    Ok(NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct,
        indirect,
        quad,
        indirect_conjugate: false,
        experiment,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source,
    })
}

fn dim_from(p: &JcampParams, group_delay: f64) -> Dim {
    let observe_freq_mhz = p
        .f64("SFO1")
        .or_else(|| p.f64("BF1"))
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or(400.0);
    let bf1 = p
        .f64("BF1")
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or(observe_freq_mhz);
    let spectral_width_hz = p
        .f64("SW_h")
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(observe_freq_mhz * 20.0);
    let nucleus = p
        .string("NUC1")
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|s| !s.is_empty() && s != "off")
        .unwrap_or_else(|| guess_nucleus(observe_freq_mhz));
    Dim {
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm: p.f64("O1").map(|o1| o1 / bf1).unwrap_or(0.0),
        nucleus,
        group_delay,
    }
}

// Group delay in points: an explicit `GRPDLY` when present, else a lookup from
// the (`DSPFVS`, `DECIM`) table for older data.
fn group_delay(params: &JcampParams) -> f64 {
    if let Some(g) = params.f64("GRPDLY")
        && g.is_finite()
        && g >= 0.0
    {
        return g;
    }
    let dspfvs = params.i64("DSPFVS").unwrap_or(-1);
    let decim = params.i64("DECIM").unwrap_or(-1);
    grpdly_from_table(dspfvs, decim).unwrap_or(0.0)
}

fn guess_nucleus(mhz: f64) -> String {
    if mhz > 300.0 {
        "1H".into()
    } else if mhz > 90.0 {
        "13C".into()
    } else {
        "X".into()
    }
}

// Parsed JCAMP-DX `acqus`: scalar `##$KEY= value` entries. Array-valued
// parameters (`##$KEY= (0..N)` then value lines) are skipped.
struct JcampParams {
    map: HashMap<String, String>,
}

impl JcampParams {
    fn parse(text: &str) -> Self {
        let mut map = HashMap::new();
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("##$") else {
                continue;
            };
            let Some((key, val)) = rest.split_once('=') else {
                continue;
            };
            let val = val.trim();
            // Array declarations like "(0..15)" carry their payload on following
            // lines, which are not consumed.
            if val.starts_with('(') {
                continue;
            }
            map.insert(key.trim().to_string(), val.to_string());
        }
        Self { map }
    }

    fn string(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }

    fn f64(&self, key: &str) -> Option<f64> {
        self.map.get(key)?.parse().ok()
    }

    fn i64(&self, key: &str) -> Option<i64> {
        self.map.get(key)?.parse().ok()
    }

    fn usize(&self, key: &str) -> Option<usize> {
        self.map.get(key)?.parse().ok()
    }
}

// Standard Bruker group-delay table for older data (`DSPFVS` 10–13), keyed by
// `DECIM`.
#[allow(clippy::excessive_precision)]
fn grpdly_from_table(dspfvs: i64, decim: i64) -> Option<f64> {
    let row: &[(i64, f64)] = match dspfvs {
        10 => &[
            (2, 44.75),
            (3, 33.5),
            (4, 66.625),
            (6, 59.083333333333333),
            (8, 68.5625),
            (12, 60.375),
            (16, 69.53125),
            (24, 61.020833333333333),
            (32, 70.015625),
            (48, 61.34375),
            (64, 70.2578125),
            (96, 61.505208333333333),
            (128, 70.37890625),
            (192, 61.5859375),
            (256, 70.439453125),
            (384, 61.626302083333333),
            (512, 70.4697265625),
            (768, 61.646484375),
            (1024, 70.48486328125),
            (1536, 61.656575520833333),
            (2048, 70.4924316406250),
        ],
        11 => &[
            (2, 46.0),
            (3, 36.5),
            (4, 48.0),
            (6, 50.166666666666667),
            (8, 53.25),
            (12, 69.5),
            (16, 72.25),
            (24, 70.166666666666667),
            (32, 72.75),
            (48, 70.5),
            (64, 73.0),
            (96, 70.666666666666667),
            (128, 72.5),
            (192, 71.333333333333333),
            (256, 72.25),
            (384, 71.666666666666667),
            (512, 72.125),
            (768, 71.833333333333333),
            (1024, 72.0625),
            (1536, 71.916666666666667),
            (2048, 72.03125),
        ],
        12 => &[
            (2, 46.0),
            (3, 36.5),
            (4, 48.0),
            (6, 50.166666666666667),
            (8, 53.25),
            (12, 69.5),
            (16, 71.625),
            (24, 70.166666666666667),
            (32, 72.125),
            (48, 70.5),
            (64, 72.375),
            (96, 70.666666666666667),
            (128, 72.5),
            (192, 71.333333333333333),
            (256, 72.25),
            (384, 71.666666666666667),
            (512, 72.125),
            (768, 71.833333333333333),
            (1024, 72.0625),
            (1536, 71.916666666666667),
            (2048, 72.03125),
        ],
        13 => &[
            (2, 2.75),
            (3, 2.8333333333333333),
            (4, 2.875),
            (6, 2.9166666666666667),
            (8, 2.9375),
            (12, 2.9583333333333333),
            (16, 2.96875),
            (24, 2.9791666666666667),
            (32, 2.984375),
            (48, 2.9895833333333333),
            (64, 2.9921875),
            (96, 2.9947916666666667),
        ],
        _ => return None,
    };
    row.iter().find(|(d, _)| *d == decim).map(|(_, g)| *g)
}

#[derive(Debug, Clone, Copy)]
enum SampleFmt {
    I32,
    F64,
}

impl SampleFmt {
    #[inline]
    fn size(self) -> usize {
        match self {
            SampleFmt::I32 => 4,
            SampleFmt::F64 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Endian {
    Big,
    Little,
}

struct Reader<'a> {
    bytes: &'a [u8],
    endian: Endian,
}

impl Reader<'_> {
    fn real(&self, at: usize, fmt: SampleFmt) -> f64 {
        match fmt {
            SampleFmt::I32 => {
                let b: [u8; 4] = self.bytes[at..at + 4].try_into().unwrap();
                let v = match self.endian {
                    Endian::Big => i32::from_be_bytes(b),
                    Endian::Little => i32::from_le_bytes(b),
                };
                v as f64
            }
            SampleFmt::F64 => {
                let b: [u8; 8] = self.bytes[at..at + 8].try_into().unwrap();
                match self.endian {
                    Endian::Big => f64::from_be_bytes(b),
                    Endian::Little => f64::from_le_bytes(b),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalar_and_skips_arrays() {
        let text = "\
##TITLE= params
##$TD= 16384
##$NUC1= <1H>
##$SW_h= 9615.38461538464
##$GRPDLY= 76
##$XGF= (0..3)
0 0 0 0
##$O1= 2820.61
";
        let p = JcampParams::parse(text);
        assert_eq!(p.usize("TD"), Some(16384));
        assert_eq!(p.string("NUC1").as_deref(), Some("<1H>"));
        assert_eq!(p.f64("GRPDLY"), Some(76.0));
        assert_eq!(p.f64("O1"), Some(2820.61));
        assert_eq!(p.string("XGF"), None);
    }

    #[test]
    fn group_delay_prefers_explicit_grpdly() {
        let p = JcampParams::parse("##$GRPDLY= 67.98\n##$DSPFVS= 21\n##$DECIM= 2080\n");
        assert!((group_delay(&p) - 67.98).abs() < 1e-9);
    }

    #[test]
    fn group_delay_falls_back_to_table() {
        let p = JcampParams::parse("##$GRPDLY= -1\n##$DSPFVS= 12\n##$DECIM= 16\n");
        assert!((group_delay(&p) - 71.625).abs() < 1e-9);
    }

    #[test]
    fn accepts_dir_or_fid_file() {
        let dir = std::env::temp_dir().join(format!("plotx_bruker_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("acqus"),
            "##$TD= 4\n##$DTYPA= 2\n##$BYTORDA= 0\n##$SW_h= 1000\n##$SFO1= 400\n##$BF1= 400\n",
        )
        .unwrap();
        let mut fid = Vec::new();
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            fid.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(dir.join("fid"), &fid).unwrap();

        assert!(is_bruker(&dir));
        assert!(is_bruker(&dir.join("fid")));

        let unwrap1d = |a: Acquisition| match a {
            Acquisition::D1(d) => d,
            Acquisition::D2(_) => panic!("expected 1D"),
            Acquisition::Electrophysiology(_) => panic!("expected NMR"),
            Acquisition::Afm(_) => panic!("expected NMR"),
        };
        let from_dir = unwrap1d(read_bruker(&dir).unwrap());
        let from_file = unwrap1d(read_bruker(&dir.join("fid")).unwrap());
        assert_eq!(
            from_dir.points,
            vec![Complex64::new(1.0, 2.0), Complex64::new(3.0, 4.0)]
        );
        assert_eq!(from_dir.points, from_file.points);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_a_hand_built_2d_ser() {
        let dir = std::env::temp_dir().join(format!("plotx_bruker2d_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("acqus"),
            "##$TD= 4\n##$DTYPA= 2\n##$BYTORDA= 0\n##$SW_h= 1000\n##$SFO1= 600\n##$BF1= 600\n\
             ##$O1= 1200\n##$NUC1= <1H>\n##$GRPDLY= 0\n##$PULPROG= <cosygpppqf>\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("acqu2s"),
            "##$TD= 2\n##$SW_h= 1000\n##$SFO1= 600\n##$BF1= 600\n##$O1= 1200\n##$NUC1= <1H>\n\
             ##$FnMODE= 4\n",
        )
        .unwrap();

        // Two rows; each 1D FID (4 reals) padded to a 1024-byte block.
        let mut ser = vec![0u8; 2 * SER_BLOCK_BYTES];
        for (row, vals) in [[1.0f64, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]
            .iter()
            .enumerate()
        {
            for (i, v) in vals.iter().enumerate() {
                let off = row * SER_BLOCK_BYTES + i * 8;
                ser[off..off + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(dir.join("ser"), &ser).unwrap();

        let two = match read_bruker(&dir).unwrap() {
            Acquisition::D2(d) => *d,
            Acquisition::D1(_) => panic!("expected 2D"),
            Acquisition::Electrophysiology(_) => panic!("expected NMR"),
            Acquisition::Afm(_) => panic!("expected NMR"),
        };
        assert_eq!((two.cols, two.rows), (2, 2));
        assert_eq!(
            two.data,
            vec![
                Complex64::new(1.0, 2.0),
                Complex64::new(3.0, 4.0),
                Complex64::new(5.0, 6.0),
                Complex64::new(7.0, 8.0),
            ]
        );
        assert_eq!(two.quad, QuadMode::States);
        assert!(!two.indirect_conjugate);
        assert_eq!(two.experiment.as_deref(), Some("cosygpppqf"));
        assert!((two.direct.carrier_ppm - 2.0).abs() < 1e-9);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn source_prefix_prefers_title_then_sample_folder() {
        let base = std::env::temp_dir().join(format!("plotx_bruker_name_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let expno = base.join("Sucrose").join("3");
        std::fs::create_dir_all(expno.join("pdata").join("1")).unwrap();
        std::fs::create_dir_all(expno.join("pdata").join("2")).unwrap();

        // An empty proc-1 title falls through to the next-lowest proc.
        std::fs::write(expno.join("pdata").join("1").join("title"), "   \n").unwrap();
        std::fs::write(
            expno.join("pdata").join("2").join("title"),
            "\nProton in CDCl3\n",
        )
        .unwrap();
        assert_eq!(source_prefix(&expno), "Proton in CDCl3 (expno 3)");

        // A non-empty proc-1 title wins.
        std::fs::write(expno.join("pdata").join("1").join("title"), "Sucrose 1H\n").unwrap();
        assert_eq!(source_prefix(&expno), "Sucrose 1H (expno 3)");

        // With no titles at all, the sample folder names the dataset.
        std::fs::remove_file(expno.join("pdata").join("1").join("title")).unwrap();
        std::fs::remove_file(expno.join("pdata").join("2").join("title")).unwrap();
        assert_eq!(source_prefix(&expno), "Sucrose (expno 3)");

        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn deinterleaves_complex_f64() {
        // TD = 4 real values → 2 complex points: (1+2i), (3+4i).
        let mut buf = Vec::new();
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        let r = Reader {
            bytes: &buf,
            endian: Endian::Little,
        };
        let p0 = Complex64::new(r.real(0, SampleFmt::F64), r.real(8, SampleFmt::F64));
        let p1 = Complex64::new(r.real(16, SampleFmt::F64), r.real(24, SampleFmt::F64));
        assert_eq!(p0, Complex64::new(1.0, 2.0));
        assert_eq!(p1, Complex64::new(3.0, 4.0));
    }
}
