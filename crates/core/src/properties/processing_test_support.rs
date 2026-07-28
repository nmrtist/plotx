use crate::automation::{ComponentRef, ResourceRef, TargetRef};
use crate::state::{Dataset, Nmr2DDataset, NmrDataset, PhaseAxis, PlotxApp};
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData, NmrData2D, QuadMode};
use plotx_processing::{ProcessingStep, StepKind, StepSource};

pub(super) fn time_domain_app() -> PlotxApp {
    let points = (0..64)
        .map(|index| {
            let time = index as f64 / 2_000.0;
            let envelope = (-18.0 * time).exp();
            let phase = std::f64::consts::TAU * 230.0 * time;
            Complex64::from_polar(envelope, phase)
                + Complex64::from_polar(0.35 * (-7.0 * time).exp(), phase * 0.43 + 0.2)
        })
        .collect();
    let data = NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: 2_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: "1H".to_owned(),
        source: "processing property test".to_owned(),
        group_delay: 0.0,
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(data))));
    app
}

pub(super) fn states_2d_app(rows: usize, cols: usize) -> PlotxApp {
    let dim = |nucleus: &str, width| Dim {
        spectral_width_hz: width,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    let data = NmrData2D {
        data: (0..rows * cols)
            .map(|index| Complex64::new((index as f64 * 0.17).sin(), 0.2))
            .collect(),
        rows,
        cols,
        domain: Domain::Time,
        direct: dim("1H", 2_400.0),
        indirect: dim("13C", 1_200.0),
        quad: QuadMode::States,
        indirect_conjugate: false,
        experiment: Some("hsqc".to_owned()),
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "States property test".to_owned(),
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data))));
    app
}

pub(super) fn target_for_axis(
    app: &PlotxApp,
    axis: PhaseAxis,
    accepts: impl Fn(&StepKind) -> bool,
) -> TargetRef {
    let Dataset::Nmr2D(dataset) = &app.doc.datasets[0] else {
        panic!("the fixture owns a 2D NMR dataset");
    };
    let pipeline = match axis {
        PhaseAxis::F1 => &dataset.params.f1,
        PhaseAxis::F2 => &dataset.params.f2,
        PhaseAxis::Direct => panic!("a 2D fixture has no Direct axis"),
    };
    let step = pipeline
        .steps
        .iter()
        .find(|step| accepts(&step.kind))
        .expect("the requested axis step exists");
    TargetRef {
        resource: ResourceRef::from(dataset.resource_id),
        component: Some(ComponentRef::ProcessingStep(step.id)),
    }
}

pub(super) fn target_for(app: &PlotxApp, accepts: impl Fn(&StepKind) -> bool) -> TargetRef {
    let Dataset::Nmr(dataset) = &app.doc.datasets[0] else {
        panic!("the fixture owns a 1D NMR dataset");
    };
    let step = dataset
        .pipeline
        .steps
        .iter()
        .find(|step| accepts(&step.kind))
        .expect("the requested processing step exists");
    TargetRef {
        resource: ResourceRef::from(dataset.resource_id),
        component: Some(ComponentRef::ProcessingStep(step.id)),
    }
}

pub(super) fn add_step(app: &mut PlotxApp, kind: StepKind) -> TargetRef {
    let Dataset::Nmr(dataset) = &mut app.doc.datasets[0] else {
        panic!("the fixture owns a 1D NMR dataset");
    };
    let id = dataset.allocate_step_id();
    dataset
        .pipeline
        .steps
        .push(ProcessingStep::new(id, kind, StepSource::User));
    TargetRef {
        resource: ResourceRef::from(dataset.resource_id),
        component: Some(ComponentRef::ProcessingStep(id)),
    }
}

pub(super) fn step<'a>(app: &'a PlotxApp, target: &TargetRef) -> &'a ProcessingStep {
    let Some(ComponentRef::ProcessingStep(id)) = target.component else {
        panic!("the target names a processing step");
    };
    let Dataset::Nmr(dataset) = &app.doc.datasets[0] else {
        panic!("the fixture owns a 1D NMR dataset");
    };
    dataset
        .pipeline
        .steps
        .iter()
        .find(|step| step.id == id)
        .expect("the stable step id resolves")
}

pub(super) fn step_mut<'a>(app: &'a mut PlotxApp, target: &TargetRef) -> &'a mut ProcessingStep {
    let Some(ComponentRef::ProcessingStep(id)) = target.component else {
        panic!("the target names a processing step");
    };
    let Dataset::Nmr(dataset) = &mut app.doc.datasets[0] else {
        panic!("the fixture owns a 1D NMR dataset");
    };
    dataset
        .pipeline
        .steps
        .iter_mut()
        .find(|step| step.id == id)
        .expect("the stable step id resolves")
}

pub(super) fn spectrum(app: &PlotxApp) -> (Vec<Complex64>, Vec<f64>) {
    let Dataset::Nmr(dataset) = &app.doc.datasets[0] else {
        panic!("the fixture owns a 1D NMR dataset");
    };
    (
        dataset.spectrum().unwrap().values.clone(),
        dataset.spectrum().unwrap().ppm.clone(),
    )
}
