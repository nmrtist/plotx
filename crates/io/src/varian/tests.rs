use super::*;
use num_complex::Complex64;
use std::sync::atomic::{AtomicU64, Ordering};

fn record(name: &str, basic: i32, values: &str) -> String {
    format!("{name} 1 {basic}\n{values}\n0\n")
}

fn base_procpar() -> String {
    [
        record("np", 1, "1 4"),
        record("sw", 1, "1 4000"),
        record("sfrq", 1, "1 500"),
        record("tof", 1, "1 2500"),
        record("tn", 2, "1 \"H1\""),
        record("array", 2, "1 \"\""),
    ]
    .concat()
}

#[derive(Clone, Copy)]
enum Encoding {
    I16,
    I32,
    F32,
}

fn fid_bytes(blocks: &[Vec<Vec<f64>>], encoding: Encoding, scales: &[i16]) -> Vec<u8> {
    let nblocks = blocks.len();
    let ntraces = blocks[0].len();
    let np = blocks[0][0].len();
    let ebytes = match encoding {
        Encoding::I16 => 2,
        Encoding::I32 | Encoding::F32 => 4,
    };
    let tbytes = np * ebytes;
    let bbytes = 28 + ntraces * tbytes;
    let status = 0x11
        | match encoding {
            Encoding::I16 => 0,
            Encoding::I32 => 0x4,
            Encoding::F32 => 0xc,
        };
    let mut out = Vec::new();
    for value in [nblocks, ntraces, np, ebytes, tbytes, bbytes] {
        out.extend_from_slice(&(value as i32).to_be_bytes());
    }
    out.extend_from_slice(&0_i16.to_be_bytes());
    out.extend_from_slice(&(status as i16).to_be_bytes());
    out.extend_from_slice(&1_i32.to_be_bytes());
    for (block, &scale) in blocks.iter().zip(scales) {
        out.extend_from_slice(&scale.to_be_bytes());
        out.extend_from_slice(&(status as i16).to_be_bytes());
        out.extend_from_slice(&[0; 24]);
        for trace in block {
            for value in trace {
                match encoding {
                    Encoding::I16 => out.extend_from_slice(&(*value as i16).to_be_bytes()),
                    Encoding::I32 => out.extend_from_slice(&(*value as i32).to_be_bytes()),
                    Encoding::F32 => out.extend_from_slice(&(*value as f32).to_be_bytes()),
                }
            }
        }
    }
    out
}

fn dataset(procpar: &str, fid: &[u8]) -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "plotx_varian_{}_{}.fid",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("procpar"), procpar).unwrap();
    std::fs::write(dir.join("fid"), fid).unwrap();
    dir
}

