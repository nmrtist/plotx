use super::pseudo_tests::{synthetic_dosy, wait_for_compute};
use super::*;

#[test]
fn live_binding_projects_the_current_field_and_keeps_external_series() {
    let mut owner = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(owner.build_dosy_map());
    owner.display = PseudoDisplay::Stack;
    let mut external = Nmr2DDataset::load(synthetic_dosy(1.5e-9));
    assert!(external.build_dosy_map());
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(owner)));
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(external)));
    let canvas = crate::workflow::build_default_canvas(&app.doc.datasets[0], "Owner");
    app.doc.canvases.push(canvas);

    let owner_id = app.doc.datasets[0].resource_id();
    let owner_stack = app.doc.datasets[0]
        .field_catalog()
        .id_for_key("nmr.stack")
        .unwrap();
    let initial_collection = app.display_binding(
        Some(owner_id),
        &app.doc.canvases[0].objects[0].plot().unwrap().binding,
    );
    let retained = initial_collection.series[1].clone();
    assert_eq!(retained.primary_color(), Some(OVERLAY_PALETTE[1]));
    let raw = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .binding
        .clone();
    let retained_binding = app.merge_display_binding(
        Some(owner_id),
        &raw,
        DataBinding {
            series: vec![retained],
        },
    );
    app.doc.canvases[0].objects[0].plot_mut().unwrap().binding = retained_binding;
    let initial = app.display_binding(
        Some(owner_id),
        &app.doc.canvases[0].objects[0].plot().unwrap().binding,
    );
    assert_eq!(initial.series.len(), 1);
    assert_eq!(app.stack_candidates(&initial), vec![1]);
    let mut external = app
        .stack_candidate_series(&initial, 1)
        .expect("a map-displaying pseudo dataset still contributes a trace item");
    assert!(external.source.item.is_some());
    external.id = app.doc.canvases[0].objects[0]
        .plot_mut()
        .unwrap()
        .allocate_series_id();
    let external_color = app.next_stack_color(&initial);
    external.set_primary_color(external_color);
    assert_ne!(external.primary_color(), initial.series[0].primary_color());
    let persisted_before = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .binding
        .clone();
    let mut edited = initial;
    edited.series.push(external.clone());
    let stored = app.merge_display_binding(Some(owner_id), &persisted_before, edited);
    app.doc.canvases[0].objects[0].plot_mut().unwrap().binding = stored.clone();
    let displayed = app.display_binding(Some(owner_id), &stored);
    assert!(
        displayed
            .series
            .iter()
            .any(|series| series.id == external.id)
    );
    assert_eq!(external.primary_color(), Some(external_color));
    assert!(displayed.series.iter().all(|series| {
        series.source.resource != owner_id || series.source.field == owner_stack
    }));
    let targets = app.series_targets(0, app.doc.canvases[0].objects[0].id);
    assert_eq!(targets.len(), displayed.series.len());
    let hidden_id = displayed.series[0].id;
    let target = app
        .series_target(0, app.doc.canvases[0].objects[0].id, hidden_id)
        .unwrap();
    let commit = app
        .plan_property_write(
            crate::properties::object::SERIES_VISIBLE,
            &[target],
            &crate::properties::PropertyValue::Bool(false),
        )
        .unwrap();
    assert_eq!(app.commit_property(commit), 1);
    assert!(
        !app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .binding
            .series
            .iter()
            .find(|series| series.id == hidden_id)
            .unwrap()
            .visible
    );
    app.undo();
    assert!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .binding
            .series
            .iter()
            .find(|series| series.id == hidden_id)
            .unwrap()
            .visible
    );
    let figure = app.build_object_figure(
        Some(owner_id),
        &stored,
        &ChartSpec::default_for(DataDomain::PseudoNmr),
        &StackSpec::default(),
        &AxisProjections::default(),
        [120.0, 80.0],
    );
    assert_eq!(figure.series.len(), displayed.series.len());
    assert_ne!(figure.series[0].color, figure.series[1].color);

    app.set_pseudo_display(0, PseudoDisplay::DosyMap);
    let map_display = app.display_binding(Some(owner_id), &stored);
    assert_eq!(map_display.series.len(), 1);
    assert_eq!(map_display.series[0].source.resource, owner_id);
    assert!(matches!(
        map_display.series[0].encoding,
        plotx_figure::SeriesEncoding::Contour(_)
    ));
    wait_for_compute(&mut app);
    assert!(
        !app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure()
            .contours
            .is_empty()
    );
    app.set_pseudo_display(0, PseudoDisplay::Stack);
    assert_eq!(app.display_binding(Some(owner_id), &stored), displayed);

    let external_only = DataBinding {
        series: vec![external.clone()],
    };
    let merged = app.merge_display_binding(Some(owner_id), &stored, external_only.clone());
    assert_eq!(app.display_binding(Some(owner_id), &merged), external_only);
    assert!(merged.series.iter().any(|series| {
        series.source.resource == owner_id && series.source.field != owner_stack
    }));
    let empty = app.merge_display_binding(Some(owner_id), &stored, DataBinding { series: vec![] });
    assert!(
        app.display_binding(Some(owner_id), &empty)
            .series
            .is_empty()
    );
    let empty_figure = app.build_object_figure(
        Some(owner_id),
        &empty,
        &ChartSpec::default_for(DataDomain::PseudoNmr),
        &StackSpec::default(),
        &AxisProjections::default(),
        [120.0, 80.0],
    );
    assert!(empty_figure.series.is_empty() && empty_figure.contours.is_empty());

    app.doc.canvases[0].objects[0].plot_mut().unwrap().binding = stored.clone();
    let path = std::env::temp_dir().join(format!(
        "plotx-live-external-series-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    let loaded_plot = loaded.doc.canvases[0].objects[0].plot().unwrap();
    assert_eq!(loaded_plot.binding, stored);
    assert_eq!(
        loaded.display_binding(loaded_plot.display_owner, &loaded_plot.binding),
        displayed
    );
    assert_eq!(loaded_plot.figure().series.len(), displayed.series.len());
}

#[test]
fn dosy_map_honors_non_default_contour_levels_and_style() {
    use plotx_figure::{
        Color, ColorSource, ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle,
        PositiveFiniteF32, PositiveFiniteF64, SeriesEncoding,
    };

    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(dataset.build_dosy_map());
    let peak = dosy_scalar_grid(dataset.dosy_map.as_ref().unwrap())
        .values
        .iter()
        .copied()
        .fold(0.0_f32, f32::max) as f64;
    let field = dataset.field_catalog.id_for_key("nmr.dosy_map").unwrap();
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
    let mut binding = DataBinding {
        series: SeriesBinding::from_field_all(&app.doc.datasets[0], field),
    };
    let color = Color::rgb(17, 93, 201);
    binding.series[0].encoding = SeriesEncoding::Contour(ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(peak * 0.35).unwrap()),
            count: 2,
            ratio: PositiveFiniteF64::new(1.4).unwrap(),
        },
        negative: None,
        style: ContourStyle {
            positive_color: ColorSource::Explicit(color),
            negative_color: ColorSource::Explicit(Color::rgb(200, 30, 30)),
            width: PositiveFiniteF32::new(2.75).unwrap(),
        },
    });
    let build = |app: &mut PlotxApp, binding: &DataBinding| {
        app.build_binding_figure(
            binding,
            &ChartSpec::default_for(DataDomain::Nmr2d),
            &StackSpec::default(),
            [120.0, 80.0],
        )
    };
    assert!(build(&mut app, &binding).contours.is_empty());
    wait_for_compute(&mut app);
    let styled = build(&mut app, &binding);
    assert!(!styled.contours.is_empty());
    assert!(styled.contours.iter().all(|contour| contour.color == color));
    assert!(styled.contours.iter().all(|contour| contour.width == 2.75));

    let SeriesEncoding::Contour(spec) = &mut binding.series[0].encoding else {
        unreachable!()
    };
    spec.positive.base = ContourBasePolicy::Absolute(PositiveFiniteF64::new(peak * 2.0).unwrap());
    app.session.status.clear();
    assert!(build(&mut app, &binding).contours.is_empty());
    assert!(app.session.status.contains("threshold"));
}

