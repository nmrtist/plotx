use super::*;

#[test]
fn global_ids_round_trip_as_uuid_text() {
    let dataset = DatasetId::new();
    let canvas = CanvasId::new();

    assert_eq!(dataset.to_string().parse(), Ok(dataset));
    assert_eq!(canvas.to_string().parse(), Ok(canvas));
    assert_eq!(
        serde_json::to_string(&dataset).unwrap(),
        format!("\"{dataset}\"")
    );
}

#[test]
fn owner_local_ids_are_distinct_types() {
    let series = SeriesId::new(7);

    assert_eq!(series.get(), 7);
    assert_eq!(serde_json::to_string(&series).unwrap(), "7");
}
