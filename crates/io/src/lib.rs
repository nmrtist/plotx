//! Data I/O: spectral format parsers producing the neutral [`NmrData`] container.

pub mod abf2;
pub mod archive;
pub mod bruker;
pub mod delimited;
pub mod jcamp_dx;
pub mod jeol;
pub mod xlsx;

use num_complex::Complex64;
use std::path::{Path, PathBuf};

/// A format identified before parsing. Detection and loading are deliberately
/// separate so GUI, CLI, and archive workflows share one dispatch contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Abf2,
    JeolDelta,
    BrukerRaw,
    BrukerProcessed1D,
    BrukerProcessed2D,
    JcampDx1D,
}

impl DataFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Abf2 => "abf2",
            Self::JeolDelta => "jeol-delta",
            Self::BrukerRaw => "bruker-raw",
            Self::BrukerProcessed1D => "bruker-processed-1d",
            Self::BrukerProcessed2D => "bruker-processed-2d",
            Self::JcampDx1D => "jcamp-dx-1d",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Path selected by the caller.
    pub selected_path: PathBuf,
    /// Binary payload actually parsed after directory resolution.
    pub data_path: PathBuf,
    /// Parameter files that define interpretation of the payload.
    pub parameter_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadWarningCode {
    ArchiveEntryFailed,
    OptionalImaginaryMissing,
    MissingStimulus,
    InvalidMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadWarning {
    pub code: LoadWarningCode,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoadResult {
    pub acquisition: Acquisition,
    pub format: DataFormat,
    pub provenance: Provenance,
    pub warnings: Vec<LoadWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Time,
    Frequency,
}

/// Neutral, format-independent container for a single 1D acquisition.
#[derive(Debug, Clone)]
pub struct NmrData {
    pub points: Vec<Complex64>,
    pub domain: Domain,
    pub spectral_width_hz: f64,
    pub observe_freq_mhz: f64,
    pub carrier_ppm: f64,
    pub nucleus: String,
    pub source: String,
    /// Digital-filter group delay in points, removed by the FFT stage as a
    /// first-order phase ramp. Nonzero for Bruker; 0.0 when absent.
    pub group_delay: f64,
}

impl NmrData {
    #[inline]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Dwell time in seconds (1 / spectral width).
    #[inline]
    pub fn dwell_s(&self) -> f64 {
        if self.spectral_width_hz != 0.0 {
            1.0 / self.spectral_width_hz
        } else {
            0.0
        }
    }
}

/// Per-axis acquisition parameters for one dimension of an nD dataset.
#[derive(Debug, Clone)]
pub struct Dim {
    pub spectral_width_hz: f64,
    pub observe_freq_mhz: f64,
    pub carrier_ppm: f64,
    pub nucleus: String,
    /// Digital-filter group delay in points. Meaningful only for the direct
    /// (F2) dimension; 0.0 for the indirect (F1) dimension.
    pub group_delay: f64,
}

impl Dim {
    #[inline]
    pub fn dwell_s(&self) -> f64 {
        if self.spectral_width_hz != 0.0 {
            1.0 / self.spectral_width_hz
        } else {
            0.0
        }
    }
}

/// Quadrature-detection scheme of the indirect (F1) dimension, which fixes how
/// the stored rows recombine into a complex t1 interferogram before its FFT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadMode {
    /// One complex row per t1 increment (phase-modulated); FFT the column as-is.
    Complex,
    /// Cosine/sine pair per increment (States / hypercomplex).
    States,
    /// States with alternate increments negated (States-TPPI).
    StatesTppi,
    /// Echo/anti-echo pair per increment (Rance–Kay).
    EchoAntiecho,
}

/// What the indirect axis of a pseudo-2D array physically varies. Fixes the
/// fitting model and the label of a DOSY/relaxation figure's second axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoKind {
    /// Pulsed-field-gradient amplitude (DOSY); values in T/m.
    Gradient,
    /// A time delay (T1/T2 relaxation array); values in seconds.
    Delay,
    /// An arrayed parameter we could not classify; values as stored.
    Generic,
}

/// Where a [`PseudoAxis`]'s values came from, surfaced so the UI can flag
/// reconstructed or hand-entered rulers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisSource {
    /// Explicit `{v1, v2, …}` list embedded in the experiment text (exact).
    EmbeddedList,
    /// `start..stop : step` ramp descriptor embedded in the experiment text.
    EmbeddedRamp,
    /// Reconstructed from header start/stop/points (assumes a linear ruler).
    LinearHeader,
    /// Entered or edited by the user.
    Manual,
}

/// The indirect-axis ruler of a pseudo-2D array: one physical value per stored
/// row (gradient strength, relaxation delay, …), in SI units, alongside the
/// display unit and provenance.
#[derive(Debug, Clone)]
pub struct PseudoAxis {
    pub name: String,
    pub kind: PseudoKind,
    /// One SI value per row (T/m for gradients, s for delays).
    pub values: Vec<f64>,
    /// Display unit the values were read in, e.g. "mT/m" or "ms".
    pub unit: String,
    pub source: AxisSource,
}

