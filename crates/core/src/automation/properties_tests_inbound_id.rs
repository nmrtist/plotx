//! Inbound property identity decoding tests.

use super::*;

#[test]
fn an_unknown_property_key_is_refused_rather_than_skipped() {
    let (mut app, _) = contour_app();
    let error = plan_tool(
        &app,
        set_request(&app, "series.contour.nonexistent", serde_json::json!(3)),
    )
    .expect_err("an unknown key cannot be planned");
    let message = error.to_string();
    assert!(
        message.contains("unknown property 'series.contour.nonexistent'"),
        "{message}"
    );
    // And it never reaches execution, so nothing is silently committed.
    let before = app.doc.automation_revision;
    let request = set_request(&app, "series.contour.nonexistent", serde_json::json!(3));
    assert!(run(&mut app, request).is_err());
    assert_eq!(app.doc.automation_revision, before);
}

#[test]
fn every_property_definition_is_reachable_by_key() {
    for definition in crate::properties::catalog() {
        assert!(
            definition_by_key(definition.id.as_str()).is_some(),
            "{} is not reachable by its own key",
            definition.id
        );
    }
}
