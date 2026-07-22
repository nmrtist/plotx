use super::*;

#[test]
fn dialog_state_passes_trim_to_settings() {
    let mut dialog = ExportDialogState::new(ExportFormat::Svg);
    assert!(!dialog.trim_to_visible_content);
    dialog.trim_to_visible_content = true;
    assert!(ExportSettings::from(&dialog).trim_to_visible_content);
}

#[test]
fn dialog_initializes_sticky_trim_and_existing_dpi_from_defaults() {
    let defaults = crate::settings::ExportDefaults {
        dpi: 450,
        trim_to_visible_content: true,
        ..Default::default()
    };
    let dialog = ExportDialogState::from_defaults(ExportFormat::Png, &defaults);
    assert_eq!(dialog.dpi, 450);
    assert!(dialog.trim_to_visible_content);
}

#[test]
fn resolves_page_scopes() {
    assert_eq!(
        resolve_page_scope(ExportPageScope::Current, Some(1), 3).unwrap(),
        vec![1]
    );
    assert_eq!(
        resolve_page_scope(ExportPageScope::All, Some(1), 3).unwrap(),
        vec![0, 1, 2]
    );
    assert_eq!(
        resolve_page_scope(ExportPageScope::Range { start: 2, end: 3 }, Some(0), 4).unwrap(),
        vec![1, 2]
    );
    assert!(resolve_page_scope(ExportPageScope::Range { start: 3, end: 2 }, Some(0), 4).is_err());
}
