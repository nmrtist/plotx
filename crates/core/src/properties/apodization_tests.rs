//! Dataset processing-step catalog slice.

use super::processing_test_support::{states_2d_app, target_for_axis};
use super::*;
use crate::actions::Action;
use crate::automation::{ComponentRef, ResourceRef, TargetRef};
use crate::state::{Dataset, Nmr2DDataset, PhaseAxis, PlotxApp};
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{Apodization, StepId, StepKind};

fn apodization_target(app: &PlotxApp) -> TargetRef {
    let Dataset::Nmr2D(dataset) = &app.doc.datasets[0] else {
        panic!("the contour fixture owns a 2D NMR dataset");
    };
    let step = dataset
        .params
        .f2
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::Apodize(_)))
        .expect("the time-domain default has an apodization step");
    TargetRef {
        resource: ResourceRef::from(dataset.resource_id),
        component: Some(ComponentRef::ProcessingStep(step.id)),
    }
}

fn time_domain_app() -> PlotxApp {
    time_domain_app_of(None)
}

/// A pseudo-2D acquisition: the indirect dimension is an array, so the
/// application never Fourier transforms it and never shows its recipe.
fn pseudo_2d_app() -> PlotxApp {
    time_domain_app_of(Some("dosy"))
}

fn time_domain_app_of(experiment: Option<&str>) -> PlotxApp {
    let dimension = |nucleus: &str| Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    let data = NmrData2D {
        data: (0..16)
            .map(|value| Complex64::new(f64::from(value), 0.5))
            .collect(),
        rows: 4,
        cols: 4,
        domain: Domain::Time,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: experiment.map(ToOwned::to_owned),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "apodization step".to_owned(),
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data))));
    app
}

fn apodization_at(app: &PlotxApp, target: &TargetRef) -> (StepId, Apodization) {
    let Some(ComponentRef::ProcessingStep(id)) = target.component else {
        panic!("the helper constructs a processing-step target");
    };
    let Dataset::Nmr2D(dataset) = &app.doc.datasets[0] else {
        panic!("the contour fixture owns a 2D NMR dataset");
    };
    let step = dataset
        .params
        .f2
        .steps
        .iter()
        .chain(dataset.params.f1.steps.iter())
        .find(|step| step.id == id)
        .expect("the stable step id still resolves");
    let StepKind::Apodize(apodization) = step.kind else {
        panic!("the target remains an apodization step");
    };
    (step.id, apodization)
}

/// One `StepId`-addressed property changes both which fields exist and the
/// typed processing action that owns the pipeline. No axis index or pipeline
/// path appears at the property boundary.
#[test]
fn apodization_step_has_a_dependent_schema_and_a_typed_processing_action() {
    let mut app = time_domain_app();
    let target = apodization_target(&app);
    let (stable_id, initial) = apodization_at(&app, &target);
    assert_eq!(initial, Apodization::CosineBell);

    let kind = PropertyAddress::new(target.clone(), apodization::KIND);
    let lb = PropertyAddress::new(target.clone(), apodization::LB_HZ);
    let gb = PropertyAddress::new(target.clone(), apodization::GB_HZ);
    assert_eq!(
        app.resolve_property(&kind)
            .expect("the window choice resolves")
            .value,
        AggregateValue::Uniform(PropertyValue::Enum(apodization::APODIZATION_COSINE_BELL))
    );
    assert!(matches!(
        app.resolve_property(&lb),
        Err(PropertyError::NotApplicable(message)) if message.contains("exponential or Gaussian")
    ));

    let commit = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_EXPONENTIAL),
        )
        .expect("an apodization kind change plans");
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("a catalog commit is composite");
    };
    assert!(matches!(
        actions.as_slice(),
        [Action::UpdateDatasetProcessing { .. }]
    ));
    app.commit_property(commit);
    assert_eq!(apodization_at(&app, &target).0, stable_id);
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 1.0 }
    );
    let resolved_lb = app.resolve_property(&lb).expect("LB now exists");
    assert_eq!(
        resolved_lb.value,
        AggregateValue::Uniform(PropertyValue::Float(1.0))
    );
    assert!(matches!(
        resolved_lb.schema,
        ResolvedSchema::Float {
            display: FloatDisplay::Linear("Hz"),
            ..
        }
    ));
    assert!(matches!(
        app.resolve_property(&gb),
        Err(PropertyError::NotApplicable(message)) if message.contains("Gaussian")
    ));

    let commit = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_GAUSSIAN),
        )
        .expect("a Gaussian window plans");
    app.commit_property(commit);
    assert_eq!(apodization_at(&app, &target).0, stable_id);
    assert!(matches!(
        app.resolve_property(&gb)
            .expect("GB appears only for Gaussian")
            .schema,
        ResolvedSchema::Float {
            display: FloatDisplay::Linear("Hz"),
            ..
        }
    ));

    for (property, value) in [
        (apodization::LB_HZ, PropertyValue::Float(2.5)),
        (apodization::GB_HZ, PropertyValue::Float(4.0)),
    ] {
        let commit = app
            .plan_property_write(property, std::slice::from_ref(&target), &value)
            .expect("the visible parameter writes");
        app.commit_property(commit);
    }
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Gaussian {
            lb_hz: 2.5,
            gb_hz: 4.0
        }
    );

    let commit = app
        .plan_property_reset(apodization::KIND, std::slice::from_ref(&target))
        .expect("the target-specific processing factory resets the default step");
    app.commit_property(commit);
    assert_eq!(apodization_at(&app, &target).0, stable_id);
    assert_eq!(apodization_at(&app, &target).1, Apodization::CosineBell);
    assert!(matches!(
        app.resolve_property(&lb),
        Err(PropertyError::NotApplicable(_))
    ));
}

