use crate::{Color, ColormapId};
use serde::{Deserialize, Deserializer, Serialize, de};

/// Default authored width, in points, for data traces and contour strokes.
pub const DEFAULT_DATA_LINE_WIDTH_PT: f32 = 0.5;

/// A finite, strictly positive scalar used by persisted presentation settings.
/// Constructors reject non-finite values so encodings cannot poison renderer keys.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PositiveFiniteF64(f64);

impl PositiveFiniteF64 {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

/// A finite signed scalar used by persisted presentation settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| de::Error::custom("expected a finite value"))
    }
}

impl<'de> Deserialize<'de> for PositiveFiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value)
            .ok_or_else(|| de::Error::custom("expected a finite value greater than zero"))
    }
}

/// A finite, strictly positive width in output-space logical units.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PositiveFiniteF32(f32);

impl PositiveFiniteF32 {
    pub fn new(value: f32) -> Option<Self> {
        (value.is_finite() && value > 0.0).then_some(Self(value))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PositiveFiniteF32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::new(value)
            .ok_or_else(|| de::Error::custom("expected a finite value greater than zero"))
    }
}

/// A finite fraction in the closed unit interval.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct UnitInterval(f64);

impl UnitInterval {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(Self(value))
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for UnitInterval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| de::Error::custom("expected a finite value in [0, 1]"))
    }
}

/// A concrete display color. Theme application rewrites these values as one
/// undoable style operation; document encodings never carry unresolved tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSource {
    Explicit(Color),
}

impl ColorSource {
    pub const fn resolve(self) -> Color {
        match self {
            Self::Explicit(color) => color,
        }
    }
}

