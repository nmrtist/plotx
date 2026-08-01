use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str, function_records: &[(u8, u8, f32, f32)]) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "plotx-waters-{name}-{}-{serial}.raw",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("create fixture directory");
        std::fs::write(
            root.join("_HeAdEr.TxT"),
            b"$$ Instrument: Synthetic SQD2\n$$ Cal Function 1: 1,2,T0\n$$ Cal Function 2: ,T0\n",
        )
        .expect("write header");
        std::fs::write(
            root.join("_ExTeRn.InF"),
            b"Instrument Parameters - Function 1:\nPolarity ES+\n",
        )
        .expect("write extern");
        let mut table = vec![0; function_records.len() * FUNCTION_RECORD_SIZE];
        for (index, &(type_code, subtype, low, high)) in function_records.iter().enumerate() {
            let record = &mut table[index * FUNCTION_RECORD_SIZE..][..FUNCTION_RECORD_SIZE];
            record[0] = type_code;
            record[1] = subtype;
            record[160..164].copy_from_slice(&low.to_le_bytes());
            record[288..292].copy_from_slice(&high.to_le_bytes());
        }
        std::fs::write(root.join("_FuNcTnS.iNf"), table).expect("write function table");
        Self { root }
    }

    fn write_low_resolution_function(&self, number: u16, scans: &[(f32, Vec<Pair>)]) {
        let mut idx = Vec::with_capacity(scans.len() * IDX22_STRIDE);
        let mut dat = Vec::new();
        for (retention_time, pairs) in scans {
            let mut record = [0_u8; IDX22_STRIDE];
            let offset = u32::try_from(dat.len()).expect("fixture offset");
            record[0..4].copy_from_slice(&offset.to_le_bytes());
            let count = u32::try_from(pairs.len()).expect("fixture pair count") | 0x1800_0000;
            record[4..8].copy_from_slice(&count.to_le_bytes());
            record[12..16].copy_from_slice(&retention_time.to_le_bytes());
            idx.extend_from_slice(&record);
            for pair in pairs {
                dat.extend_from_slice(&pair.encode());
            }
        }
        let stem = format!("_FuNc{number:03}");
        std::fs::write(self.root.join(format!("{stem}.IdX")), idx).expect("write IDX");
        std::fs::write(self.root.join(format!("{stem}.DaT")), dat).expect("write DAT");
    }

    fn write_unsupported_function(&self, number: u16, pair_width: usize) {
        let mut idx = [0_u8; IDX22_STRIDE];
        idx[4..8].copy_from_slice(&1_u32.to_le_bytes());
        idx[12..16].copy_from_slice(&1.0_f32.to_le_bytes());
        std::fs::write(self.root.join(format!("_FUNC{number:03}.IDX")), idx).expect("write IDX");
        std::fs::write(
            self.root.join(format!("_FUNC{number:03}.DAT")),
            vec![0; pair_width],
        )
        .expect("write DAT");
    }

    fn write_missing_auxiliary_descriptor(&self, name: &str) {
        let mut info = vec![0_u8; 128 + 85];
        info[0..2].copy_from_slice(&128_u16.to_le_bytes());
        info[2..4].copy_from_slice(&1_u16.to_le_bytes());
        info[4..6].copy_from_slice(&85_u16.to_le_bytes());
        info[6..8].copy_from_slice(&1_u16.to_le_bytes());
        let descriptor = format!("{name},1,3,0,0,C");
        info[128..128 + descriptor.len()].copy_from_slice(descriptor.as_bytes());
        std::fs::write(self.root.join("_ChRoMs.InF"), info).expect("write CHROMS");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Copy)]
struct Pair {
    coordinate: u32,
    value: i16,
    value_exponent: u8,
}

impl Pair {
    const fn new(coordinate: u32, value: i16, value_exponent: u8) -> Self {
        Self {
            coordinate,
            value,
            value_exponent,
        }
    }

    fn encode(self) -> [u8; 6] {
        assert!(self.coordinate < (1 << 23));
        assert!(self.value_exponent < 16);
        let raw = (self.coordinate << 9) | (23 << 4) | u32::from(self.value_exponent);
        let value = self.value.to_le_bytes();
        let coordinate = raw.to_le_bytes();
        [
            value[0],
            value[1],
            coordinate[0],
            coordinate[1],
            coordinate[2],
            coordinate[3],
        ]
    }
}

