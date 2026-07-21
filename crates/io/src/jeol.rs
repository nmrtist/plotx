//! JEOL Delta `.jdf` reader.

use crate::{
    Acquisition, DiffusionMeta, Dim, Domain, IoError, NmrData, NmrData2D, PseudoAxis, PseudoKind,
    QuadMode, gradient_shape_factor, gyromagnetic_ratio,
};
use num_complex::Complex64;
use std::collections::HashMap;
use std::path::Path;

mod filter;
mod nus;
mod ruler;
use filter::group_delay;
use nus::detect_nus;
#[cfg(test)]
use nus::extract_nuslist;
use ruler::{ascii_trim, kind_for_unit, prefix_exponent, scan_embedded_axis};

const MAGIC: &[u8; 8] = b"JEOL.NMR";
const HEADER_LEN: usize = 1360;

// Byte offsets into the fixed big-endian header. Array fields hold one slot per
// possible dimension; slot 0 is read for 1D data.
#[allow(dead_code)]
mod off {
    pub const ENDIAN: usize = 8; // u8: body endianness, 0 = big, 1 = little
    pub const MAJOR_VERSION: usize = 9; // u8
    pub const DATA_DIMENSION_NUMBER: usize = 12; // u8
    pub const DATA_AXIS_TYPE: usize = 24; // 8 × u8 (0 None, 1 Real, 3 Complex, ...)
    pub const DATA_POINTS: usize = 176; // 8 × u32 (per axis, padded to a tile edge)
    pub const DATA_OFFSET_STOP: usize = 240; // 8 × u32 (per axis, last real index)
    pub const DATA_AXIS_START: usize = 272; // 8 × f64 (axis low end)
    pub const DATA_AXIS_STOP: usize = 336; // 8 × f64 (axis high end; for a FID = acq time, s)
    pub const BASE_FREQ: usize = 1064; // 8 × f64 (MHz)
    pub const PARAM_LIST: usize = 1360; // parameter-list header, right after the fixed header
    pub const DATA_START: usize = 1284; // u32: byte offset of the data section
    pub const DATA_LENGTH: usize = 1288; // u64: length of the data section in bytes
}

const AXIS_COMPLEX: u8 = 3;
const AXIS_REAL_COMPLEX: u8 = 4;

// Edge of the square submatrix tiles nD data is stored in. Data_Points are
// padded up to a multiple of this along every axis. True-2D data uses the 32
// edge; pseudo-2D arrays with few increments use the 4 edge.
const TILE: usize = 32;
const SMALL_TILE: usize = 4;

/// True if the file begins with the JEOL Delta magic, regardless of extension.
pub fn is_jdf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; MAGIC.len()];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|()| &magic == MAGIC)
        .unwrap_or(false)
}

pub fn read_jdf_path(path: &Path) -> Result<Acquisition, IoError> {
    let bytes = std::fs::read(path)?;
    let source = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<jdf>")
        .to_string();
    read_jdf_bytes(&bytes, source)
}

pub fn read_jdf_bytes(bytes: &[u8], source: String) -> Result<Acquisition, IoError> {
    if bytes.len() < HEADER_LEN {
        return Err(IoError::Truncated {
            offset: 0,
            needed: HEADER_LEN,
            have: bytes.len(),
        });
    }
    if &bytes[..8] != MAGIC {
        return Err(IoError::BadMagic);
    }

    let body_endian = match bytes[off::ENDIAN] {
        0 => Endian::Big,
        1 => Endian::Little,
        other => {
            return Err(IoError::Unsupported(format!(
                "unknown endian marker {other} at byte 8"
            )));
        }
    };

    match bytes[off::DATA_DIMENSION_NUMBER] {
        1 => read_jdf_1d(bytes, source, body_endian).map(Acquisition::D1),
        2 => read_jdf_2d(bytes, source, body_endian).map(|d| Acquisition::D2(Box::new(d))),
        ndim => Err(IoError::Unsupported(format!(
            "{ndim}-dimensional data (only 1D and 2D are implemented)"
        ))),
    }
}

