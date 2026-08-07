//! Pseudo-2D dataset tests (DOSY map extraction).

use super::*;
use num_complex::Complex64;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};

pub(super) fn wait_for_compute(app: &mut PlotxApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while app.compute_busy() && std::time::Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(
        !app.compute_busy(),
        "field job did not settle before deadline"
    );
}

fn dim(nucleus: &str) -> Dim {
    Dim {
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.into(),
        group_delay: 0.0,
    }
}

// A synthetic DOSY array: one decaying resonance whose amplitude follows a
// Stejskal–Tanner decay with a known D across 16 linear gradient steps.
pub(super) fn synthetic_dosy(d_true: f64) -> NmrData2D {
    let (cols, rows) = (256usize, 16usize);
    let meta = DiffusionMeta {
        gamma: 2.675_222e8,
        delta: 2e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    };
    let g: Vec<f64> = (0..rows)
        .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / (rows as f64 - 1.0))
        .collect();
    let direct = dim("1H");
    let dt = 1.0 / direct.spectral_width_hz;
    let f_hz = 1.0 * direct.observe_freq_mhz; // 1 ppm
    let mut data = Vec::with_capacity(rows * cols);
    for &gr in &g {
        let att = (-d_true * meta.b_factor(gr)).exp();
        for j in 0..cols {
            let t = j as f64 * dt;
            let decay = (-t / 0.2).exp();
            data.push(Complex64::from_polar(
                att * decay,
                std::f64::consts::TAU * f_hz * t,
            ));
        }
    }
    NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct,
        indirect: dim("1H"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("bpp_ste_diffusion".into()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".into(),
            kind: PseudoKind::Gradient,
            values: g,
            unit: "mT/m".into(),
            source: AxisSource::EmbeddedRamp,
        }),
        diffusion: Some(meta),
        nus: None,
        source: "synthetic DOSY".into(),
    }
}

#[test]
fn dataset_builds_dosy_map() {
    let d_true = 1.2e-9;
    let mut ds = Nmr2DDataset::load(synthetic_dosy(d_true));
    assert!(ds.is_pseudo());
    assert_eq!(ds.preset, Preset2D::Dosy);

    assert!(ds.build_dosy_map());
    assert_eq!(ds.display, PseudoDisplay::DosyMap);
    assert!(!ds.figure().contours.is_empty(), "DOSY map should contour");
}

#[test]
fn ordered_series_supports_region_analysis() {
    let series = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(synthetic_dosy(1.2e-9))));
    assert!(series.supports_region_analysis());
    assert!(series.tool_groups().contains(&ToolGroup::RegionAnalysis));

    let mut without_ruler = synthetic_dosy(1.2e-9);
    without_ruler.pseudo_axis = None;
    let not_a_series = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(without_ruler)));
    assert!(!not_a_series.supports_region_analysis());
    assert!(
        !not_a_series
            .tool_groups()
            .contains(&ToolGroup::RegionAnalysis)
    );
}

/// `supports_region_analysis` gates the Regions and Series Table commands, so it
/// must agree with the predicate `build_region_table` enforces. A dataset that
/// says yes and then yields no table would strand the user with regions drawn
/// and a button that silently does nothing.
#[test]
fn region_support_matches_what_the_table_builder_accepts() {
    let mut app = crate::state::PlotxApp::new_with_settings(crate::settings::Settings::default());
    let mut series = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    series.region_analysis.regions = vec![Region {
        id: RegionId::new(0),
        lo: 0.9,
        hi: 1.1,
        name: "peak".to_owned(),
        label_position: None,
        color: [200, 80, 80],
        metric: None,
    }];
    series.region_analysis.next_region_id = RegionId::new(1);
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(series)));
    assert!(app.doc.datasets[0].supports_region_analysis());
    app.create_region_table(0);
    assert!(
        app.region_table_index(0).is_some(),
        "a supported series with regions must actually yield a table"
    );

    // Regions survive in saved projects, so a dataset can carry them without
    // being a series. The support predicate must reject it, which is what keeps
    // the Series Table command from offering a table that cannot be built.
    let mut ruler_less = synthetic_dosy(1.2e-9);
    ruler_less.pseudo_axis = None;
    let mut stale = Nmr2DDataset::load(ruler_less);
    stale.region_analysis.regions = vec![Region {
        id: RegionId::new(0),
        lo: 0.9,
        hi: 1.1,
        name: "peak".to_owned(),
        label_position: None,
        color: [200, 80, 80],
        metric: None,
    }];
    let mut app = crate::state::PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(stale)));
    assert!(!app.doc.datasets[0].supports_region_analysis());
    app.create_region_table(0);
    assert!(
        app.region_table_index(0).is_none(),
        "an unsupported dataset must not produce a series table"
    );
}

