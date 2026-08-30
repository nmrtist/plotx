use super::*;
use plotx_io::{ChromatogramProvenance, MassTransition, Polarity};

fn channel(
    id: &str,
    name: &str,
    kind: ChromatogramKind,
    polarity: Polarity,
    transition: Option<(f64, f64, Option<f64>, Option<&str>)>,
) -> ChromatogramChannel {
    ChromatogramChannel {
        id: ChromatogramChannelId(id.to_owned()),
        kind,
        provenance: ChromatogramProvenance::Source,
        polarity,
        transition: transition.map(
            |(precursor_mz, product_mz, collision_energy, activation_method)| MassTransition {
                precursor_mz: Some(precursor_mz),
                product_mz: Some(product_mz),
                collision_energy,
                activation_method: activation_method.map(str::to_owned),
            },
        ),
        source_stream: None,
        coordinate: None,
        description: name.to_owned(),
        unit: "cps".to_owned(),
        time_min: vec![0.0, 1.0],
        values: vec![1.0, 2.0],
    }
}

fn fixture_channels() -> Vec<ChromatogramChannel> {
    vec![
        channel(
            "chrom=beta native=22",
            "Beta transition",
            ChromatogramKind::SelectedReactionMonitoring,
            Polarity::Negative,
            Some((500.2, 200.1, Some(35.0), Some("HCD"))),
        ),
        channel(
            "chrom=alpha native=11",
            "Alpha transition",
            ChromatogramKind::SelectedReactionMonitoring,
            Polarity::Positive,
            Some((400.2, 100.1, Some(20.0), Some("CID"))),
        ),
        channel(
            "chrom=tic",
            "Total ion current",
            ChromatogramKind::TotalIonCurrent,
            Polarity::Unknown,
            None,
        ),
    ]
}

#[test]
fn free_text_matches_channel_name_and_native_id() {
    let mut state = BrowserState::new(ChannelIndex::from_channels(&fixture_channels()));
    state.filters.text = "beta".to_owned();
    assert!(state.refresh());
    assert_eq!(state.matches.len(), 1);
    assert_eq!(state.matches[0].id.0, "chrom=beta native=22");

    state.filters.text = "native=11".to_owned();
    assert!(state.refresh());
    assert_eq!(state.matches.len(), 1);
    assert_eq!(state.matches[0].name, "Alpha transition");
}

#[test]
fn transition_metadata_filters_compose_deterministically() {
    let mut state = BrowserState::new(ChannelIndex::from_channels(&fixture_channels()));
    state.filters.precursor_mz = "400..450".to_owned();
    state.filters.product_mz = "<=150".to_owned();
    state.filters.collision_energy = ">=20".to_owned();
    state.filters.polarity = PolarityFilter::Positive;
    state.filters.activation_method = "cid".to_owned();
    assert!(state.refresh());
    assert_eq!(state.matches.len(), 1);
    assert_eq!(state.matches[0].name, "Alpha transition");

    assert!(NumericPredicate::parse("not-a-number").is_err());
    assert_eq!(
        NumericPredicate::parse("500.2").unwrap(),
        Some(NumericPredicate::Equal(500.2))
    );
}

#[test]
fn channel_order_is_stable_and_transition_aware() {
    let index = ChannelIndex::from_channels(&fixture_channels());
    let names = index
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["Total ion current", "Alpha transition", "Beta transition"]
    );
}

#[test]
fn empty_and_metadata_free_runs_are_distinguished() {
    let empty = ChannelIndex::from_channels(&[]);
    assert!(empty.entries.is_empty());
    assert_eq!(empty.transition_count, 0);

    let channels = [channel(
        "chrom=uv",
        "UV 280 nm",
        ChromatogramKind::Optical,
        Polarity::Unknown,
        None,
    )];
    let metadata_free = ChannelIndex::from_channels(&channels);
    assert_eq!(metadata_free.entries.len(), 1);
    assert_eq!(metadata_free.transition_count, 0);
    let state = BrowserState::new(metadata_free);
    assert_eq!(state.matches.len(), 1);
}

#[test]
fn large_channel_cache_scans_only_when_filters_change() {
    let mut channels = vec![
        channel(
            "tic",
            "TIC",
            ChromatogramKind::TotalIonCurrent,
            Polarity::Unknown,
            None,
        ),
        channel(
            "bpc",
            "BPC",
            ChromatogramKind::BasePeak,
            Polarity::Unknown,
            None,
        ),
    ];
    channels.extend((0..720).map(|index| {
        channel(
            &format!("transition={index:03}"),
            &format!("MRM transition {index:03}"),
            ChromatogramKind::SelectedReactionMonitoring,
            if index % 2 == 0 {
                Polarity::Positive
            } else {
                Polarity::Negative
            },
            Some((
                400.0 + index as f64,
                100.0 + index as f64,
                Some(30.0),
                Some("CID"),
            )),
        )
    }));
    let mut state = BrowserState::new(ChannelIndex::from_channels(&channels));
    assert_eq!(state.index.entries.len(), 722);
    assert_eq!(state.index.transition_count, 720);
    assert_eq!(state.filter_scans, 1);
    assert!(!state.refresh());
    assert_eq!(state.filter_scans, 1);

    state.filters.text = "transition 719".to_owned();
    assert!(state.refresh());
    assert_eq!(state.filter_scans, 2);
    assert_eq!(state.matches.len(), 1);
    assert_eq!(state.matches[0].id.0, "transition=719");
}