fn read_jdf_1d(bytes: &[u8], source: String, body_endian: Endian) -> Result<NmrData, IoError> {
    // The fixed header is always big-endian; the body follows the Endian byte.
    let h = Reader {
        bytes,
        endian: Endian::Big,
    };

    let axis_type = bytes[off::DATA_AXIS_TYPE];
    let components = match axis_type {
        AXIS_COMPLEX | AXIS_REAL_COMPLEX => 2,
        _ => 1,
    };

    let npoints = h.u32(off::DATA_POINTS) as usize;
    if npoints == 0 {
        return Err(IoError::Unsupported(
            "header reports zero data points".into(),
        ));
    }
    // DATA_POINTS is padded to a tile edge; DATA_OFFSET_STOP is the last real
    // index, so real count = stop+1 (mirrors the 2D path); reading the padded count
    // pulls in trailing zeros and scales the sweep width. Fall back when unset (0).
    let real_n = {
        let stop = h.u32(off::DATA_OFFSET_STOP) as usize;
        if stop > 0 {
            (stop + 1).min(npoints)
        } else {
            npoints
        }
    };

    let base_freq_mhz = h.f64(off::BASE_FREQ);
    // For a FID this axis is the acquisition time in seconds (start → stop).
    let acq_time_s = (h.f64(off::DATA_AXIS_STOP) - h.f64(off::DATA_AXIS_START)).abs();

    let params = Params::parse(bytes, off::PARAM_LIST, body_endian);

    let data_start = {
        let ds = h.u32(off::DATA_START) as usize;
        if ds >= HEADER_LEN && ds < bytes.len() {
            ds
        } else {
            HEADER_LEN
        }
    };
    let data_length = h.u64(off::DATA_LENGTH) as usize;

    // Sample width (f32/f64) comes from the data-section byte budget, requiring an
    // exact f32 or f64 fit rather than an ambiguous type nibble or a size guess.
    let total_samples = npoints
        .checked_mul(components)
        .ok_or_else(|| IoError::Unsupported("point count overflow".into()))?;
    let avail = bytes.len().saturating_sub(data_start);
    let budget = if data_length > 0 && data_length <= avail {
        data_length
    } else {
        avail
    };
    let sample = sample_format(budget, total_samples)?;
    let stride = sample.size();

    let need = data_start
        .checked_add(total_samples * stride)
        .ok_or_else(|| IoError::Unsupported("data section size overflow".into()))?;
    if bytes.len() < need {
        return Err(IoError::Truncated {
            offset: data_start,
            needed: total_samples * stride,
            have: avail,
        });
    }

    let d = Reader {
        bytes,
        endian: body_endian,
    };
    // Real then imaginary channel, each padded to `npoints`; read only the real
    // extent, but the imaginary channel still starts after the full padded block.
    let real = d.read_reals(data_start, real_n, sample);
    let imag = if components == 2 {
        d.read_reals(data_start + npoints * stride, real_n, sample)
    } else {
        vec![0.0; real_n]
    };
    // JEOL stores the FID with the opposite quadrature sense to a naive forward
    // FFT; conjugating it here (negating the imaginary channel) makes a plain
    // forward FFT downstream yield the correct ppm ordering.
    let points: Vec<Complex64> = real
        .into_iter()
        .zip(imag)
        .map(|(re, im)| Complex64::new(re, -im))
        .collect();

    let observe_freq_mhz = if base_freq_mhz.is_finite() && base_freq_mhz > 1.0 {
        base_freq_mhz
    } else {
        400.0
    };
    // Sweep width = 1/dwell; the last FID point sits at (N-1)·dwell = acq_time.
    let spectral_width_hz = if acq_time_s.is_finite() && acq_time_s > 0.0 && real_n > 1 {
        (real_n as f64 - 1.0) / acq_time_s
    } else {
        observe_freq_mhz * 20.0
    };
    let carrier_ppm = params.f64("X_OFFSET").unwrap_or(0.0);

    let nucleus = params
        .string("X_DOMAIN")
        .map(|s| normalize_nucleus(&s))
        .unwrap_or_else(|| guess_nucleus(observe_freq_mhz));
    let solvent = params.string("SOLVENT").unwrap_or_default();
    let provenance = if solvent.is_empty() {
        format!("{source} (JEOL Delta, {sample:?}, {real_n} pts)")
    } else {
        format!("{source} (JEOL Delta, {solvent}, {real_n} pts)")
    };

    Ok(NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm,
        nucleus,
        source: provenance,
        group_delay: group_delay(&params),
    })
}