/// The catalog exposes exactly the processing steps the rest of the application
/// exposes. A pseudo-2D acquisition has no transformed indirect dimension, so
/// the Processing panel shows only F2; expanding F1 here would let a headless
/// caller write a recipe nobody can see, navigate to, or undo from a panel, and
/// be told it succeeded.
#[test]
fn a_pseudo_2d_dataset_exposes_only_the_steps_the_application_shows() {
    let app = pseudo_2d_app();
    let Dataset::Nmr2D(dataset) = &app.doc.datasets[0] else {
        panic!("the fixture owns a 2D NMR dataset");
    };
    assert!(
        !dataset.is_true_2d(),
        "the fixture has to be pseudo-2D for this to mean anything"
    );
    let resource = ResourceRef::from(dataset.resource_id);
    let expected = dataset.params.f2.steps.len();
    assert!(!dataset.params.f1.steps.is_empty());

    let targets = app.resource_processing_step_targets(&resource);
    assert_eq!(targets.len(), expected);
    let hidden: Vec<StepId> = dataset.params.f1.steps.iter().map(|step| step.id).collect();
    for target in &targets {
        let Some(ComponentRef::ProcessingStep(id)) = target.component else {
            panic!("a step target names a step");
        };
        assert!(
            !hidden.contains(&id),
            "step {id:?} lives on the axis the application hides"
        );
    }
}

/// The window choice is the panel's own control, so the panel's "Pause
/// auto-recompute" switch has to govern it. A catalog write that recomputed
/// anyway would make the switch silently do nothing, and would never record the
/// pending edit the Apply button is built on.
#[test]
fn a_paused_panel_stages_a_catalog_recipe_write_instead_of_recomputing() {
    let mut app = time_domain_app();
    let target = apodization_target(&app);
    app.session.ui.proc_paused = true;

    let commit = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_EXPONENTIAL),
        )
        .expect("an apodization kind change plans");
    assert_eq!(app.commit_property(commit), 1);

    assert!(
        app.has_pending_processing(),
        "a paused write has to leave an Apply for the user to press"
    );
    assert!(
        !app.can_undo(),
        "a staged recipe is not history until it is applied"
    );
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 1.0 },
        "the staged recipe is live so the panel shows what Apply would do"
    );
}