#[test]
fn ilt_map_honors_non_default_contour_style() {
    use plotx_figure::{
        Color, ColorSource, ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle,
        PositiveFiniteF32, PositiveFiniteF64, SeriesEncoding,
    };

    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(dataset.build_ilt_map(IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 32,
    }));
    let peak = dataset
        .ilt_map
        .as_ref()
        .unwrap()
        .amp
        .iter()
        .flatten()
        .copied()
        .fold(0.0_f64, f64::max);
    let field = dataset.field_catalog.id_for_key("nmr.ilt_map").unwrap();
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
    let mut binding = DataBinding {
        series: SeriesBinding::from_field_all(&app.doc.datasets[0], field),
    };
    let color = Color::rgb(31, 151, 87);
    binding.series[0].encoding = SeriesEncoding::Contour(ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(peak * 0.35).unwrap()),
            count: 2,
            ratio: PositiveFiniteF64::new(1.4).unwrap(),
        },
        negative: None,
        style: ContourStyle {
            positive_color: ColorSource::Explicit(color),
            negative_color: ColorSource::Explicit(Color::rgb(180, 40, 40)),
            width: PositiveFiniteF32::new(3.25).unwrap(),
        },
    });
    let pending = app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    assert!(pending.contours.is_empty());
    wait_for_compute(&mut app);
    let styled = app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    assert!(!styled.contours.is_empty());
    assert!(styled.contours.iter().all(|contour| contour.color == color));
    assert!(styled.contours.iter().all(|contour| contour.width == 3.25));
}
