use super::*;

#[test]
fn missing_spacing_mode_defaults_to_visual_and_writes_explicitly() {
    let dto: PageLayoutDto = serde_json::from_str(
        r#"{"margin_mm":[0.0,0.0,0.0,0.0],"gutter_mm":5.0,"rows":1,"cols":2}"#,
    )
    .unwrap();
    assert_eq!(
        dto.into_layout().spacing_mode,
        crate::layout::SpacingMode::Visual
    );
    let encoded =
        serde_json::to_string(&PageLayoutDto::from_layout(&PageLayout::default())).unwrap();
    assert!(encoded.contains("\"spacing_mode\":\"visual\""));
}