#[test]
fn dataset_builds_ilt_dosy_map() {
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    let params = IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 64,
    };
    assert!(ds.build_ilt_map(params), "ILT map should populate");
    assert!(matches!(ds.dosy_method, DosyMethod::Ilt(_)));
    assert_eq!(ds.display, PseudoDisplay::DosyMap);
    assert!(!ds.figure().contours.is_empty(), "ILT map should contour");
    // The per-column mono-exp path must still coexist.
    assert!(ds.build_dosy_map());
    assert!(matches!(ds.dosy_method, DosyMethod::MonoExp));
    assert!(ds.ilt_map.is_some(), "ILT result should remain cached");
}

#[test]
fn pseudo_map_fields_are_truthful_scalar_grids_with_map_encodings() {
    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(dataset.build_dosy_map());
    assert!(dataset.build_ilt_map(IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 32,
    }));
    let dataset = Dataset::Nmr2D(Box::new(dataset));
    let fields = dataset.field_descriptors();
    assert!(fields.iter().any(|field| field.local_id == "nmr.stack"));
    for key in ["nmr.dosy_map", "nmr.ilt_map"] {
        let field = fields.iter().find(|field| field.local_id == key).unwrap();
        assert!(matches!(
            dataset.field_payload(field.id),
            Some(FieldPayload::ScalarGrid2D(ScalarGrid2D { rows, cols, ref values, .. }))
                if rows > 1 && cols > 1 && values.len() == rows * cols
        ));
        let binding = SeriesBinding::from_field_all(&dataset, field.id);
        assert!(matches!(
            binding[0].encoding,
            plotx_figure::SeriesEncoding::Contour(_)
        ));
        assert!(dataset.supports_encoding(field.id, &binding[0].encoding));
        assert_eq!(
            field
                .metadata
                .0
                .get("recommended_encoding")
                .map(String::as_str),
            Some("contour")
        );
    }
}

/// Both maps can be cached at once, so the figure cache must be keyed by method.
/// A single shared slot would serve whichever figure was built last for whichever
/// method the display happens to select — an ILT contour labelled per-column DOSY.
#[test]
fn switching_dosy_method_serves_that_methods_figure() {
    let params = IltParams {
        lambda: 1e-2,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 64,
    };
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(ds.build_dosy_map(), "per-column map should populate");
    assert!(ds.build_ilt_map(params), "ILT map should populate");
    assert!(ds.figure().title.starts_with("DOSY (ILT)"));

    // Switching back is what the DOSY method buttons do: flip the method and
    // rebuild. Both maps are still cached, so the per-column figure must come
    // back rather than the ILT figure that happened to be built last.
    ds.dosy_method = DosyMethod::MonoExp;
    let title = ds.figure().title;
    assert!(
        title.starts_with("DOSY —"),
        "per-column display served the wrong method's figure: {title}"
    );

    ds.dosy_method = DosyMethod::Ilt(params);
    assert!(ds.figure().title.starts_with("DOSY (ILT)"));
}