fn loaded_run(result: &LoadResult) -> &MassSpecRun {
    let Acquisition::MassSpec(run) = &result.acquisition else {
        panic!("expected mass-spec acquisition")
    };
    run
}

#[test]
fn decodes_functions_by_metadata_and_builds_dynamic_optical_channels() {
    let fixture = Fixture::new(
        "decode",
        &[(0x00, 0x25, 5.0, 100.0), (0x0c, 0x24, 200.0, 300.0)],
    );
    fixture.write_low_resolution_function(
        2,
        &[
            (0.5, vec![Pair::new(214, -2, 0), Pair::new(254, 3, 0)]),
            (1.0, vec![Pair::new(214, 5, 0), Pair::new(254, -7, 0)]),
        ],
    );
    fixture.write_low_resolution_function(
        1,
        &[
            (
                0.25,
                vec![
                    Pair::new(10, 2, 1),
                    Pair::new(10, -1, 0),
                    Pair::new(20, 3, 0),
                ],
            ),
            (0.5, Vec::new()),
            (0.75, vec![Pair::new(30, -2, 0)]),
        ],
    );
    fixture.write_missing_auxiliary_descriptor("Sample Temp");

    assert!(is_masslynx_raw(&fixture.root));
    let result = load(&fixture.root).expect("load synthetic MassLynx bundle");
    let run = loaded_run(&result);
    assert_eq!(run.functions.len(), 2);
    assert_eq!(run.functions[0].id, FunctionId::new(1));
    assert_eq!(run.functions[0].kind, FunctionKind::MassSpectrum);
    assert_eq!(run.functions[0].polarity, Polarity::Positive);
    assert_eq!(run.functions[0].scans.len(), 3);
    assert_eq!(run.functions[0].scans[0].mz, [21.0, 21.0, 41.0]);
    assert_eq!(run.functions[0].scans[0].intensity, [8.0, -1.0, 3.0]);
    assert_eq!(run.functions[0].scans[0].tic, 10.0);
    assert!(run.functions[0].scans[1].mz.is_empty());
    assert_eq!(run.functions[1].kind, FunctionKind::OpticalDetector);
    assert_eq!(run.chromatograms.len(), 2);
    assert_eq!(run.chromatograms[0].coordinate, Some(214.0));
    assert_eq!(run.chromatograms[0].values, [-2.0, 5.0]);
    assert_eq!(run.chromatograms[1].coordinate, Some(254.0));
    assert_eq!(run.chromatograms[1].values, [3.0, -7.0]);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].message.contains("Sample Temp"));
    assert_eq!(result.provenance.selected_path, fixture.root);
    assert!(result.provenance.companion_paths.iter().any(|path| {
        path.file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("_FuNc002.DaT"))
    }));
}

#[test]
fn reports_unsupported_required_encoding_with_layout_context() {
    let fixture = Fixture::new("unsupported-ms", &[(0x00, 0x25, 5.0, 100.0)]);
    fixture.write_unsupported_function(1, 4);
    let error = load(&fixture.root).expect_err("four-byte MS pairs are unsupported");
    assert!(matches!(
        error,
        IoError::UnsupportedWatersEncoding {
            function_id,
            idx_stride: 22,
            pair_width: 4,
            ref instrument,
        } if function_id == FunctionId::new(1) && instrument == "Synthetic SQD2"
    ));
}

#[test]
fn preserves_unsupported_optional_function_as_a_warning() {
    let fixture = Fixture::new(
        "optional",
        &[(0x00, 0x25, 5.0, 100.0), (0x44, 0x00, 0.0, 0.0)],
    );
    fixture.write_low_resolution_function(1, &[(0.0, vec![Pair::new(10, 1, 0)])]);
    fixture.write_unsupported_function(2, 4);
    let result = load(&fixture.root).expect("optional function must not fail import");
    let run = loaded_run(&result);
    assert_eq!(run.functions[1].kind, FunctionKind::Unknown);
    assert!(run.functions[1].scans.is_empty());
    assert_eq!(run.functions[1].encoding.pair_width, 4);
    assert!(result.warnings[0].message.contains("function 2"));
}

