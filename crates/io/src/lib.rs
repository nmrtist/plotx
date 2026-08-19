//! Data I/O: spectral format parsers producing the neutral [`NmrData`] container.

pub mod abf2;
pub mod archive;
pub mod bruker;
pub mod delimited;
pub mod jcamp_dx;
pub mod jeol;
mod mass_spec;
pub mod mzml;
pub mod nanoscope;
pub mod origin;
pub mod varian;
pub mod waters;
pub mod xlsx;
pub mod xps;
pub mod xrd;

pub use mass_spec::*;

use num_complex::Complex64;
use std::path::{Path, PathBuf};

/// A format identified before parsing. Detection and loading are deliberately
/// separate so GUI, CLI, and archive workflows share one dispatch contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Abf2,
    JeolDelta,
    BrukerRaw,
    VarianAgilentRaw,
    BrukerProcessed1D,
    BrukerProcessed2D,
    JcampDx1D,
    BrukerNanoScopeSpm,
    BrukerPeakForceCapture,
    WatersMassLynxRaw,
    MzMl,
    RigakuRasx,
    RigakuRaw,
    RigakuProfile,
    VamasXps,
    CasaXpsText,
}

impl DataFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Abf2 => "abf2",
            Self::JeolDelta => "jeol-delta",
            Self::BrukerRaw => "bruker-raw",
            Self::VarianAgilentRaw => "varian-agilent-raw",
            Self::BrukerProcessed1D => "bruker-processed-1d",
            Self::BrukerProcessed2D => "bruker-processed-2d",
            Self::JcampDx1D => "jcamp-dx-1d",
            Self::BrukerNanoScopeSpm => "bruker-nanoscope-spm",
            Self::BrukerPeakForceCapture => "bruker-peakforce-capture",
            Self::WatersMassLynxRaw => "waters-masslynx-raw",
            Self::MzMl => "mzml",
            Self::RigakuRasx => "rigaku-rasx",
            Self::RigakuRaw => "rigaku-raw-fi",
            Self::RigakuProfile => "rigaku-profile",
            Self::VamasXps => "vamas-xps",
            Self::CasaXpsText => "casaxps-text",
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
    /// Related payloads merged into this logical dataset.
    pub companion_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadWarningCode {
    ArchiveEntryFailed,
    OptionalImaginaryMissing,
    MissingStimulus,
    InvalidMetadata,
    MissingCalibration,
    MissingCompanion,
    CompanionMismatch,
    OptionalChannelSkipped,
    UnsupportedFunction,
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
    /// Normalized, user-facing identity recovered by the importer. This is
    /// deliberately separate from provenance paths and parser diagnostics: the
    /// application must never reverse-parse `source` strings to name a sample.
    pub scientific_identity: ImportedScientificIdentity,
    pub format: DataFormat,
    pub provenance: Provenance,
    pub warnings: Vec<LoadWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportedScientificIdentity {
    /// The specimen, recording, run, or other scientific subject.
    pub subject: Option<String>,
    /// The acquisition experiment, protocol, or method when the format names it.
    pub acquisition: Option<String>,
    /// A clean logical source name used only when no subject was recovered.
    pub source_label: String,
}

impl ImportedScientificIdentity {
    pub fn from_path(path: &Path) -> Self {
        let source_label = path
            .file_stem()
            .or_else(|| path.file_name())
            .and_then(|value| value.to_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Untitled data")
            .to_owned();
        Self {
            subject: None,
            acquisition: None,
            source_label,
        }
    }
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
    Afm(Box<AfmData>),
    MassSpec(Box<MassSpecRun>),
    Xrd(Box<XrdData>),
    Xps(Box<xps::XpsExperiment>),
}

/// A one-dimensional powder X-ray diffraction pattern.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct XrdData {
    /// Diffraction angle 2theta in degrees, strictly increasing.
    pub two_theta_deg: Vec<f64>,
    /// Observed intensity in counts per second when the source declares it.
    pub intensity: Vec<f64>,
    /// Per-point attenuation multiplier retained from Rigaku profiles.
    pub attenuation: Option<Vec<f64>>,
    pub source: String,
    pub instrument: Option<String>,
    pub target: Option<String>,
    pub wavelength_angstrom: Option<f64>,
    pub voltage_kv: Option<f64>,
    pub current_ma: Option<f64>,
    pub scan_step_deg: Option<f64>,
    pub scan_speed_deg_min: Option<f64>,
}

impl XrdData {
    pub fn len(&self) -> usize {
        self.two_theta_deg.len()
    }

    pub fn is_empty(&self) -> bool {
        self.two_theta_deg.is_empty()
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.len() < 2 || self.len() != self.intensity.len() {
            return Err("XRD payload has inconsistent axes");
        }
        if self
            .two_theta_deg
            .iter()
            .zip(&self.intensity)
            .any(|(&angle, &intensity)| {
                !angle.is_finite() || !intensity.is_finite() || intensity < 0.0
            })
        {
            return Err("XRD payload contains invalid numeric values");
        }
        if self.two_theta_deg.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("XRD 2theta values must increase strictly");
        }
        if self.attenuation.as_ref().is_some_and(|values| {
            values.len() != self.len()
                || values
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
        }) {
            return Err("XRD attenuation values are invalid");
        }
        if [
            self.wavelength_angstrom,
            self.voltage_kv,
            self.current_ma,
            self.scan_step_deg,
            self.scan_speed_deg_min,
        ]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value <= 0.0)
        {
            return Err("XRD acquisition metadata contains an invalid numeric value");
        }
        Ok(())
    }
}