/// A NUS schedule mutates `data` while leaving the recipe untouched, so nothing in
/// `params` records that the cached base is void. Without the explicit flag, a
/// frequency-only edit arriving before the reconstruction lands would schedule a
/// re-apply from the pre-NUS base and strand the reconstruction forever.
#[test]
fn entering_a_nus_schedule_forces_a_retransform_until_a_base_lands() {
    let mut data = synthetic_dosy(1.2e-9);
    data.nus = Some(plotx_io::NusMeta {
        grid: data.rows * 2,
        acquired: data.rows,
        idx_base: 0,
        mode: String::new(),
        echo_antiecho: false,
        schedule: None,
    });
    let mut ds = Nmr2DDataset::load(data);
    assert!(!ds.base_stale);

    let rows = ds.data.rows;
    ds.set_nus_schedule(&(0..rows).collect::<Vec<_>>(), 0)
        .expect("a full in-grid schedule is valid");
    assert!(ds.base_stale, "the cached base no longer derives from data");

    ds.retransform();
    assert!(!ds.base_stale, "a fresh base clears the flag");
}

#[test]
fn persisted_display_and_method_changes_mark_the_document_dirty() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(
            synthetic_dosy(1.2e-9),
        ))));

    app.doc.dirty = false;
    app.set_pseudo_display(0, PseudoDisplay::DosyMap);
    assert!(
        app.doc.dirty,
        "changing the persisted display must be dirty"
    );

    app.doc.dirty = false;
    let params = IltParams {
        lambda: 0.03,
        ..IltParams::default()
    };
    app.set_pseudo_dosy_method(0, DosyMethod::Ilt(params));
    assert!(app.doc.dirty, "changing the persisted method must be dirty");
}

