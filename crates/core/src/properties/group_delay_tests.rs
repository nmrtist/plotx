use super::*;
use crate::actions::{Action, DatasetProcessingState};
use crate::automation::{ResourceRef, TargetRef};
use crate::state::{Dataset, Nmr2DDataset, PlotxApp};
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};

fn time_domain_2d_app() -> PlotxApp {
    let dimension = |nucleus: &str| Dim {
        spectral_width_hz: 2_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: nucleus.to_owned(),
        group_delay: 4.0,
    };
    let data = NmrData2D {
        data: (0..64)
            .map(|index| Complex64::new((index as f64 * 0.2).sin(), 0.1))
            .collect(),
        rows: 8,
        cols: 8,
        domain: Domain::Time,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "group delay property".to_owned(),
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data).unwrap())));
    app
}

#[test]
fn two_dimensional_group_delay_is_in_the_typed_action_and_is_undoable() {
    let mut app = time_domain_2d_app();
    let resource = app.doc.datasets[0].resource_id();
    let target = TargetRef {
        resource: ResourceRef::from(resource),
        component: None,
    };
    let commit = app
        .plan_property_write(
            group_delay::CORRECT,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .expect("the dataset-level property plans");
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("the catalog commit is composite");
    };
    assert!(matches!(
        actions.as_slice(),
        [Action::UpdateDatasetProcessing {
            before: DatasetProcessingState::Nmr2D {
                group_delay_correct: true,
                ..
            },
            after: DatasetProcessingState::Nmr2D {
                group_delay_correct: false,
                ..
            },
            ..
        }]
    ));
    app.commit_property(commit);
    assert!(
        !app.doc.datasets[0]
            .as_nmr2d()
            .expect("the dataset remains 2D NMR")
            .group_delay_correct
    );
    let disabled_input = app.doc.datasets[0]
        .as_nmr2d()
        .unwrap()
        .data
        .source_dataset();
    let delay = disabled_input
        .dataset()
        .as_raw()
        .unwrap()
        .descriptor()
        .axes()[1]
        .group_delay();
    assert!(
        matches!(delay, nmr::acquisition::GroupDelayState::Pending(value) if value.delay_points() == 4.0)
    );

    app.undo();
    assert!(
        app.doc.datasets[0]
            .as_nmr2d()
            .expect("the dataset remains 2D NMR")
            .group_delay_correct
    );
}

#[test]
fn two_dimensional_group_delay_settings_produce_different_real_spectra() {
    let mut app = time_domain_2d_app();
    let target = TargetRef {
        resource: ResourceRef::from(app.doc.datasets[0].resource_id()),
        component: None,
    };
    let corrected = {
        let dataset = app.doc.datasets[0].as_nmr2d().unwrap();
        plotx_processing::nmr_execution::execute_2d(
            dataset.data.source_dataset(),
            &dataset.params,
            if dataset.group_delay_correct {
                plotx_processing::nmr_bridge::DelayPolicy::AxisEvidence
            } else {
                plotx_processing::nmr_bridge::DelayPolicy::Disabled
            },
            plotx_processing::nmr_bridge::RecipeRange::Base,
            None,
            &mut nmr::ExecutionContext::default(),
        )
        .unwrap()
        .view
    };
    let changed = app
        .plan_property_write(
            group_delay::CORRECT,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .unwrap();
    app.commit_property(changed);
    let uncorrected = {
        let dataset = app.doc.datasets[0].as_nmr2d().unwrap();
        plotx_processing::nmr_execution::execute_2d(
            dataset.data.source_dataset(),
            &dataset.params,
            if dataset.group_delay_correct {
                plotx_processing::nmr_bridge::DelayPolicy::AxisEvidence
            } else {
                plotx_processing::nmr_bridge::DelayPolicy::Disabled
            },
            plotx_processing::nmr_bridge::RecipeRange::Base,
            None,
            &mut nmr::ExecutionContext::default(),
        )
        .unwrap()
        .view
    };
    let (
        plotx_processing::Processed2D::Ft(corrected),
        plotx_processing::Processed2D::Ft(uncorrected),
    ) = (corrected, uncorrected)
    else {
        panic!("the HSQC fixture produces a true 2D spectrum");
    };
    assert_ne!(corrected.data, uncorrected.data);
}

#[test]
fn group_delay_reset_uses_the_same_factory_rule_as_dataset_construction() {
    let mut app = time_domain_2d_app();
    let target = TargetRef {
        resource: ResourceRef::from(app.doc.datasets[0].resource_id()),
        component: None,
    };
    let changed = app
        .plan_property_write(
            group_delay::CORRECT,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .unwrap();
    app.commit_property(changed);
    let reset = app
        .plan_property_reset(group_delay::CORRECT, std::slice::from_ref(&target))
        .unwrap();
    assert_eq!(reset.applied.len(), 1);
    app.commit_property(reset);
    assert!(app.doc.datasets[0].as_nmr2d().unwrap().group_delay_correct);
}

#[test]
fn reset_preserves_unknown_delay_and_processed_input_defaults() {
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../io/tests/fixtures/nmr");
    for name in ["jeol-complex.jdf", "bruker-1d/pdata/1/1r"] {
        let loaded = plotx_io::load_path(fixtures.join(name)).unwrap();
        let plotx_io::Acquisition::Nmr(source) = loaded.acquisition else {
            panic!("expected NMR fixture");
        };
        let mut app = PlotxApp::new();
        app.doc.datasets.push(Dataset::Nmr(Box::new(
            crate::state::NmrDataset::load(source).unwrap(),
        )));
        let target = TargetRef {
            resource: ResourceRef::from(app.doc.datasets[0].resource_id()),
            component: None,
        };
        let reset = app
            .plan_property_reset(group_delay::CORRECT, &[target])
            .unwrap();
        app.commit_property(reset);
        assert!(!app.doc.datasets[0].as_nmr().unwrap().group_delay_correct);
        let reset = crate::project::reset_processing(&app.doc.datasets[0]).unwrap();
        reset.apply_to(&mut app.doc.datasets[0]).unwrap();
        let dataset = app.doc.datasets[0].as_nmr().unwrap();
        assert!(!dataset.group_delay_correct);
        assert!(!dataset.pipeline.has_enabled_fft());
        assert_eq!(dataset.output_domain(), dataset.input_domain());
    }
}

#[test]
fn reset_of_an_imported_2d_spectrum_does_not_add_time_domain_steps() {
    let app = time_domain_2d_app();
    let source = app.doc.datasets[0]
        .as_nmr2d()
        .unwrap()
        .native_processed
        .clone();
    let mut dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(source).unwrap()));
    let reset = crate::project::reset_processing(&dataset).unwrap();
    reset.apply_to(&mut dataset).unwrap();
    let dataset = dataset.as_nmr2d().unwrap();
    assert!(!dataset.params.f1.has_enabled_fft());
    assert!(!dataset.params.f2.has_enabled_fft());
    assert!(!dataset.group_delay_correct);
}