/// Diffusion-encoding parameters needed to turn a gradient ruler into a
/// Stejskal–Tanner b-factor. All times in seconds, `gamma` in rad·s⁻¹·T⁻¹.
#[derive(Debug, Clone, Copy)]
pub struct DiffusionMeta {
    pub gamma: f64,
    /// Encoding gradient pulse width δ.
    pub delta: f64,
    /// Diffusion delay Δ (JEOL `diffusion_time` / `delta_large`).
    pub big_delta: f64,
    /// Bipolar-pair recovery delay τ (0 for monopolar).
    pub tau: f64,
    /// Effective-delay coefficient on δ from the gradient shape (SQUARE = 1/3).
    pub shape_factor: f64,
}

impl DiffusionMeta {
    /// Effective diffusion time Δ − shape_factor·δ − τ/2.
    #[inline]
    pub fn effective_delay(&self) -> f64 {
        self.big_delta - self.shape_factor * self.delta - 0.5 * self.tau
    }

    /// Stejskal–Tanner b-factor at gradient strength `g` (T/m): the coefficient
    /// such that I(g) = I0·exp(−D·b). Units s·m⁻².
    #[inline]
    pub fn b_factor(&self, g: f64) -> f64 {
        let x = self.gamma * self.delta * g;
        x * x * self.effective_delay()
    }
}

/// Gyromagnetic ratio in rad·s⁻¹·T⁻¹ for a nucleus label ("1H", "19F", …).
pub fn gyromagnetic_ratio(nucleus: &str) -> Option<f64> {
    let key = nucleus.trim().to_ascii_uppercase();
    let g = match key.as_str() {
        "1H" | "H1" | "PROTON" => 2.675_222_005e8,
        "2H" | "H2" | "DEUTERIUM" => 4.106_627_9e7,
        "13C" | "C13" | "CARBON13" => 6.728_284e7,
        "15N" | "N15" | "NITROGEN15" => -2.712_618e7,
        "19F" | "F19" | "FLUORINE19" => 2.518_148e8,
        "31P" | "P31" | "PHOSPHORUS31" => 1.083_941e8,
        "7LI" | "LI7" => 1.039_764e8,
        "11B" | "B11" => 8.584_708e7,
        "23NA" | "NA23" => 7.080_493e7,
        _ => return None,
    };
    Some(g)
}

/// Gradient-shape δ-coefficient for the effective diffusion time, matching the
/// JEOL `bpp_ste_diffusion` definitions. Defaults to the SQUARE value.
pub fn gradient_shape_factor(shape: &str) -> f64 {
    match shape.trim().to_ascii_uppercase().as_str() {
        "SINE" => 0.3125,
        "SQUARE_SINE" => 0.30167,
        "TRAPEZOID" => 0.32545,
        "S_RECTANGLE" => 0.32526,
        _ => 1.0 / 3.0,
    }
}

/// Non-uniform sampling (NUS) metadata for the indirect axis. Present when the
/// acquisition sampled only a subset of the nominal F1 grid; the missing
/// increments must be reconstructed before the F1 FFT. Readers recover the
/// sampling schedule when the source format stores it; otherwise `schedule`
/// stays `None` until the user supplies the list.
#[derive(Debug, Clone)]
pub struct NusMeta {
    /// Nominal full grid size N (complex increments) the schedule indexes into.
    pub grid: usize,
    /// Acquired complex increment count M (the stored, sampled rows).
    pub acquired: usize,
    /// Index base of a sampling list (JEOL `nuslist_idx_base`, normally 1).
    pub idx_base: usize,
    /// Scheduling mode label (`poisson gap`, …), surfaced for the user.
    pub mode: String,
    /// True for echo/anti-echo (P/N) coherence selection (`pn_type = "y"`): the
    /// two stored F1 channels are P and N and need a `pn_to_shr` conversion
    /// before the States-style hypercomplex assembly.
    pub echo_antiecho: bool,
    /// Sampling schedule from the source file or user: one nominal-grid index
    /// per acquired increment, stored 0-based (`idx_base` already subtracted).
    pub schedule: Option<Vec<usize>>,
}

/// Neutral, format-independent container for a single 2D acquisition. `data` is
/// a row-major matrix of `rows` (indirect / F1) rows, each a complex FID of
/// `cols` (direct / F2) points.
#[derive(Debug, Clone)]
pub struct NmrData2D {
    pub data: Vec<Complex64>,
    pub rows: usize,
    pub cols: usize,
    pub domain: Domain,
    pub direct: Dim,
    pub indirect: Dim,
    pub quad: QuadMode,
    /// When true, the indirect (F1) modulation is conjugated relative to a
    /// forward-FFT convention, so the F1 stage conjugates the t1 vector to get
    /// the frequency sense right. True for JEOL (its FID conjugation on read
    /// flips F1); false for Bruker.
    pub indirect_conjugate: bool,
    /// Free-text experiment hint (Bruker `PULPROG`, JEOL experiment name, or the
    /// file name) used to recommend a default processing layout. Lower-cased.
    pub experiment: Option<String>,
    /// Indirect-axis ruler for a pseudo-2D array (DOSY gradients, relaxation
    /// delays). `None` for true-2D experiments or when no ruler was recovered.
    pub pseudo_axis: Option<PseudoAxis>,
    /// Diffusion-encoding parameters, populated for DOSY acquisitions.
    pub diffusion: Option<DiffusionMeta>,
    /// Non-uniform sampling metadata; `None` for uniformly sampled acquisitions.
    pub nus: Option<NusMeta>,
    pub source: String,
}

