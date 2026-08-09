//! Catalog registration and definition uniqueness tests.

use super::*;

/// Two definitions sharing an id would make every lookup, every search hit and
/// every reset ambiguous.
#[test]
fn stable_property_ids_are_unique() {
    let mut ids: Vec<&str> = catalog()
        .iter()
        .map(|definition| definition.id.as_str())
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids must be unique");
}

/// Provider modules are registered only through `GROUPS`. Keeping the catalog
/// derived from that one list means a new family cannot become searchable while
/// its reader/writer was forgotten, or vice versa.
#[test]
fn provider_groups_are_the_catalog_registration() {
    let grouped: Vec<PropertyId> = GROUPS
        .iter()
        .flat_map(|group| group.provider.definitions())
        .map(|definition| definition.id)
        .collect();
    let catalogued: Vec<PropertyId> = catalog().iter().map(|definition| definition.id).collect();
    assert_eq!(catalogued, grouped, "catalog entries come only from GROUPS");
    assert!(
        catalogued.contains(&contour::COUNT),
        "the contour provider must be registered through GROUPS"
    );
    for id in grouped {
        assert!(
            provider_for(id).is_some(),
            "{id} has a definition but no dispatch provider"
        );
    }
}

/// Every entry must be reachable: a definition nothing can address is dead
/// weight that still costs a panel row and a search hit.
#[test]
fn every_definition_declares_an_addressable_shape() {
    for definition in catalog() {
        assert!(
            !definition.canonical_label.is_empty(),
            "{} has no canonical label",
            definition.id
        );
        if definition.access == PropertyAccess::ReadOnly {
            assert!(
                matches!(definition.default_policy, DefaultPolicy::None),
                "{} is read-only and cannot have a default to reset to",
                definition.id
            );
        }
    }
}

#[test]
fn every_derived_default_read_reports_a_reset_target() {
    use crate::state::{CanvasObject, CanvasObjectKind, TextBox};

    let (mut app, series) = super::contour_app();
    let plot_id: crate::state::ObjectId = series
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let heatmap_series = {
        let plot = app.doc.canvases[0]
            .object_mut(plot_id)
            .and_then(|object| object.plot_mut())
            .unwrap();
        let id = plot.allocate_series_id();
        let mut binding = plot.binding.series[0].clone();
        binding.id = id;
        binding.encoding =
            plotx_figure::SeriesEncoding::Heatmap(plotx_figure::HeatmapSpec::default());
        plot.binding.series.push(binding);
        id
    };
    let heatmap = app
        .series_target(0, plot_id, heatmap_series)
        .expect("heatmap target");
    let canvas = &mut app.doc.canvases[0];
    let text_id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id: text_id,
        name: "Text".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 20.0, 10.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("derived default".to_owned())),
    });
    let object = crate::automation::TargetRef::resource(series.resource.clone());
    let text = app.object_target(0, text_id).expect("text target");
    let application = app.app_target();
    for definition in catalog()
        .iter()
        .filter(|definition| matches!(&definition.default_policy, DefaultPolicy::Derived))
    {
        let target = if definition.applicability.encoding == Some(EncodingKind::Heatmap) {
            heatmap.clone()
        } else if definition.scope_kind == ScopeKind::App {
            application.clone()
        } else if matches!(
            definition.id,
            object::TEXT
                | object::TEXT_FONT_SIZE
                | object::TEXT_BOLD
                | object::TEXT_ALIGN
                | object::TEXT_COLOR
        ) {
            text.clone()
        } else {
            object.clone()
        };
        let resolved = app
            .resolve_property(&PropertyAddress::new(target, definition.id))
            .unwrap_or_else(|error| panic!("{} did not resolve: {error}", definition.id));
        assert!(
            resolved.default_value.is_some(),
            "{} declares Derived but read reports no default_value",
            definition.id
        );
    }
}

/// A float definition states the unit of the value it stores, never the unit of
/// the number a control happens to draw. `FloatDisplay::caption` derives the
/// second from the first, so a definition that spells the transformation out
/// again would have it announced twice ("log₁₀ log₁₀ λ").
#[test]
fn a_logarithmic_unit_does_not_restate_its_own_transformation() {
    for definition in catalog() {
        let ValueSchema::Float { display, .. } = definition.value_schema else {
            continue;
        };
        let FloatDisplay::Log10(unit) = display else {
            continue;
        };
        assert!(
            !unit.contains("log"),
            "{} declares the unit {unit:?}; state the domain unit and let the \
             caption add the exponent",
            definition.id
        );
        assert!(
            display.caption().starts_with("log₁₀"),
            "{} must announce that its control edits an exponent",
            definition.id
        );
    }
}