#[test]
fn switching_an_existing_stack_canvas_to_dosy_rebuilds_it_as_a_map() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(dataset.build_dosy_map());
    dataset.display = PseudoDisplay::Stack;
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));

    let mut canvas = CanvasDocument::new("DOSY".to_owned(), [120.0, 80.0]);
    let [width, height] = canvas.size_pt();
    let object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, width, height),
        canvas.allocate_object_id(),
        "DOSY".to_owned(),
    );
    assert!(object.plot().unwrap().figure().heatmap.is_none());
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);

    {
        let plot = app.doc.canvases[0].objects[0].plot_mut().unwrap();
        plot.binding.series.swap(0, 1);
        let series = &mut plot.binding.series[0];
        series.label = Some("authored increment".to_owned());
        if let plotx_figure::SeriesEncoding::Line(line) = &mut series.encoding {
            line.scale = 1.75;
            line.width = plotx_figure::PositiveFiniteF32::new(2.25).unwrap();
            line.color = plotx_figure::ColorSource::Explicit(plotx_figure::Color::rgb(12, 34, 56));
        }
    }

    let authored_stack = {
        let plot = app.doc.canvases[0].objects[0].plot().unwrap();
        let stack_field = app.doc.datasets[0]
            .field_catalog()
            .id_for_key("nmr.stack")
            .unwrap();
        DataBinding {
            series: plot
                .binding
                .series
                .iter()
                .filter(|series| series.source.field == stack_field)
                .cloned()
                .collect(),
        }
    };
    let extracted = DataBinding {
        series: vec![authored_stack.series[0].clone()],
    };
    app.set_pseudo_display(0, PseudoDisplay::DosyMap);
    wait_for_compute(&mut app);
    let plot = app.doc.canvases[0].objects[0].plot().unwrap();
    let display_field = app.doc.datasets[0]
        .field_catalog()
        .id_for_key("nmr.dosy_map")
        .unwrap();
    assert!(
        plot.binding
            .series
            .iter()
            .any(|series| { series.source.field == display_field && series.source.item.is_none() })
    );
    let figure = plot.figure();
    assert!(
        figure.heatmap.is_some() || !figure.contours.is_empty(),
        "the rebuilt canvas must show the selected DOSY map"
    );

    let trace = app.build_binding_figure(
        &extracted,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    assert_eq!(trace.series.len(), 1, "an extracted item remains a trace");
    assert!(trace.heatmap.is_none() && trace.contours.is_empty());

    let independent_id = app.doc.canvases[0].allocate_object_id();
    let mut independent = app.build_plot_object(
        0,
        ObjectFrame::new(4.0, 4.0, 100.0, 60.0),
        independent_id,
        "Extracted increment".to_owned(),
    );
    {
        let plot = independent.plot_mut().unwrap();
        plot.display_owner = None;
        plot.binding = extracted.clone();
    }
    app.doc.canvases[0].objects.push(independent);
    app.rebuild_canvas(0);
    assert_eq!(
        app.doc.canvases[0].objects[1]
            .plot()
            .unwrap()
            .figure()
            .series
            .len(),
        1
    );

    let default_binding_before_save = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .binding
        .clone();
    let path = std::env::temp_dir().join(format!(
        "plotx-pseudo-extracted-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    let loaded_plot = loaded.doc.canvases[0].objects[1].plot().unwrap();
    assert_eq!(
        loaded.doc.canvases[0].objects[0].plot().unwrap().binding,
        default_binding_before_save,
        "palette colors and all stack authoring state must round-trip"
    );
    assert_eq!(loaded_plot.display_owner, None);
    assert_eq!(loaded_plot.binding, extracted);
    assert_eq!(loaded_plot.figure().series.len(), 1);

    app.set_pseudo_display(0, PseudoDisplay::Stack);
    let plot = app.doc.canvases[0].objects[0].plot().unwrap();
    let stack_field = app.doc.datasets[0]
        .field_catalog()
        .id_for_key("nmr.stack")
        .unwrap();
    let restored = DataBinding {
        series: plot
            .binding
            .series
            .iter()
            .filter(|series| series.source.field == stack_field)
            .cloned()
            .collect(),
    };
    assert_eq!(restored, authored_stack);
}

#[test]
fn processing_invalidation_explains_the_stack_fallback() {
    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(dataset.build_dosy_map());
    assert_eq!(dataset.display, PseudoDisplay::DosyMap);

    dataset.rebuild();

    assert!(dataset.dosy_map.is_none());
    assert!(dataset.figure().title.starts_with("Pseudo-2D stack —"));
    let warning = dataset
        .dosy_provenance_warning
        .as_deref()
        .expect("processing invalidation must explain the stack fallback");
    assert!(warning.contains("Processing changed"), "{warning}");
    assert!(warning.contains("showing the stack"), "{warning}");
    assert!(warning.contains("Build"), "{warning}");
}

#[test]
fn ilt_invocation_resolution_obeys_explicit_provenance_default_and_reports_empty_explicit_runs() {
    let mut settings = crate::settings::Settings::default();
    settings.processing.ilt_lambda = 0.8;
    let mut app = PlotxApp::new_with_settings(settings);
    let mut dataset = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    let previous = IltParams {
        lambda: 0.03,
        d_min: 1e-10,
        d_max: 1e-8,
        n_grid: 32,
    };
    assert!(dataset.build_ilt_map(previous));
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));

    let explicit = IltParams {
        lambda: 0.5,
        ..IltParams::default()
    };
    assert_eq!(app.resolve_ilt_params_for(0, Some(explicit)), explicit);
    assert_eq!(app.resolve_ilt_params_for(0, None), previous);
    app.doc.datasets[0].as_nmr2d_mut().unwrap().ilt_provenance = None;
    assert_eq!(app.resolve_ilt_params_for(0, None).lambda, 0.8);

    let mut empty = synthetic_dosy(1.2e-9);
    empty.data.fill(Complex64::new(0.0, 0.0));
    let mut empty_app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    empty_app
        .doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(empty))));
    // Deliberately not a boundary value: at MIN or MAX the assertion below would
    // be satisfied by the range text the same message prints, and would still
    // pass with the value itself removed from the message.
    let chosen = IltParams {
        lambda: 0.37,
        ..IltParams::default()
    };
    assert!(
        chosen.lambda != crate::settings::MIN_ILT_LAMBDA
            && chosen.lambda != crate::settings::MAX_ILT_LAMBDA
    );
    empty_app.build_ilt_map_for_with_params(0, Some(chosen));
    assert_eq!(
        empty_app.doc.datasets[0].as_nmr2d().unwrap().dosy_method,
        DosyMethod::Ilt(chosen),
        "a legal explicit lambda is obeyed even when the result is empty"
    );
    assert!(
        empty_app
            .session
            .status
            .contains(&chosen.lambda.to_string()),
        "{}",
        empty_app.session.status
    );
    assert!(
        empty_app
            .session
            .status
            .contains(&crate::settings::MIN_ILT_LAMBDA.to_string())
            && empty_app
                .session
                .status
                .contains(&crate::settings::MAX_ILT_LAMBDA.to_string()),
        "{}",
        empty_app.session.status
    );
}