// nD data is stored as square submatrix tiles of edge `TILE`. For a 2D dataset
// with a complex direct (F2) axis and a real indirect (F1) axis there are two
// planes — all F2-real tiles, then all F2-imag tiles. Within a plane the F1
// tile-block is the outer loop and the F2 tile-block the inner, and each tile is
// row-major. The imaginary plane is negated relative to a forward-FFT
// convention, so the complex value is `re - i·im` (the same conjugation the 1D
// reader applies).
fn read_jdf_2d(bytes: &[u8], source: String, body_endian: Endian) -> Result<NmrData2D, IoError> {
    let h = Reader {
        bytes,
        endian: Endian::Big,
    };
    let axis_u32 = |base: usize, i: usize| h.u32(base + i * 4) as usize;

    let cols_pad = axis_u32(off::DATA_POINTS, 0);
    let rows_pad = axis_u32(off::DATA_POINTS, 1);
    if cols_pad == 0 || rows_pad == 0 {
        return Err(IoError::Unsupported(
            "header reports zero data points".into(),
        ));
    }
    // Real (non-padding) extent; the offset-stop is the last valid index.
    let cols_real = (axis_u32(off::DATA_OFFSET_STOP, 0) + 1).min(cols_pad);
    let rows_real = (axis_u32(off::DATA_OFFSET_STOP, 1) + 1).min(rows_pad);

    // JEOL stores nD data in square submatrix tiles. True-2D acquisitions use a
    // 32-point tile edge; pseudo-2D arrays with few increments (DOSY, T1/T2) use
    // a 4-point tile. Both loop the indirect (F1) tile-block outer and the direct
    // (F2) tile-block inner, row-major within each tile.
    let tile = if cols_pad % TILE == 0 && rows_pad % TILE == 0 {
        TILE
    } else {
        SMALL_TILE
    };
    if cols_pad % tile != 0 || rows_pad % tile != 0 {
        return Err(IoError::Unsupported(format!(
            "2D data points ({cols_pad}×{rows_pad}) are not a multiple of the {tile}-point tile edge"
        )));
    }

    let axis_kind = |i: usize| bytes[off::DATA_AXIS_TYPE + i];
    let f2_complex = matches!(axis_kind(0), AXIS_COMPLEX | AXIS_REAL_COMPLEX);
    // A `Complex` (type 3) indirect axis is States-style hypercomplex: the F1
    // cosine and sine modulations are acquired separately and stored as their own
    // sample planes, so the plane count doubles and the indirect FFT needs States
    // recombination. A `Real_Complex` (type 4) indirect axis is already a single
    // phase-modulated interferogram (one plane pair) recombined as plain Complex.
    let f1_hypercomplex = axis_kind(1) == AXIS_COMPLEX;
    let f2_planes = if f2_complex { 2 } else { 1 };
    let f1_planes = if f1_hypercomplex { 2 } else { 1 };
    let planes = f2_planes * f1_planes;

    let data_start = {
        let ds = h.u32(off::DATA_START) as usize;
        if ds >= HEADER_LEN && ds < bytes.len() {
            ds
        } else {
            HEADER_LEN
        }
    };
    let data_length = h.u64(off::DATA_LENGTH) as usize;

    let total_samples = cols_pad
        .checked_mul(rows_pad)
        .and_then(|v| v.checked_mul(planes))
        .ok_or_else(|| IoError::Unsupported("2D point count overflow".into()))?;
    let avail = bytes.len().saturating_sub(data_start);
    let budget = if data_length > 0 && data_length <= avail {
        data_length
    } else {
        avail
    };
    let sample = sample_format(budget, total_samples)?;
    let stride = sample.size();
    let need = data_start
        .checked_add(total_samples * stride)
        .ok_or_else(|| IoError::Unsupported("2D data section size overflow".into()))?;
    if bytes.len() < need {
        return Err(IoError::Truncated {
            offset: data_start,
            needed: total_samples * stride,
            have: avail,
        });
    }

    let d = Reader {
        bytes,
        endian: body_endian,
    };
    let n_f2_blocks = cols_pad / tile;
    let plane_len = rows_pad * cols_pad;
    let sample_at = |plane: usize, row: usize, col: usize| -> f64 {
        let block = (row / tile) * n_f2_blocks + (col / tile);
        let idx = plane * plane_len + block * tile * tile + (row % tile) * tile + (col % tile);
        d.sample(data_start + idx * stride, sample)
    };
    // Section plane index for (F1 imaginary?, F2 imaginary?); F2 toggles fastest,
    // matching the 2-plane (F2-only-complex) layout the 1D reader shares. JEOL's
    // imaginary plane is negated relative to a forward-FFT convention.
    let complex_at = |f1_imag: usize, row: usize, col: usize| -> Complex64 {
        let re = sample_at(f1_imag * f2_planes, row, col);
        let im = f2_complex.then(|| sample_at(f1_imag * f2_planes + 1, row, col));
        Complex64::new(re, -im.unwrap_or(0.0))
    };

    // For a hypercomplex indirect axis, interleave each increment's cosine
    // (F1-real) and sine (F1-imag) channel as consecutive rows so the indirect
    // FFT's States recombination pairs the 2k / 2k+1 rows into one t1 point.
    let f1_channels = if f1_hypercomplex { 2 } else { 1 };
    let mut data = Vec::with_capacity(f1_channels * rows_real * cols_real);
    for row in 0..rows_real {
        for f1_imag in 0..f1_channels {
            for col in 0..cols_real {
                data.push(complex_at(f1_imag, row, col));
            }
        }
    }
    let stored_rows = f1_channels * rows_real;
    let quad = if f1_hypercomplex {
        QuadMode::States
    } else {
        QuadMode::Complex
    };

    let params = Params::parse(bytes, off::PARAM_LIST, body_endian);
    let acq_time =
        |i: usize| (h.f64(off::DATA_AXIS_STOP + i * 8) - h.f64(off::DATA_AXIS_START + i * 8)).abs();
    let base_freq = |i: usize| {
        let f = h.f64(off::BASE_FREQ + i * 8);
        if f.is_finite() && f > 1.0 { f } else { 400.0 }
    };
    let sweep = |i: usize, real_n: usize| {
        let acq = acq_time(i);
        if acq.is_finite() && acq > 0.0 && real_n > 1 {
            (real_n as f64 - 1.0) / acq
        } else {
            base_freq(i) * 20.0
        }
    };
    // The indirect axis stores no usable t1 acquisition time (its `Data_Axis_Stop`
    // is not the increment span), so the acq-time estimate collapses the F1 sweep.
    // The `Y_SWEEP` parameter (SI Hz, scaler folded) is authoritative; fall back to
    // the acq-time estimate only when it is absent.
    let indirect_sweep = params
        .si("Y_SWEEP")
        .filter(|v| v.is_finite() && *v > 1.0)
        .unwrap_or_else(|| sweep(1, rows_real));
    let direct = Dim {
        spectral_width_hz: sweep(0, cols_real),
        observe_freq_mhz: base_freq(0),
        carrier_ppm: params.f64("X_OFFSET").unwrap_or(0.0),
        nucleus: params
            .string("X_DOMAIN")
            .map(|s| normalize_nucleus(&s))
            .unwrap_or_else(|| guess_nucleus(base_freq(0))),
        group_delay: group_delay(&params),
    };
    let indirect = Dim {
        spectral_width_hz: indirect_sweep,
        observe_freq_mhz: base_freq(1),
        carrier_ppm: params.f64("Y_OFFSET").unwrap_or(0.0),
        nucleus: params
            .string("Y_DOMAIN")
            .map(|s| normalize_nucleus(&s))
            .unwrap_or_else(|| guess_nucleus(base_freq(1))),
        group_delay: 0.0,
    };

    let experiment = params
        .string_ci("experiment")
        .or_else(|| params.string_ci("content"))
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    let (pseudo_axis, diffusion) = extract_pseudo(bytes, &params, &experiment, &direct, rows_real);
    let nus = detect_nus(bytes, &params, rows_real);

    Ok(NmrData2D {
        data,
        rows: stored_rows,
        cols: cols_real,
        domain: Domain::Time,
        direct,
        indirect,
        quad,
        indirect_conjugate: true,
        experiment,
        pseudo_axis,
        diffusion,
        nus,
        source: format!("{source} (JEOL Delta 2D, {sample:?}, {cols_real}×{rows_real})"),
    })
}

