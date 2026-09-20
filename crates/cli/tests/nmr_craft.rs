use std::{
    f64::consts::{PI, TAU},
    process::Command,
};

#[test]
fn craft_reads_with_nmr_and_preserves_the_independent_tone_frequencies() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("acquisition");
    std::fs::create_dir(&input).unwrap();
    let parameters = "##TITLE=PlotX synthetic CRAFT acceptance\n##$TD=8192\n##$PARMODE=0\n##$AQ_mod=3\n##$BYTORDA=0\n##$DTYPA=0\n##$SW_h=2000\n##$SFO1=500.005\n##$BF1=500\n##$O1=5000\n##$NUC1=<1H>\n##$GRPDLY=0\n##END=\n";
    std::fs::write(input.join("acqus"), parameters).unwrap();
    let bytes = (0..4096)
        .flat_map(|index| {
            let time = index as f64 / 2000.0;
            let value = [(-75.0, 800_000.0, 0.3, 1.5), (120.0, 400_000.0, -0.2, 2.0)]
                .into_iter()
                .fold(
                    nmr::Complex64::new(0., 0.),
                    |sum, (frequency, amplitude, phase, width)| {
                        sum + nmr::Complex64::from_polar(
                            amplitude * (-PI * width * time).exp(),
                            phase + TAU * frequency * time,
                        )
                    },
                );
            [value.re, value.im]
                .into_iter()
                .flat_map(|v| (v.round() as i32).to_le_bytes())
        })
        .collect::<Vec<_>>();
    std::fs::write(input.join("fid"), bytes).unwrap();
    let output = dir.path().join("result.json");
    let result = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("craft")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert!(
        result.status.success(),
        "{report}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let item = &report["datasets"][0];
    assert_eq!(item["inspection"]["format"], "bruker-raw");
    assert_eq!(item["acquisition"]["observe_frequency_mhz"], 500.005);
    assert_eq!(
        item["chemical_shift_reference"]["reference_frequency_mhz"],
        500.0
    );
    let components = item["components"].as_array().unwrap();
    for component in components {
        let frequency = component["frequency_hz"].as_f64().unwrap();
        let ppm = component["chemical_shift_ppm"].as_f64().unwrap();
        assert!((ppm - (10.0 + frequency / 500.0)).abs() < 1e-12);
    }
    for expected in [-75.0, 120.0] {
        assert!(
            components
                .iter()
                .any(
                    |component| (component["frequency_hz"].as_f64().unwrap() - expected).abs()
                        < 0.05
                ),
            "{components:?}"
        );
    }

    std::fs::write(input.join("acqus"), parameters.replace("##$GRPDLY=0\n", "")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("craft")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(report["datasets"][0]["status"], "failed");
    assert!(
        report["datasets"][0]["error"]
            .as_str()
            .unwrap()
            .contains("delay evidence")
    );
}