/// One drag of a continuous control is one edit. Recording a step per frame
/// would spend the bounded history on a single gesture and evict everything
/// before it.
#[test]
fn a_control_gesture_records_one_undo_step_for_the_whole_drag() {
    let mut app = time_domain_app();
    let target = apodization_target(&app);
    let commit = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_EXPONENTIAL),
        )
        .expect("an exponential window plans");
    app.commit_property(commit);
    let history = app.session.undo_stack.len();

    app.begin_property_gesture(apodization::LB_HZ);
    for lb in [2.0, 3.0, 4.0] {
        let commit = app
            .plan_property_write(
                apodization::LB_HZ,
                std::slice::from_ref(&target),
                &PropertyValue::Float(lb),
            )
            .expect("each frame of the drag plans");
        app.commit_property(commit);
        assert_eq!(
            app.session.undo_stack.len(),
            history,
            "a frame of a drag is a live preview, not an edit"
        );
    }
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 4.0 },
        "the preview is live while the drag runs"
    );
    app.end_property_gesture();

    assert_eq!(app.session.undo_stack.len(), history + 1);
    app.undo();
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 1.0 },
        "one undo has to take back the whole gesture"
    );
    app.redo();
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 4.0 },
        "and one redo has to restore where it ended"
    );
}

/// The drag notch is declared, not derived. A range states what is admissible:
/// broadening is legal out to +-10 kHz, so a range-derived notch would move it a
/// hundred hertz per pixel while real values sit between 0.3 and 5 Hz.
#[test]
fn the_broadening_notch_is_declared_rather_than_derived_from_the_range() {
    for id in [apodization::LB_HZ, apodization::GB_HZ] {
        let definition = definition(id).expect("the parameter is registered");
        let ValueSchema::Float {
            bounds, drag_step, ..
        } = definition.value_schema
        else {
            panic!("a broadening parameter is a float");
        };
        let derived = (bounds.max - bounds.min) / 200.0;
        let declared = drag_step.expect("the definition declares its own notch");
        assert!(
            declared <= 1.0 && declared < derived,
            "{id} declares {declared} where the range alone would give {derived}"
        );
    }
}

/// A step the user added is not a deviation from anything. Reporting a factory
/// value for it marked it modified the moment it appeared, and its reset button
/// replaced the window the user had just chosen with a step that does nothing.
#[test]
fn a_hand_added_step_has_no_factory_setting_to_reset_to() {
    let mut app = time_domain_app();
    let Dataset::Nmr2D(dataset) = &mut app.doc.datasets[0] else {
        panic!("the fixture owns a 2D NMR dataset");
    };
    let id = StepId::new(4_096);
    let mut added = plotx_processing::ProcessingStep::new(
        id,
        StepKind::Apodize(Apodization::Exponential { lb_hz: 1.0 }),
        plotx_processing::StepSource::User,
    );
    added.enabled = true;
    dataset.params.f2.steps.insert(0, added);
    let resource = ResourceRef::from(dataset.resource_id);
    let target = TargetRef {
        resource,
        component: Some(ComponentRef::ProcessingStep(id)),
    };

    let kind = PropertyAddress::new(target.clone(), apodization::KIND);
    let resolved = app
        .resolve_property(&kind)
        .expect("the added step resolves");
    assert_eq!(resolved.default_value, None);
    assert!(
        !resolved.is_modified(),
        "a step the user just added is not a change from a default"
    );

    let commit = app
        .plan_property_reset(apodization::KIND, std::slice::from_ref(&target))
        .expect("a reset with nothing to reset is a skip, not a failure");
    assert!(commit.applied.is_empty());
    assert_eq!(commit.skipped.len(), 1);
    assert_eq!(commit.skipped[0].reason, super::SkipReason::NotApplicable);
    app.commit_property(commit);
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Exponential { lb_hz: 1.0 },
        "the window the user chose survives a reset that had nothing to do"
    );
}

/// The factory default is read out of the factory, not re-derived. This is what
/// keeps "what does reset give me" and "what does a new document get" from
/// drifting apart.
#[test]
fn the_step_default_is_read_from_the_pipeline_factory() {
    let app = time_domain_app();
    let target = apodization_target(&app);
    let expected = app.doc.datasets[0]
        .factory_pipeline(PhaseAxis::F2)
        .expect("the direct axis has a factory recipe")
        .steps
        .iter()
        .find_map(|step| match step.kind {
            StepKind::Apodize(apodization) => Some(apodization),
            _ => None,
        })
        .expect("the time-domain factory recipe has an apodization step");

    let kind = PropertyAddress::new(target, apodization::KIND);
    assert_eq!(
        app.resolve_property(&kind)
            .expect("the default step resolves")
            .default_value,
        Some(PropertyValue::Enum(match expected {
            Apodization::None => apodization::APODIZATION_NONE,
            Apodization::CosineBell => apodization::APODIZATION_COSINE_BELL,
            Apodization::Exponential { .. } => apodization::APODIZATION_EXPONENTIAL,
            Apodization::Gaussian { .. } => apodization::APODIZATION_GAUSSIAN,
        }))
    );
}