/// An estimator identity selected by an encoding. The concrete estimate and its
/// provenance are deliberately owned by the field provider rather than here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum EstimatorSelection {
    FollowLatest,
    Frozen { estimator: String, version: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineEncoding {
    pub color: ColorSource,
    pub scale: f64,
    pub width: PositiveFiniteF32,
    /// Plot-owned translation in the x-axis coordinate system.
    pub x_shift: FiniteF64,
}

impl Default for LineEncoding {
    fn default() -> Self {
        Self {
            color: ColorSource::Explicit(Color::TRACE),
            scale: 1.0,
            width: PositiveFiniteF32::new(DEFAULT_DATA_LINE_WIDTH_PT)
                .expect("literal width is valid"),
            x_shift: FiniteF64::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourSpec {
    pub positive: ContourLevelSpec,
    pub negative: Option<ContourLevelSpec>,
    pub style: ContourStyle,
}

impl ContourSpec {
    pub fn absolute(base: f64, negative: bool) -> Option<Self> {
        let level = ContourLevelSpec {
            base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(base)?),
            count: 14,
            ratio: PositiveFiniteF64::new(1.35)?,
        };
        Some(Self {
            positive: level.clone(),
            negative: negative.then_some(level),
            style: ContourStyle::default(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContourLevelSpec {
    pub base: ContourBasePolicy,
    pub count: u16,
    pub ratio: PositiveFiniteF64,
}

impl ContourLevelSpec {
    pub const MAX_COUNT: u16 = 256;

    fn validate(&self) -> bool {
        self.count > 0 && self.count <= Self::MAX_COUNT && self.ratio.get() > 1.0
    }
}

impl<'de> Deserialize<'de> for ContourLevelSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawContourLevelSpec {
            base: ContourBasePolicy,
            count: u16,
            ratio: PositiveFiniteF64,
        }

        let raw = RawContourLevelSpec::deserialize(deserializer)?;
        let level = Self {
            base: raw.base,
            count: raw.count,
            ratio: raw.ratio,
        };
        level.validate().then_some(level).ok_or_else(|| {
            de::Error::custom("contour count must be in 1..=256 and ratio must exceed one")
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "policy", content = "value")]
pub enum ContourBasePolicy {
    Absolute(PositiveFiniteF64),
    /// A multiple of the field's noise scale, where "noise scale" is the larger
    /// of the estimator's answer and `peak_fraction` of the field's peak
    /// magnitude.
    ///
    /// Both coefficients are carried here rather than one being applied inside
    /// resolution, because which of the two is in force decides what the number
    /// beside the control means. A policy that silently substituted a peak
    /// fraction while the interface still read `5 × σ` would be describing a
    /// picture the data never produced.
    ///
    /// The floor is not a preference: an estimator measures thermal noise, and
    /// a field with enough dynamic range carries sampling artefacts of its own
    /// strongest feature far above that. Below `peak_fraction` of the peak a
    /// contour traces those artefacts rather than signal. A field whose
    /// estimated scale is large next to its peak never reaches the floor and
    /// resolves exactly as a plain multiple of σ would.
    NoiseFloor {
        multiplier: PositiveFiniteF64,
        /// The smallest noise scale this anchor will accept, as a fraction of
        /// the field's peak magnitude. Zero disables the floor.
        peak_fraction: UnitInterval,
        estimator: EstimatorSelection,
    },
    BackgroundScale {
        multiplier: PositiveFiniteF64,
        estimator: EstimatorSelection,
    },
    FractionOfRange(UnitInterval),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContourStyle {
    pub positive_color: ColorSource,
    pub negative_color: ColorSource,
    pub width: PositiveFiniteF32,
}

impl Default for ContourStyle {
    fn default() -> Self {
        Self {
            positive_color: ColorSource::Explicit(Color::TRACE),
            negative_color: ColorSource::Explicit(Color::rgb(0xd1, 0x24, 0x2a)),
            width: PositiveFiniteF32::new(DEFAULT_DATA_LINE_WIDTH_PT)
                .expect("literal width is valid"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeatmapSpec {
    pub colormap: ColormapId,
    /// `None` uses the scalar field's finite min/max summary.
    pub value_range: Option<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageSpec {
    pub opacity: UnitInterval,
    pub interpolation: ImageInterpolation,
}

impl Default for ImageSpec {
    fn default() -> Self {
        Self {
            opacity: UnitInterval::new(1.0).expect("literal opacity is valid"),
            interpolation: ImageInterpolation::Linear,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageInterpolation {
    Nearest,
    #[default]
    Linear,
}

/// The concrete, persisted visual encoding of one series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "spec")]
pub enum SeriesEncoding {
    Line(LineEncoding),
    Contour(ContourSpec),
    Heatmap(HeatmapSpec),
    Image(ImageSpec),
}

impl Default for SeriesEncoding {
    fn default() -> Self {
        Self::Line(LineEncoding::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contour_style_keeps_positive_and_negative_colors_distinct() {
        let style = ContourStyle::default();
        assert_ne!(
            style.positive_color.resolve(),
            style.negative_color.resolve()
        );
    }

    #[test]
    fn line_and_contour_defaults_share_the_fine_data_stroke() {
        assert_eq!(
            LineEncoding::default().width.get(),
            DEFAULT_DATA_LINE_WIDTH_PT
        );
        assert_eq!(
            ContourStyle::default().width.get(),
            DEFAULT_DATA_LINE_WIDTH_PT
        );
    }

    #[test]
    fn persisted_encoding_has_no_auto_variant() {
        let value = serde_json::to_value(SeriesEncoding::default()).unwrap();
        assert_ne!(value["kind"], "auto");
    }

    #[test]
    fn persisted_numeric_settings_reject_invalid_values() {
        assert!(serde_json::from_str::<PositiveFiniteF64>("-1.0").is_err());
        assert!(serde_json::from_str::<PositiveFiniteF32>("0.0").is_err());
        assert!(serde_json::from_str::<UnitInterval>("1.1").is_err());
        assert!(
            serde_json::from_str::<ContourLevelSpec>(
                r#"{"base":{"policy":"absolute","value":1.0},"count":0,"ratio":1.35}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ContourLevelSpec>(
                r#"{"base":{"policy":"absolute","value":1.0},"count":3,"ratio":1.0}"#,
            )
            .is_err()
        );
        let error = serde_json::from_str::<ContourLevelSpec>(
            r#"{"base":{"policy":"absolute","value":1.0},"count":257,"ratio":1.35}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("1..=256"));
        assert!(
            serde_json::from_str::<ContourLevelSpec>(
                r#"{"base":{"policy":"absolute","value":1.0},"count":3,"ratio":1.35}"#,
            )
            .is_ok()
        );
    }

    #[test]
    fn strict_line_encoding_requires_one_finite_x_shift() {
        let value = serde_json::to_value(LineEncoding::default()).unwrap();
        assert_eq!(value["x_shift"], 0.0);

        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("x_shift");
        assert!(serde_json::from_value::<LineEncoding>(missing).is_err());

        let mut unknown = value.clone();
        unknown["legacy_shift"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<LineEncoding>(unknown).is_err());

        assert!(serde_json::from_str::<FiniteF64>("1e999").is_err());
    }
}
