use super::tests::synthetic_1d;
use super::*;
use crate::state::{MultipletPatternKind, StoredJValue, StoredMultiplet};

pub(super) fn sample_multiplet() -> StoredMultiplet {
    StoredMultiplet {
        id: 3,
        lo: 2.30,
        hi: 2.41,
        center_ppm: 2.354,
        pattern: MultipletPatternKind::DoubletOfDoublets,
        j_values: vec![
            StoredJValue {
                hz: 12.0,
                sigma_hz: Some(0.2),
            },
            StoredJValue {
                hz: 4.0,
                sigma_hz: None,
            },
        ],
        area: 5.5,
        peak_ppm: vec![2.41, 2.37, 2.34, 2.30],
    }
}

#[test]
fn multiplets_survive_project_roundtrip() {
    let mut dataset = NmrDataset::load(synthetic_1d());
    dataset.multiplets.push(sample_multiplet());
    dataset.next_multiplet_id = 4;
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let path = super::tests::temp_project("multiplets");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let n = loaded.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(n.multiplets, vec![sample_multiplet()]);
    assert_eq!(n.next_multiplet_id, 4);
}

#[test]
fn recipe_without_multiplets_key_loads_with_empty() {
    let mut dataset = NmrDataset::load(synthetic_1d());
    dataset.multiplets.push(sample_multiplet());
    dataset.next_multiplet_id = 4;
    let recipe = RecipeObject {
        id: "recipe_000000".to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("nmr".to_owned()),
            object: "recipe".to_owned(),
        },
        input: "data_000000".to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::json!({
            "plotx.analysis": { "peaks": crate::state::PeakSet::default(), "integrals": [] }
        }),
    };

    apply_1d_recipe(&mut dataset, &recipe);

    assert!(dataset.multiplets.is_empty());
    assert_eq!(dataset.next_multiplet_id, 0);
}