/// A build that fits nothing still installs the method, the map and its
/// provenance — all persisted state. Dirtying only the populated branch would let
/// those changes be lost on close with no save prompt, which is the silent form
/// of data loss this branch exists to remove.
#[test]
fn a_build_that_fits_nothing_still_marks_the_document_dirty() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    // Zero signal: every column falls below the noise threshold, so both builders
    // return `false` while still writing their results.
    let mut empty = synthetic_dosy(1.2e-9);
    empty
        .data
        .iter_mut()
        .for_each(|value| *value = Complex64::ZERO);
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(empty))));

    app.doc.dirty = false;
    app.build_dosy_map_for(0);
    let d2 = app.doc.datasets[0].as_nmr2d().unwrap();
    assert!(d2.dosy_map.is_some(), "the empty map is still installed");
    assert!(d2.dosy_provenance.is_some());
    assert!(
        app.doc.dirty,
        "an empty per-column build still changed persisted state"
    );

    app.doc.dirty = false;
    let params = IltParams {
        lambda: 0.03,
        ..IltParams::default()
    };
    app.build_ilt_map_for_with_params(0, Some(params));
    let d2 = app.doc.datasets[0].as_nmr2d().unwrap();
    assert_eq!(d2.dosy_method, DosyMethod::Ilt(params));
    assert!(d2.ilt_provenance.is_some());
    assert!(
        app.doc.dirty,
        "an empty ILT build still changed persisted state"
    );
}

/// The "map is missing" note is derived, never stored, so it cannot outlive the
/// condition. A stored copy would keep claiming the map is unavailable while the
/// map is on screen.
#[test]
fn the_missing_map_note_tracks_the_current_selection_instead_of_persisting() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    assert!(ds.build_dosy_map());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let d2 = app.doc.datasets[0].as_nmr2d().unwrap();
    assert_eq!(d2.display, PseudoDisplay::DosyMap);
    assert_eq!(d2.missing_selected_map_note(), None);

    // Selecting ILT, for which no map exists, must explain the stack fallback…
    app.set_pseudo_dosy_method(0, DosyMethod::Ilt(IltParams::default()));
    let note = app.doc.datasets[0]
        .as_nmr2d()
        .unwrap()
        .missing_selected_map_note()
        .expect("selecting a method with no map explains the fallback");
    assert!(note.contains("ILT DOSY map is not available"), "{note}");

    // …and going back to the method that does have one must stop explaining it.
    app.set_pseudo_dosy_method(0, DosyMethod::MonoExp);
    assert_eq!(
        app.doc.datasets[0]
            .as_nmr2d()
            .unwrap()
            .missing_selected_map_note(),
        None,
        "the note must not outlive the condition it describes"
    );

    // Nor should it fire when no map is selected at all.
    app.set_pseudo_dosy_method(0, DosyMethod::Ilt(IltParams::default()));
    app.set_pseudo_display(0, PseudoDisplay::Stack);
    assert_eq!(
        app.doc.datasets[0]
            .as_nmr2d()
            .unwrap()
            .missing_selected_map_note(),
        None
    );
}