impl NmrData2D {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[inline]
    pub fn row(&self, r: usize) -> &[Complex64] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }
}

/// A loaded acquisition: 1D or 2D. Higher layers dispatch on the dimensionality.
#[derive(Debug, Clone)]
pub enum Acquisition {
    D1(NmrData),
    D2(Box<NmrData2D>),
    Electrophysiology(Box<ElectrophysiologyData>),
}

/// Physical quantity represented by an electrophysiology channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ElectricalQuantity {
    Voltage,
    Current,
    Unknown,
}

/// A display unit retained exactly enough to preserve the instrument scale.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ElectricalUnit {
    pub symbol: String,
    pub quantity: ElectricalQuantity,
}

impl ElectricalUnit {
    pub fn from_symbol(symbol: impl Into<String>) -> Self {
        let symbol = symbol.into();
        let quantity = match symbol.trim().to_ascii_lowercase().as_str() {
            "v" | "mv" | "uv" | "kv" => ElectricalQuantity::Voltage,
            "a" | "ma" | "ua" | "na" | "pa" => ElectricalQuantity::Current,
            _ => ElectricalQuantity::Unknown,
        };
        Self { symbol, quantity }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordedChannel {
    pub name: String,
    pub unit: ElectricalUnit,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandWaveform {
    pub name: String,
    pub unit: ElectricalUnit,
    pub holding_level: f64,
    pub samples: Vec<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Sweep {
    pub start_time_s: f64,
    /// One sample vector per [`RecordedChannel`], in channel order.
    pub channels: Vec<Vec<f64>>,
    pub commands: Vec<CommandWaveform>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ElectrophysiologyData {
    pub abf_version: String,
    pub sample_rate_hz: f64,
    pub channels: Vec<RecordedChannel>,
    pub sweeps: Vec<Sweep>,
    pub protocol: Option<String>,
    pub source: String,
    pub import_warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("archive error: {0}")]
    Archive(String),

    #[error("not a JEOL Delta file: bad magic (expected \"JEOL.NMR\")")]
    BadMagic,

    #[error("file is truncated: needed {needed} bytes at offset {offset}, have {have}")]
    Truncated {
        offset: usize,
        needed: usize,
        have: usize,
    },

    #[error("unsupported JEOL feature: {0}")]
    Unsupported(String),

    #[error("invalid ABF2 file: {0}")]
    InvalidAbf2(String),

    #[error(transparent)]
    JcampDx(#[from] jcamp_dx::JcampDxError),
}

/// Load a dataset, auto-detecting the format from the path. A Bruker
/// acquisition is recognised whether given as its directory or as the `fid`/
/// `ser` file inside it; other files dispatch by extension, then by content.
pub fn detect_format(path: impl AsRef<Path>) -> Result<DataFormat, IoError> {
    let path = path.as_ref();
    if let Some(format) = bruker::detect_processed(path) {
        return Ok(format);
    }
    if bruker::is_bruker(path) {
        return Ok(DataFormat::BrukerRaw);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "abf" if abf2::is_abf2(path) => Ok(DataFormat::Abf2),
        "jdf" => Ok(DataFormat::JeolDelta),
        "dx" | "jdx" | "jcamp" => Ok(DataFormat::JcampDx1D),
        // Fall back to a content sniff so extensionless or mislabelled files
        // are still recognised by their magic bytes.
        _ if abf2::is_abf2(path) => Ok(DataFormat::Abf2),
        _ if jeol::is_jdf(path) => Ok(DataFormat::JeolDelta),
        _ => Err(IoError::Unsupported(format!(
            "unrecognised path {}: expected ABF2 .abf, JEOL .jdf, JCAMP-DX .dx/.jdx/.jcamp, Bruker fid/ser, or Bruker pdata",
            path.display()
        ))),
    }
}

pub fn load_path(path: impl AsRef<Path>) -> Result<LoadResult, IoError> {
    let path = path.as_ref();
    match detect_format(path)? {
        DataFormat::Abf2 => abf2::load(path),
        DataFormat::JeolDelta => Ok(LoadResult {
            acquisition: jeol::read_jdf_path(path)?,
            format: DataFormat::JeolDelta,
            provenance: Provenance {
                selected_path: path.to_path_buf(),
                data_path: path.to_path_buf(),
                parameter_paths: Vec::new(),
            },
            warnings: Vec::new(),
        }),
        DataFormat::BrukerRaw => bruker::load_raw(path),
        DataFormat::BrukerProcessed1D | DataFormat::BrukerProcessed2D => {
            bruker::load_processed(path)
        }
        DataFormat::JcampDx1D => jcamp_dx::load(path),
    }
}