/// Recover the pseudo-2D indirect ruler and (for DOSY) the diffusion-encoding
/// parameters. The ruler comes from the embedded experiment text; diffusion
/// scalars come from the SI-normalized parameter list.
fn extract_pseudo(
    bytes: &[u8],
    params: &Params,
    experiment: &Option<String>,
    direct: &Dim,
    rows: usize,
) -> (Option<PseudoAxis>, Option<DiffusionMeta>) {
    let axis = scan_embedded_axis(bytes).map(|(name, mut values, unit, source)| {
        // Trust the stored row count over a ramp that rounded to a different length.
        if values.len() > rows && rows > 0 {
            values.truncate(rows);
        }
        PseudoAxis {
            kind: kind_for_unit(&unit),
            name,
            values,
            unit,
            source,
        }
    });

    let hint = experiment.as_deref().unwrap_or("");
    let looks_dosy = axis
        .as_ref()
        .map(|a| a.kind == PseudoKind::Gradient)
        .unwrap_or(false)
        || ["dosy", "diffusion", "bpp", "ste", "led", "oneshot"]
            .iter()
            .any(|k| hint.contains(k));

    let diffusion = if looks_dosy {
        let gamma = gyromagnetic_ratio(&direct.nucleus).unwrap_or(2.675_222_005e8);
        let delta = params.si("delta").unwrap_or(0.0);
        let big_delta = params
            .si("diffusion_time")
            .or_else(|| params.si("delta_large"))
            .unwrap_or(0.0);
        let tau = params.si("tau").unwrap_or(0.0);
        let shape_factor = gradient_shape_factor(
            params
                .string_ci("grad_shape")
                .as_deref()
                .unwrap_or("SQUARE"),
        );
        (delta > 0.0 && big_delta > 0.0).then_some(DiffusionMeta {
            gamma,
            delta,
            big_delta,
            tau,
            shape_factor,
        })
    } else {
        None
    };

    (axis, diffusion)
}