/// Linear calibration applied lazily to an AFM integer signal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AfmScale {
    pub multiplier: f64,
    pub offset: f64,
    pub unit: String,
}

impl AfmScale {
    pub fn apply(&self, raw: i32) -> f64 {
        self.offset + self.multiplier * f64::from(raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AfmFrameDirection {
    Trace,
    Retrace,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AfmImageChannel {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub scan_size_x: f64,
    pub scan_size_y: f64,
    pub lateral_unit: String,
    pub scale: AfmScale,
    /// Row-major, normalized left-to-right and bottom-to-top.
    pub raw: std::sync::Arc<[i32]>,
    pub frame_direction: AfmFrameDirection,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AfmForceSet {
    pub grid_width: usize,
    pub grid_height: usize,
    pub samples_per_curve: usize,
    /// Pixel-major curves in normalized image order.
    pub raw: std::sync::Arc<[i32]>,
    pub signal_scale: AfmScale,
    pub sample_period_s: Option<f64>,
    pub z_positions: Option<std::sync::Arc<[f64]>>,
    /// Sample indices in display order; approach precedes retract.
    pub display_order: std::sync::Arc<[usize]>,
    pub approach_samples: usize,
    pub deflection_sensitivity_m_per_v: Option<f64>,
    pub spring_constant_n_per_m: Option<f64>,
}

impl AfmForceSet {
    pub fn curve_raw(&self, x: usize, y: usize) -> Option<&[i32]> {
        if x >= self.grid_width || y >= self.grid_height {
            return None;
        }
        let pixel = y.checked_mul(self.grid_width)?.checked_add(x)?;
        let start = pixel.checked_mul(self.samples_per_curve)?;
        self.raw
            .get(start..start.checked_add(self.samples_per_curve)?)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AfmData {
    pub images: Vec<AfmImageChannel>,
    pub forces: Option<AfmForceSet>,
    pub source: String,
    pub import_warnings: Vec<String>,
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

    #[error("invalid NanoScope file: {0}")]
    InvalidNanoScope(String),

    #[error("invalid Waters MassLynx RAW bundle: {0}")]
    InvalidWatersRaw(String),

    #[error("invalid XRD data: {0}")]
    InvalidXrd(String),

    #[error(
        "unsupported Waters encoding for function {native_function}: IDX stride {idx_stride}, pair width {pair_width}; instrument {instrument}"
    )]
    UnsupportedWatersEncoding {
        native_function: u64,
        idx_stride: usize,
        pair_width: usize,
        instrument: String,
    },

    #[error("invalid or unsupported mzML: {0}")]
    InvalidMzMl(String),

    #[error("invalid XPS data: {0}")]
    InvalidXps(String),

    #[error("invalid Varian/Agilent VnmrJ data: {0}")]
    InvalidVarian(String),

    #[error("unsupported Varian/Agilent VnmrJ data: {0}")]
    UnsupportedVarian(String),
}

/// Load a dataset, auto-detecting the format from the path. A Bruker
/// acquisition is recognised whether given as its directory or as the `fid`/
/// `ser` file inside it; other files dispatch by extension, then by content.
pub fn detect_format(path: impl AsRef<Path>) -> Result<DataFormat, IoError> {
    let path = path.as_ref();
    if xps::is_vamas_xps(path) {
        return Ok(DataFormat::VamasXps);
    }
    if xps::is_casaxps_text(path) {
        return Ok(DataFormat::CasaXpsText);
    }
    if waters::is_masslynx_raw(path) {
        return Ok(DataFormat::WatersMassLynxRaw);
    }
    if let Some(format) = bruker::detect_processed(path) {
        return Ok(format);
    }
    if bruker::is_bruker(path) {
        return Ok(DataFormat::BrukerRaw);
    }
    if varian::is_varian(path) {
        return Ok(DataFormat::VarianAgilentRaw);
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rasx" => Ok(DataFormat::RigakuRasx),
        "raw" if xrd::is_rigaku_raw(path) => Ok(DataFormat::RigakuRaw),
        "txt" if xrd::is_rigaku_profile(path) => Ok(DataFormat::RigakuProfile),
        "raw" if path.is_file() => Err(IoError::Unsupported(format!(
            "unsupported XRD .raw variant; this build recognizes the Rigaku FI layout, .rasx, and exported profile .txt ({})",
            path.display()
        ))),
        "spm" if nanoscope::is_nanoscope(path) => Ok(DataFormat::BrukerNanoScopeSpm),
        "pfc" if nanoscope::is_nanoscope(path) => Ok(DataFormat::BrukerPeakForceCapture),
        "abf" if abf2::is_abf2(path) => Ok(DataFormat::Abf2),
        "jdf" => Ok(DataFormat::JeolDelta),
        "dx" | "jdx" | "jcamp" => Ok(DataFormat::JcampDx1D),
        "mzml" => Ok(DataFormat::MzMl),
        // Fall back to a content sniff so extensionless or mislabelled files
        // are still recognised by their magic bytes.
        _ if abf2::is_abf2(path) => Ok(DataFormat::Abf2),
        _ if jeol::is_jdf(path) => Ok(DataFormat::JeolDelta),
        _ => Err(IoError::Unsupported(format!(
            "unrecognised path {}: expected mzML, Rigaku FI .raw/.rasx/profile .txt, a Waters .raw directory, NanoScope .spm/.pfc, ABF2 .abf, JEOL .jdf, JCAMP-DX .dx/.jdx/.jcamp, Bruker fid/ser or pdata, or a Varian/Agilent VnmrJ .fid directory",
            path.display()
        ))),
    }
}

pub fn load_path(path: impl AsRef<Path>) -> Result<LoadResult, IoError> {
    let path = path.as_ref();
    match detect_format(path)? {
        DataFormat::Abf2 => abf2::load(path),
        DataFormat::JeolDelta => jeol::load_jdf_path(path),
        DataFormat::BrukerRaw => bruker::load_raw(path),
        DataFormat::VarianAgilentRaw => varian::load_raw(path),
        DataFormat::BrukerProcessed1D | DataFormat::BrukerProcessed2D => {
            bruker::load_processed(path)
        }
        DataFormat::JcampDx1D => jcamp_dx::load(path),
        DataFormat::BrukerNanoScopeSpm | DataFormat::BrukerPeakForceCapture => {
            nanoscope::load(path)
        }
        DataFormat::WatersMassLynxRaw => waters::load(path),
        DataFormat::MzMl => mzml::load(path),
        DataFormat::RigakuRasx => xrd::load_rasx(path),
        DataFormat::RigakuRaw => xrd::load_raw(path),
        DataFormat::RigakuProfile => xrd::load_profile(path),
        DataFormat::VamasXps => xps::load_vamas(path),
        DataFormat::CasaXpsText => xps::load_casaxps(path),
    }
}
pub mod image;
