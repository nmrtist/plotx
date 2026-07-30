use super::*;
use crate::state::{Peak2DOrigin, Peak2DPoint, Peak2DReview};

#[test]
fn project_roundtrip_preserves_cross_peak_pairs_and_review_state() {
    let mut app = PlotxApp::new();
    let mut dataset = Nmr2DDataset::load(super::tests::synthetic_true_2d());
    let ids = dataset
        .peaks
        .add_pair(
            Peak2DPoint {
                f2: 7.1,
                f1: 3.2,
                intensity: 12.0,
            },
            Peak2DPoint {
                f2: 3.2,
                f1: 7.1,
                intensity: 9.0,
            },
            [0.01, 0.01],
            Peak2DOrigin::Manual,
        )
        .unwrap();
    dataset.peaks.set_review(ids[0], Peak2DReview::Confirmed);
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));

    let path = super::tests::temp_project("symmetry-peaks");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let restored = loaded.doc.datasets[0].as_nmr2d().unwrap();
    assert_eq!(restored.peaks.marks.len(), 2);
    assert_eq!(restored.peaks.mark(ids[0]).unwrap().partner, Some(ids[1]));
    assert_eq!(restored.peaks.mark(ids[1]).unwrap().partner, Some(ids[0]));
    assert_eq!(
        restored.peaks.mark(ids[0]).unwrap().review,
        Peak2DReview::Confirmed
    );
    assert!(restored.peaks.validate().is_ok());
}
