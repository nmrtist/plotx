use super::*;

pub fn default_contour_spec(
    capabilities: &FieldCapabilities,
    peak: PeakMagnitude<'_>,
) -> ContourSpec {
    let base = contour_base_policy(default_contour_base_kind(capabilities), peak)
        .expect("default base kind is a known policy");
    let level = ContourLevelSpec {
        base,
        count: 14,
        ratio: PositiveFiniteF64::new(1.35).expect("literal ratio is valid"),
    };
    ContourSpec {
        positive: level.clone(),
        negative: capabilities.contains(CAP_FIELD_SIGNED).then_some(level),
        style: ContourStyle {
            positive_color: ColorSource::Explicit(plotx_figure::Color::TRACE),
            negative_color: ColorSource::Explicit(plotx_figure::Color::rgb(0xd1, 0x24, 0x2a)),
            ..ContourStyle::default()
        },
    }
}
