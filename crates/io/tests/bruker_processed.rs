use plotx_io::{Acquisition, DataFormat, Domain, LoadWarningCode};

fn fixture(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "plotx_{name}_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}

#[test]
fn loads_big_endian_scaled_1r_from_experiment_directory() {
    let root = fixture("bruker_processed_1d");
    let experiment = root.join("sample").join("3");
    let proc_dir = experiment.join("pdata").join("1");
    std::fs::create_dir_all(&proc_dir).unwrap();
    std::fs::write(experiment.join("acqus"), "##$TD= 8\n").unwrap();
    std::fs::write(
        proc_dir.join("procs"),
        "##$SI= 4\n##$DTYPP= 0\n##$BYTORDP= 1\n##$NC_proc= 1\n\
         ##$SW_p= 4000\n##$SF= 400\n##$OFFSET= 10\n##$AXNUC= <1H>\n",
    )
    .unwrap();
    let bytes: Vec<u8> = [1i32, 2, 3, 4]
        .into_iter()
        .flat_map(i32::to_be_bytes)
        .collect();
    std::fs::write(proc_dir.join("1r"), bytes).unwrap();

    assert_eq!(
        plotx_io::detect_format(&experiment).unwrap(),
        DataFormat::BrukerProcessed1D
    );
    let loaded = plotx_io::load_path(&experiment).unwrap();
    assert_eq!(loaded.format, DataFormat::BrukerProcessed1D);
    assert!(
        loaded
            .provenance
            .parameter_paths
            .contains(&experiment.join("acqus"))
    );
    assert!(
        loaded
            .warnings
            .iter()
            .any(|warning| { warning.code == LoadWarningCode::OptionalImaginaryMissing })
    );
    let data = match loaded.acquisition {
        Acquisition::D1(data) => data,
        Acquisition::D2(_) => panic!("expected 1D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!(data.domain, Domain::Frequency);
    assert_eq!(
        data.points.iter().map(|value| value.re).collect::<Vec<_>>(),
        vec![8.0, 6.0, 4.0, 2.0]
    );
    assert_eq!(data.carrier_ppm, 5.0);
    assert_eq!(data.nucleus, "1H");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn loads_2rr_and_reverses_both_frequency_axes() {
    let root = fixture("bruker_processed_2d");
    let proc_dir = root.join("sample").join("7").join("pdata").join("2");
    std::fs::create_dir_all(&proc_dir).unwrap();
    std::fs::write(
        proc_dir.join("procs"),
        "##$SI= 3\n##$DTYPP= 0\n##$BYTORDP= 0\n##$NC_proc= 0\n\
         ##$SW_p= 3000\n##$SF= 600\n##$OFFSET= 9\n##$AXNUC= <1H>\n",
    )
    .unwrap();
    std::fs::write(
        proc_dir.join("proc2s"),
        "##$SI= 2\n##$SW_p= 2000\n##$SF= 100\n##$OFFSET= 120\n##$AXNUC= <13C>\n",
    )
    .unwrap();
    let bytes: Vec<u8> = [1i32, 2, 3, 4, 5, 6]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect();
    std::fs::write(proc_dir.join("2rr"), bytes).unwrap();

    let loaded = plotx_io::load_path(&proc_dir).unwrap();
    assert_eq!(loaded.format, DataFormat::BrukerProcessed2D);
    let data = match loaded.acquisition {
        Acquisition::D2(data) => *data,
        Acquisition::D1(_) => panic!("expected 2D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!(data.domain, Domain::Frequency);
    assert_eq!((data.rows, data.cols), (2, 3));
    assert_eq!(
        data.data.iter().map(|value| value.re).collect::<Vec<_>>(),
        vec![6.0, 5.0, 4.0, 3.0, 2.0, 1.0]
    );
    assert_eq!(data.direct.carrier_ppm, 6.5);
    assert_eq!(data.indirect.carrier_ppm, 110.0);

    std::fs::remove_dir_all(root).unwrap();
}