#[test]
fn rejects_bad_offsets_and_malformed_required_calibration() {
    let fixture = Fixture::new("bad-offset", &[(0x00, 0x25, 5.0, 100.0)]);
    fixture.write_low_resolution_function(1, &[(0.0, vec![Pair::new(10, 1, 0)])]);
    let idx_path = fixture.root.join("_FuNc001.IdX");
    let mut idx = std::fs::read(&idx_path).expect("read IDX");
    idx[0..4].copy_from_slice(&1_u32.to_le_bytes());
    std::fs::write(&idx_path, idx).expect("rewrite IDX");
    assert!(
        load(&fixture.root)
            .expect_err("bad first offset")
            .to_string()
            .contains("not zero")
    );

    let fixture = Fixture::new("bad-calibration", &[(0x00, 0x25, 5.0, 100.0)]);
    fixture.write_low_resolution_function(1, &[(0.0, vec![Pair::new(10, 1, 0)])]);
    std::fs::write(
        fixture.root.join("_HeAdEr.TxT"),
        b"$$ Instrument: Synthetic SQD2\n$$ Cal Function 1: nope,T0\n",
    )
    .expect("rewrite header");
    assert!(
        load(&fixture.root)
            .expect_err("bad calibration")
            .to_string()
            .contains("invalid calibration coefficient")
    );
}

#[test]
fn rejects_invalid_idx_stride_missing_calibration_and_mismatched_function_table() {
    let fixture = Fixture::new("bad-stride", &[(0x00, 0x25, 5.0, 100.0)]);
    std::fs::write(fixture.root.join("_FUNC001.IDX"), vec![0; 21]).unwrap();
    std::fs::write(fixture.root.join("_FUNC001.DAT"), vec![0; 6]).unwrap();
    assert!(
        load(&fixture.root)
            .expect_err("invalid IDX stride")
            .to_string()
            .contains("matches no registered stride")
    );

    let fixture = Fixture::new("missing-calibration", &[(0x00, 0x25, 5.0, 100.0)]);
    fixture.write_low_resolution_function(1, &[(0.0, vec![Pair::new(10, 1, 0)])]);
    std::fs::write(
        fixture.root.join("_HeAdEr.TxT"),
        b"$$ Instrument: Synthetic SQD2\n$$ Cal Function 1: ,T0\n",
    )
    .unwrap();
    assert!(
        load(&fixture.root)
            .expect_err("required calibration is missing")
            .to_string()
            .contains("no valid calibration polynomial")
    );

    let fixture = Fixture::new(
        "function-mismatch",
        &[(0x00, 0x25, 5.0, 100.0), (0x44, 0x00, 0.0, 0.0)],
    );
    fixture.write_low_resolution_function(1, &[(0.0, vec![Pair::new(10, 1, 0)])]);
    assert!(
        load(&fixture.root)
            .expect_err("function files do not match records")
            .to_string()
            .contains("2 records but 1 numbered functions")
    );
}

#[test]
fn validates_local_acceptance_bundles_when_present() {
    let expected = [
        ("A2_10 mM_20260731.raw", 1_643_630_usize),
        ("PmST1_BIAO_20260728_240 MIN-3.raw", 968_073),
        ("PmST1_gaogong_20260728_240 MIN-3.raw", 935_742),
    ];
    let root = Path::new(r"C:\tmp\plotx-ms-demodata");
    for (name, expected_pairs) in expected {
        let path = root.join(name);
        if !path.is_dir() {
            continue;
        }
        let result = load(&path).expect("load local acceptance bundle");
        let run = loaded_run(&result);
        let ms = run
            .functions
            .iter()
            .find(|function| function.kind == FunctionKind::MassSpectrum)
            .expect("MS function");
        assert_eq!(ms.scans.len(), 596);
        assert_eq!(
            ms.scans.iter().map(|scan| scan.mz.len()).sum::<usize>(),
            expected_pairs
        );
        let coordinates = run
            .chromatograms
            .iter()
            .filter(|channel| channel.kind == ChromatogramKind::Optical)
            .filter_map(|channel| channel.coordinate)
            .collect::<Vec<_>>();
        assert_eq!(coordinates, [214.0, 254.0]);
    }
}