/// Gaussian broadening is open at zero because the transform says so: the window
/// is `exp(+pi*lb*t - g*t^2)` with `g = (pi*gb)^2 / (4 ln 2)`, so `gb = 0` is
/// unbounded growth and `-gb` behaves as `+gb`. Line broadening keeps both signs,
/// which is what makes resolution enhancement reachable.
#[test]
fn gaussian_broadening_is_open_at_zero_while_line_broadening_keeps_both_signs() {
    let mut app = time_domain_app();
    let target = apodization_target(&app);
    let commit = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_GAUSSIAN),
        )
        .expect("a Gaussian window plans");
    app.commit_property(commit);

    // Switching in has to land on a value the control itself would accept.
    let seeded = apodization_at(&app, &target).1;
    let Apodization::Gaussian { gb_hz, .. } = seeded else {
        panic!("the window is Gaussian now");
    };
    assert_eq!(gb_hz, apodization::GB_DEFAULT_HZ);
    let gb = PropertyAddress::new(target.clone(), apodization::GB_HZ);
    let resolved = app.resolve_property(&gb).expect("GB resolves");
    let ResolvedSchema::Float { bounds, .. } = resolved.schema else {
        panic!("GB is a float");
    };
    assert!(bounds.admits(gb_hz), "the seed is inside its own bound");
    assert_eq!(resolved.default_value, Some(PropertyValue::Float(gb_hz)));

    for refused in [0.0, -1.0] {
        assert!(
            matches!(
                app.plan_property_write(
                    apodization::GB_HZ,
                    std::slice::from_ref(&target),
                    &PropertyValue::Float(refused),
                ),
                Err(PropertyError::InvalidValue { .. })
            ),
            "a Gaussian broadening of {refused} is not a window"
        );
    }
    let commit = app
        .plan_property_write(
            apodization::LB_HZ,
            std::slice::from_ref(&target),
            &PropertyValue::Float(-2.0),
        )
        .expect("a negative line broadening stays reachable");
    app.commit_property(commit);
    assert_eq!(
        apodization_at(&app, &target).1,
        Apodization::Gaussian {
            lb_hz: -2.0,
            gb_hz: apodization::GB_DEFAULT_HZ
        }
    );
}

#[test]
fn states_f1_default_apodization_resolves_and_resets_by_provenance_not_step_id() {
    let mut app = states_2d_app(10, 6);
    let target = target_for_axis(&app, PhaseAxis::F1, |kind| {
        matches!(kind, StepKind::Apodize(_))
    });
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), apodization::KIND))
        .expect("the reminted F1 default resolves");
    assert_eq!(
        resolved.default_value,
        Some(PropertyValue::Enum(apodization::APODIZATION_COSINE_BELL))
    );

    let changed = app
        .plan_property_write(
            apodization::KIND,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(apodization::APODIZATION_EXPONENTIAL),
        )
        .expect("F1 apodization changes");
    app.commit_property(changed);
    let reset = app
        .plan_property_reset(apodization::KIND, std::slice::from_ref(&target))
        .expect("F1 reset plans through the real catalog entry");
    assert_eq!(reset.applied.len(), 1);
    app.commit_property(reset);
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(target, apodization::KIND))
            .expect("the reset F1 step resolves")
            .value,
        AggregateValue::Uniform(PropertyValue::Enum(apodization::APODIZATION_COSINE_BELL))
    );
}

/// A sentence a user reads names the choice the way the control names it. The
/// wire id is what the value is stored and transmitted under, and putting it in
/// prose leaks an identifier into the interface.
#[test]
fn an_unavailable_parameter_names_the_window_the_way_the_control_does() {
    let app = time_domain_app();
    let target = apodization_target(&app);
    let lb = PropertyAddress::new(target, apodization::LB_HZ);
    let Err(PropertyError::NotApplicable(message)) = app.resolve_property(&lb) else {
        panic!("the default 2D window is a cosine bell, which carries no LB");
    };
    assert!(
        message.contains("Cosine bell"),
        "the reason names the choice by its label: {message}"
    );
    assert!(
        !message.contains(apodization::APODIZATION_COSINE_BELL),
        "and never by its wire id: {message}"
    );
}