/// Recover non-uniform-sampling metadata from the parameter list. Present only
/// when `sampling` reports a NUS scheme; the acquired increment count is the
/// stored real row count and the nominal grid is inferred from the sampling
/// rate. Recent Delta files also serialize `Y_NUSLIST` as a big-endian integer
/// array near the file tail; use it when its size and bounds agree with the
/// acquisition, otherwise leave the schedule for the user to supply.
fn normalize_nucleus(domain: &str) -> String {
    match domain.trim().to_ascii_lowercase().as_str() {
        "proton" => "1H".into(),
        "carbon13" | "carbon" => "13C".into(),
        "phosphorus31" | "phosphorus" => "31P".into(),
        "fluorine19" | "fluorine" => "19F".into(),
        "nitrogen15" | "nitrogen" => "15N".into(),
        other if !other.is_empty() => domain.trim().to_string(),
        _ => "X".into(),
    }
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

struct Params {
    f64s: HashMap<String, f64>,
    /// SI-normalized numeric values (raw value with its scaler prefix folded in).
    si: HashMap<String, f64>,
    strings: HashMap<String, String>,
}

impl Params {
    // Offsets within one fixed-size record, from the record start.
    const SCALER: usize = 0x06; // u8: SI-prefix nibble (high nibble) on the value
    const VALUE: usize = 0x10; // value payload (f64 uses the first 8 bytes)
    const VALUE_TYPE: usize = 0x20; // u32: 0 = string, 2 = f64
    const NAME: usize = 0x24; // 28-byte name, space/NUL padded
    const NAME_LEN: usize = 28;

    fn empty() -> Self {
        Self {
            f64s: HashMap::new(),
            si: HashMap::new(),
            strings: HashMap::new(),
        }
    }

    // List header at `at` (body endianness): record_size u32, low_index u32,
    // high_index u32, total_size u32; then fixed-size records.
    fn parse(bytes: &[u8], at: usize, endian: Endian) -> Self {
        let r = Reader { bytes, endian };
        if at + 16 > bytes.len() {
            return Self::empty();
        }
        let rec_size = r.u32(at) as usize;
        let high = r.u32(at + 8) as usize;
        if !(Self::NAME + Self::NAME_LEN..=4096).contains(&rec_size) {
            return Self::empty();
        }
        let count = high.saturating_add(1).min(4096);
        let base = at + 16;

        let mut out = Self::empty();
        for i in 0..count {
            let rec = base + i * rec_size;
            if rec + rec_size > bytes.len() {
                break;
            }
            let name = ascii_trim(&bytes[rec + Self::NAME..rec + Self::NAME + Self::NAME_LEN]);
            if name.is_empty() {
                continue;
            }
            match r.u32(rec + Self::VALUE_TYPE) {
                2 => {
                    let raw = r.f64(rec + Self::VALUE);
                    let scaler = bytes[rec + Self::SCALER];
                    let si = raw * 10f64.powi(prefix_exponent(scaler));
                    out.si.insert(name.clone(), si);
                    out.f64s.insert(name, raw);
                }
                0 => {
                    let s = ascii_trim(&bytes[rec + Self::VALUE..rec + Self::VALUE + 16]);
                    if !s.is_empty() {
                        out.strings.insert(name, s);
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn f64(&self, name: &str) -> Option<f64> {
        self.f64s.get(name).copied().filter(|v| v.is_finite())
    }

    fn string(&self, name: &str) -> Option<String> {
        self.strings.get(name).cloned()
    }

    /// SI-normalized value of a numeric parameter, matched case-insensitively so
    /// system params (`X_OFFSET`) and experiment params (`delta`) both resolve.
    fn si(&self, name: &str) -> Option<f64> {
        if let Some(v) = self.si.get(name) {
            return Some(*v).filter(|v| v.is_finite());
        }
        let key = name.to_ascii_lowercase();
        self.si
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == key)
            .map(|(_, v)| *v)
            .filter(|v| v.is_finite())
    }

    fn string_ci(&self, name: &str) -> Option<String> {
        if let Some(s) = self.strings.get(name) {
            return Some(s.clone());
        }
        let key = name.to_ascii_lowercase();
        self.strings
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == key)
            .map(|(_, v)| v.clone())
    }
}

/// Stored sample width from the data-section byte budget, requiring an exact f32/f64
/// fit — a size matching neither is reported, not guessed (else silent garbage).
fn sample_format(budget: usize, total_samples: usize) -> Result<SampleFmt, IoError> {
    if total_samples == 0 {
        return Err(IoError::Unsupported(
            "header reports zero data points".into(),
        ));
    }
    if budget == 8 * total_samples {
        Ok(SampleFmt::F64)
    } else if budget == 4 * total_samples {
        Ok(SampleFmt::F32)
    } else {
        Err(IoError::Unsupported(format!(
            "data section of {budget} bytes fits neither f32 ({}) nor f64 ({}) for {total_samples} samples",
            4 * total_samples,
            8 * total_samples
        )))
    }
}

#[derive(Debug, Clone, Copy)]
enum SampleFmt {
    F32,
    F64,
}

impl SampleFmt {
    #[inline]
    fn size(self) -> usize {
        match self {
            SampleFmt::F32 => 4,
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
    fn u32(&self, at: usize) -> u32 {
        let b: [u8; 4] = self.bytes[at..at + 4].try_into().unwrap();
        match self.endian {
            Endian::Big => u32::from_be_bytes(b),
            Endian::Little => u32::from_le_bytes(b),
        }
    }

    fn u64(&self, at: usize) -> u64 {
        let b: [u8; 8] = self.bytes[at..at + 8].try_into().unwrap();
        match self.endian {
            Endian::Big => u64::from_be_bytes(b),
            Endian::Little => u64::from_le_bytes(b),
        }
    }

    fn f64(&self, at: usize) -> f64 {
        let b: [u8; 8] = self.bytes[at..at + 8].try_into().unwrap();
        match self.endian {
            Endian::Big => f64::from_be_bytes(b),
            Endian::Little => f64::from_le_bytes(b),
        }
    }

    fn f32(&self, at: usize) -> f32 {
        let b: [u8; 4] = self.bytes[at..at + 4].try_into().unwrap();
        match self.endian {
            Endian::Big => f32::from_be_bytes(b),
            Endian::Little => f32::from_le_bytes(b),
        }
    }

    #[inline]
    fn sample(&self, at: usize, fmt: SampleFmt) -> f64 {
        match fmt {
            SampleFmt::F32 => self.f32(at) as f64,
            SampleFmt::F64 => self.f64(at),
        }
    }

    fn read_reals(&self, at: usize, n: usize, fmt: SampleFmt) -> Vec<f64> {
        (0..n)
            .map(|i| match fmt {
                SampleFmt::F32 => self.f32(at + i * 4) as f64,
                SampleFmt::F64 => self.f64(at + i * 8),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
