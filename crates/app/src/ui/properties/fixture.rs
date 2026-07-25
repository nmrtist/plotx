//! Documents the property interface tests share.
//!
//! The interface half of the catalog is only testable against a document that
//! actually draws something, and more than one test needs the same one: a page
//! of contour plots whose settings can be made to agree or to differ.

use plotx_core::automation::TargetRef;
use plotx_core::properties::{PropertyValue, contour};
use plotx_core::state::{
    CanvasDocument, Dataset, Nmr2DDataset, ObjectFrame, ObjectId, PlotxApp, Selection,
};

fn nmr2d(source: &str) -> plotx_io::NmrData2D {
    let dimension = |nucleus: &str| plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    plotx_io::NmrData2D {
        data: (0..16)
            .map(|value| num_complex::Complex64::new(f64::from(value) - 7.0, 0.5))
            .collect(),
        rows: 4,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: source.to_owned(),
    }
}

/// One page holding `plots` contour plots of one 2D spectrum, all selected.
pub(crate) fn contour_page(plots: usize) -> (PlotxApp, Vec<ObjectId>) {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(nmr2d("panel")))));
    let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
    let mut ids = Vec::new();
    for index in 0..plots {
        let id = canvas.allocate_object_id();
        let object = app.build_plot_object(
            0,
            ObjectFrame::new(0.0, 60.0 * index as f32, 100.0, 50.0),
            id,
            format!("Plot {index}"),
        );
        canvas.objects.push(object);
        ids.push(id);
    }
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    app.set_selection(Selection::Objects(ids.clone()));
    (app, ids)
}

/// A second dataset, so a navigation test can tell whether the data focus
/// followed the object it landed on.
pub(crate) fn add_dataset(app: &mut PlotxApp) -> usize {
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(nmr2d(
            "second",
        )))));
    app.doc.datasets.len() - 1
}

/// Another page carrying `plots` plots of `dataset`. Object ids are allocated
/// per page, so the ids here start again at one — which is exactly why a
/// selection may not survive a page switch.
pub(crate) fn add_page(
    app: &mut PlotxApp,
    name: &str,
    dataset: usize,
    plots: usize,
) -> (usize, Vec<ObjectId>) {
    let mut canvas = CanvasDocument::new(name.to_owned(), [200.0, 200.0]);
    let mut ids = Vec::new();
    for index in 0..plots {
        let id = canvas.allocate_object_id();
        let object = app.build_plot_object(
            dataset,
            ObjectFrame::new(0.0, 60.0 * index as f32, 100.0, 50.0),
            id,
            format!("{name} {index}"),
        );
        canvas.objects.push(object);
        ids.push(id);
    }
    app.doc.canvases.push(canvas);
    (app.doc.canvases.len() - 1, ids)
}

/// Redraw one plot's series as a heatmap: a target that exists, resolves, and
/// carries no contour setting.
pub(crate) fn draw_as_heatmap(app: &mut PlotxApp, object: ObjectId) {
    if let Some(series) = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .and_then(|plot| plot.binding.series.first_mut())
    {
        series.encoding =
            plotx_figure::SeriesEncoding::Heatmap(plotx_figure::HeatmapSpec::default());
    }
}

pub(crate) fn targets_of(app: &PlotxApp, object: ObjectId) -> Vec<TargetRef> {
    app.series_targets(0, object)
}

/// Move one plot's lowest level through the catalog, so the disagreement the
/// panel then reports was produced the way a user produces it.
pub(crate) fn set_lowest_level(app: &mut PlotxApp, object: ObjectId, multiplier: f64) {
    let targets = targets_of(app, object);
    let commit = app
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            &targets,
            &PropertyValue::Float(multiplier),
        )
        .expect("the fixture writes a valid multiplier");
    app.commit_property(commit);
}
