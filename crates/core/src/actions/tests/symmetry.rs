use super::*;
use crate::state::{Nmr2DDataset, Peak2DOrigin, Peak2DPoint, Peak2DReview, Peak2DSet};

#[test]
fn cross_peak_pair_is_one_undoable_edit() {
    let mut data = synthetic_2d();
    data.indirect = data.direct.clone();
    data.experiment = Some("cosy".to_owned());

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data))));
    let dataset_id = app.doc.datasets[0].resource_id();
    let before = Peak2DSet::default();
    let mut after = before.clone();
    let ids = after
        .add_pair(
            Peak2DPoint {
                f2: 7.0,
                f1: 3.0,
                intensity: 10.0,
            },
            Peak2DPoint {
                f2: 3.0,
                f1: 7.0,
                intensity: 8.0,
            },
            [0.01, 0.01],
            Peak2DOrigin::Manual,
        )
        .unwrap();
    after.set_review(ids[0], Peak2DReview::Confirmed);

    app.execute_action(Action::set_peaks_2d(
        dataset_id,
        before.clone(),
        after.clone(),
    ));
    assert_eq!(app.doc.datasets[0].as_nmr2d().unwrap().peaks, after);

    app.undo();
    assert_eq!(app.doc.datasets[0].as_nmr2d().unwrap().peaks, before);

    app.redo();
    assert_eq!(app.doc.datasets[0].as_nmr2d().unwrap().peaks, after);
}