/// The fingerprint has to cover every numeric input the fit consumes, not just
/// the trace samples. Referencing moves the chemical-shift axis without touching
/// a sample, and the diffusion metadata reaches the per-column fit through the
/// b-factor conversion; either change produces a different map, so either must
/// produce a different digest.
#[test]
fn the_data_fingerprint_covers_coordinates_and_diffusion_metadata() {
    let mut ds = Nmr2DDataset::load(synthetic_dosy(1.2e-9));
    let Processed2D::Stack(stack) = &ds.processed else {
        panic!("synthetic DOSY must process as a stack");
    };
    let stack = stack.clone();
    let axis = ds.data.pseudo_axis.clone().unwrap();
    let meta = ds.data.diffusion.unwrap();
    let base = crate::state::dosy_data_fingerprint(&stack, &axis.values, &meta);

    let mut shifted = (*stack).clone();
    shifted.ppm.iter_mut().for_each(|value| *value += 0.5);
    assert_ne!(
        crate::state::dosy_data_fingerprint(&shifted, &axis.values, &meta),
        base,
        "a reference change moves the ppm axis and must be detected"
    );

    let mut edited = meta;
    edited.big_delta *= 2.0;
    assert_ne!(
        crate::state::dosy_data_fingerprint(&stack, &axis.values, &edited),
        base,
        "diffusion metadata reaches the fit and must be detected"
    );

    let mut ruler = axis.values.clone();
    ruler[0] += 1e-3;
    assert_ne!(
        crate::state::dosy_data_fingerprint(&stack, &ruler, &meta),
        base,
        "the gradient ruler must be detected"
    );

    // Guard against the digest becoming trivially input-insensitive.
    assert_eq!(
        crate::state::dosy_data_fingerprint(&stack, &axis.values, &meta),
        base
    );
    let _ = &mut ds;
}

/// Grid parameters can arrive from a project file, so they are external input.
/// The inversion solves a dense `n_grid x n_grid` system; an unchecked value
/// would size that allocation instead of producing an error.
#[test]
fn ilt_parameters_from_a_project_are_validated_before_the_inversion() {
    use crate::settings::{MAX_ILT_GRID, MAX_ILT_LAMBDA};
    let ok = IltParams::default();
    assert!(crate::state::validate_ilt_params(ok).is_ok());

    let huge_grid = IltParams {
        n_grid: 4_000_000,
        ..ok
    };
    let message = crate::state::validate_ilt_params(huge_grid)
        .expect_err("an out-of-range grid size must be refused");
    assert!(message.contains("4000000"), "{message}");
    assert!(message.contains(&MAX_ILT_GRID.to_string()), "{message}");

    let reversed = IltParams {
        d_min: 1e-8,
        d_max: 1e-11,
        ..ok
    };
    let message = crate::state::validate_ilt_params(reversed)
        .expect_err("a reversed diffusion range must be refused");
    assert!(
        message.contains("1e-8") && message.contains("1e-11"),
        "{message}"
    );

    let unhinged = IltParams {
        d_min: f64::NAN,
        ..ok
    };
    assert!(crate::state::validate_ilt_params(unhinged).is_err());

    let bad_lambda = IltParams {
        lambda: MAX_ILT_LAMBDA * 10.0,
        ..ok
    };
    assert!(crate::state::validate_ilt_params(bad_lambda).is_err());

    // And the build path must actually consult it rather than reaching the
    // inversion with the value.
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(
            synthetic_dosy(1.2e-9),
        ))));
    app.build_ilt_map_for_with_params(0, Some(huge_grid));
    assert!(
        app.doc.datasets[0].as_nmr2d().unwrap().ilt_map.is_none(),
        "the inversion must not have run"
    );
    assert!(
        app.session.status.contains("4000000"),
        "{}",
        app.session.status
    );
}
