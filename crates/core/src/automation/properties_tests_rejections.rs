//! Layered validation and capability rejection tests.

use super::*;

/// A target the property does not apply to is reported with its reason, and the
/// one it does apply to still lands.
#[test]
fn a_skipped_component_is_reported_not_dropped() {
    let (mut app, _) = contour_app();
    add_line_series(&mut app);
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(9));
    let result = run(&mut app, request).expect("the contour series accepts the write");
    let succeeded = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Succeeded)
        .collect::<Vec<_>>();
    let skipped = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Skipped)
        .collect::<Vec<_>>();
    assert_eq!(succeeded.len(), 1, "{:?}", result.targets);
    assert_eq!(skipped.len(), 1, "{:?}", result.targets);
    assert!(
        skipped[0].message.contains("line"),
        "the skip names the encoding that caused it: {}",
        skipped[0].message
    );
    // The two rows must be distinguishable, or a results panel shows one plot
    // object twice with nothing to tell the rows apart.
    assert_ne!(
        succeeded[0].target.describe(),
        skipped[0].target.describe(),
        "expanded targets carry their component"
    );
    assert!(succeeded[0].target.describe().contains("series"));
}

/// A validation failure leaves every target exactly as it was. A commit that
/// applied to the first series and then failed on the second would be a partial
/// landing, and the ladder of the first would silently disagree with the panel.
#[test]
fn a_validation_failure_lands_on_no_target_at_all() {
    let (mut app, target) = contour_app();
    add_line_series(&mut app);
    let before_revision = app.doc.automation_revision;
    let before_spec = contour_spec(&app, &target);
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(0));
    let error = run(&mut app, request).expect_err("a level count of zero is out of range");
    assert!(error.to_string().contains("out of range"), "{error}");
    assert_eq!(
        app.doc.automation_revision, before_revision,
        "a rejected write never advances the document"
    );
    assert_eq!(
        contour_spec(&app, &target),
        before_spec,
        "a rejected write never reaches a spec"
    );
}

/// A text object exposes the catalog for its own properties, while a plot-only
/// property is still skipped by the owning provider.
#[test]
fn an_object_without_components_is_skipped_by_the_shared_gate() {
    let (mut app, _) = contour_app();
    let canvas = &mut app.doc.canvases[0];
    let id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id,
        name: "Caption".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 20.0, 10.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("hello".to_owned())),
    });
    let text_id = format!("{}/{id}", app.doc.canvases[0].resource_id);
    let plan = plan_tool(
        &app,
        request(
            &app,
            TOOL_INSPECT,
            serde_json::json!({"key": contour::COUNT.as_str()}),
            vec![text_id.clone()],
            CallerType::Agent,
        ),
    )
    .expect("planning succeeds; the target is merely skipped");
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].status, TargetCompatibility::Skipped);
    assert!(
        plan.targets[0].reason.contains("series component"),
        "{}",
        plan.targets[0].reason
    );
    assert!(plan.targets[0].target.component.is_none());
}

/// The three new tools are admitted by capability, and the descriptors say so
/// rather than naming an object type.
#[test]
fn the_property_tools_are_gated_by_capability() {
    let registry = ToolRegistry::built_in();
    registry.validate_unique().expect("ids stay unique");
    for id in [TOOL_INSPECT, TOOL_SET, TOOL_RESET] {
        let descriptor = registry.get(id).unwrap_or_else(|| panic!("{id} exists"));
        assert_eq!(
            descriptor.required_capabilities,
            vec![CapabilityId::new(CAP_PROPERTY_CATALOG)],
            "{id}"
        );
        assert_eq!(
            descriptor.target_kinds,
            vec![
                ResourceKindId::new(KIND_APP),
                ResourceKindId::new(KIND_DOCUMENT),
                ResourceKindId::new(KIND_DATASET),
                ResourceKindId::new(KIND_CANVAS),
                ResourceKindId::new(KIND_CANVAS_OBJECT),
            ],
            "{id}"
        );
    }
    assert_eq!(
        registry.get(TOOL_INSPECT).unwrap().effect,
        EffectLevel::ReadOnly
    );
    assert_eq!(
        registry.get(TOOL_SET).unwrap().effect,
        EffectLevel::Reversible
    );
    assert_eq!(
        registry.get(TOOL_RESET).unwrap().effect,
        EffectLevel::Reversible
    );
    assert!(registry.get(TOOL_SET).unwrap().undoable);
}

/// Whole-encoding reset is deliberately not exposed: its scope needs the caller
/// to name an encoding kind, and a JSON caller that omits it would rebuild every
/// series of an object from defaults while naming only one of them.
#[test]
fn whole_encoding_reset_is_not_a_tool() {
    assert!(
        ToolRegistry::built_in()
            .descriptors()
            .all(|descriptor| descriptor.id != "properties.reset_encoding"),
    );
}

/// Admission to the property tools is a capability question, and the capability
/// is the catalog's own answer about whether a resource has components to
/// address. Deriving it from the dataset variant instead would put a data-domain
/// branch in the admission gate — the one thing the encoding and property
/// registries exist to avoid — and would drift the moment a kind gained or lost
/// addressable components.
#[test]
fn the_catalog_capability_follows_addressable_components_not_the_dataset_kind() {
    let (mut app, _) = contour_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points: (0..8)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.0))
                .collect(),
            domain: Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "capability gate".to_owned(),
            group_delay: 0.0,
        }))));
    let catalog = CapabilityId::new(CAP_PROPERTY_CATALOG);
    let provider = ProjectResourceProvider::new(&app);
    let descriptors = provider.descriptors();
    let mut checked = 0;
    for dataset in &app.doc.datasets {
        let id = dataset.resource_id().to_string();
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.resource.id == id)
            .unwrap_or_else(|| panic!("dataset {id} has a descriptor"));
        assert_eq!(
            descriptor.capabilities.contains(&catalog),
            crate::properties::has_addressable_components(dataset),
            "the capability of {id} disagrees with what the catalog can address"
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "the fixture covers more than one dataset kind"
    );
}