#[test]
fn loads_directory_and_fid_with_provenance_and_metadata() {
    let mut procpar = base_procpar();
    procpar.push_str(&record("samplename", 2, "1 \"Test sample\""));
    procpar.push_str(&record("pslabel", 2, "1 \"PROTON\""));
    let dir = dataset(
        &procpar,
        &fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::I16, &[1]),
    );
    assert_eq!(
        crate::detect_format(&dir).unwrap(),
        DataFormat::VarianAgilentRaw
    );
    for selected in [&dir, &dir.join("fid")] {
        let loaded = load_raw(selected).unwrap();
        assert_eq!(loaded.provenance.selected_path, *selected);
        assert_eq!(loaded.provenance.data_path, dir.join("fid"));
        assert_eq!(loaded.provenance.parameter_paths, vec![dir.join("procpar")]);
        let Acquisition::D1(data) = loaded.acquisition else {
            panic!("expected 1D")
        };
        assert_eq!(
            data.points,
            vec![Complex64::new(2., 4.), Complex64::new(6., 8.)]
        );
        assert_eq!(
            (
                data.spectral_width_hz,
                data.observe_freq_mhz,
                data.carrier_ppm
            ),
            (4000., 500., 5.)
        );
        assert_eq!(data.nucleus, "1H");
        assert_eq!(data.source, "Test sample — 1H — PROTON");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reads_all_sample_widths_and_block_major_trace_minor_order() {
    for encoding in [Encoding::I16, Encoding::I32, Encoding::F32] {
        let bytes = fid_bytes(
            &[
                vec![vec![1., 2.], vec![3., 4.]],
                vec![vec![5., 6.], vec![7., 8.]],
            ],
            encoding,
            &[0, 1],
        );
        let raw = fid::parse(&bytes).unwrap();
        assert_eq!(
            raw.traces.iter().flatten().copied().collect::<Vec<_>>(),
            vec![
                Complex64::new(1., 2.),
                Complex64::new(3., 4.),
                Complex64::new(10., 12.),
                Complex64::new(14., 16.)
            ]
        );
    }
}

#[test]
fn loads_homonuclear_and_heteronuclear_states_2d() {
    let raw = fid_bytes(
        &[
            vec![vec![1., 2., 3., 4.], vec![5., 6., 7., 8.]],
            vec![vec![9., 10., 11., 12.], vec![13., 14., 15., 16.]],
        ],
        Encoding::I32,
        &[0, 0],
    );
    for (seq, indirect) in [("gcosy", (500., 5., "1H")), ("ghsqc", (125., 80., "13C"))] {
        let mut p = base_procpar();
        p.push_str(&record("ni", 1, "1 2"));
        p.push_str(&record("phase", 1, "2 1 2"));
        p.push_str(&record("array", 2, "1 \"phase\""));
        p.push_str(&record("sw1", 1, "1 20000"));
        p.push_str(&record("seqfil", 2, &format!("1 \"{seq}\"")));
        if seq == "ghsqc" {
            p.push_str(&record("dfrq", 1, "1 125"));
            p.push_str(&record("dof", 1, "1 10000"));
            p.push_str(&record("dn", 2, "1 \"C13\""));
        }
        let dir = dataset(&p, &raw);
        let Acquisition::D2(data) = load_raw(&dir).unwrap().acquisition else {
            panic!("expected 2D")
        };
        assert_eq!((data.rows, data.cols, data.quad), (4, 2, QuadMode::States));
        assert_eq!(
            (
                data.indirect.observe_freq_mhz,
                data.indirect.carrier_ppm,
                data.indirect.nucleus.as_str()
            ),
            indirect
        );
        assert_eq!(data.experiment.as_deref(), Some(seq));
        assert!(!data.indirect_conjugate);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn rejects_unsupported_two_entry_phase_table() {
    let raw = fid_bytes(
        &[
            vec![vec![1., 2., 3., 4.], vec![5., 6., 7., 8.]],
            vec![vec![9., 10., 11., 12.], vec![13., 14., 15., 16.]],
        ],
        Encoding::I32,
        &[0, 0],
    );
    let mut p = base_procpar();
    p.push_str(&record("ni", 1, "1 2"));
    p.push_str(&record("phase", 1, "2 1 3"));
    p.push_str(&record("array", 2, "1 \"phase\""));
    let dir = dataset(&p, &raw);

    assert!(matches!(load_raw(&dir), Err(IoError::UnsupportedVarian(_))));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_procpar_np_disagreement() {
    let mut p = base_procpar();
    p.push_str(&record("np", 1, "1 6"));
    let dir = dataset(
        &p,
        &fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::I16, &[0]),
    );

    assert!(matches!(load_raw(&dir), Err(IoError::InvalidVarian(_))));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_corrupt_and_unsupported_layouts() {
    let mut odd = fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::I16, &[0]);
    odd[8..12].copy_from_slice(&3_i32.to_be_bytes());
    assert!(matches!(fid::parse(&odd), Err(IoError::InvalidVarian(_))));
    let mut truncated = fid_bytes(&[vec![vec![1., 2.]]], Encoding::I16, &[0]);
    truncated.pop();
    assert!(matches!(
        fid::parse(&truncated),
        Err(IoError::Truncated { .. })
    ));
    let mut spectrum = fid_bytes(&[vec![vec![1., 2.]]], Encoding::I16, &[0]);
    spectrum[26..28].copy_from_slice(&0x13_i16.to_be_bytes());
    assert!(matches!(
        fid::parse(&spectrum),
        Err(IoError::UnsupportedVarian(_))
    ));
}

#[test]
fn accepts_ddr_fid_without_legacy_complex_bit() {
    let mut ddr = fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::F32, &[0]);
    let status = 0x00c9_i16;
    ddr[26..28].copy_from_slice(&status.to_be_bytes());
    ddr[34..36].copy_from_slice(&status.to_be_bytes());

    let raw = fid::parse(&ddr).unwrap();
    assert_eq!(
        raw.traces[0],
        vec![Complex64::new(1., 2.), Complex64::new(3., 4.)]
    );
}

#[test]
fn rejects_processed_and_higher_dimensional_header_flags() {
    let base = fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::F32, &[0]);
    for status_bit in [0x2_i16, 0x100, 0x200, 0x400] {
        let mut bytes = base.clone();
        let status = i16::from_be_bytes(bytes[26..28].try_into().unwrap()) | status_bit;
        bytes[26..28].copy_from_slice(&status.to_be_bytes());
        assert!(matches!(
            fid::parse(&bytes),
            Err(IoError::UnsupportedVarian(_))
        ));
    }

    let mut ni3 = base.clone();
    ni3[28..32].copy_from_slice(&0x10001_i32.to_be_bytes());
    assert!(matches!(
        fid::parse(&ni3),
        Err(IoError::UnsupportedVarian(_))
    ));
}

#[test]
fn rejects_block_sample_type_disagreement() {
    let mut bytes = fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::F32, &[0]);
    let integer = fid_bytes(&[vec![vec![1., 2., 3., 4.]]], Encoding::I32, &[0]);
    bytes[34..36].copy_from_slice(&integer[34..36]);
    assert!(matches!(fid::parse(&bytes), Err(IoError::InvalidVarian(_))));
}
