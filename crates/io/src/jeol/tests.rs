use super::*;
use crate::AxisSource;

#[test]
fn prefix_exponent_ladder() {
    assert_eq!(prefix_exponent(0x01), 0); // none
    assert_eq!(prefix_exponent(0x11), -3); // milli
    assert_eq!(prefix_exponent(0x21), -6); // micro
    assert_eq!(prefix_exponent(0x31), -9); // nano
    assert_eq!(prefix_exponent(0xF1), 3); // kilo
    assert_eq!(prefix_exponent(0xE1), 6); // mega
}

#[test]
fn scans_embedded_list_ruler() {
    let text = b"comment_7 => \"*** Pulse Delay ***\";\n    tau_interval          \
        => y_acq {1[ms], 1.7644[ms], 3.11312[ms], 5[s]}, help \"arrayed list\";";
    let (name, values, unit, source) = scan_embedded_axis(text).expect("axis");
    assert_eq!(name, "tau_interval");
    assert_eq!(unit, "ms");
    assert_eq!(source, AxisSource::EmbeddedList);
    assert!((values[0] - 0.001).abs() < 1e-12);
    assert!((values[1] - 0.0017644).abs() < 1e-12);
    assert!((values[3] - 5.0).abs() < 1e-12); // 5[s] converted from seconds
    assert_eq!(kind_for_unit(&unit), PseudoKind::Delay);
}

#[test]
fn scans_embedded_ramp_ruler() {
    let text =
        b"    g                        => y_acq 20[mT/m]..0.28[T/m] : 17.33333[mT/m], help \"g\";";
    let (name, values, unit, source) = scan_embedded_axis(text).expect("axis");
    assert_eq!(name, "g");
    assert_eq!(source, AxisSource::EmbeddedRamp);
    assert_eq!(kind_for_unit(&unit), PseudoKind::Gradient);
    assert_eq!(values.len(), 16);
    assert!((values[0] - 0.02).abs() < 1e-9);
    assert!((values[15] - 0.28).abs() < 1e-6);
}

fn params_with(strings: &[(&str, &str)]) -> Params {
    let mut p = Params::empty();
    for (k, v) in strings {
        p.strings.insert((*k).to_string(), (*v).to_string());
    }
    p
}

#[test]
fn group_delay_from_fir_cascade() {
    // orders = "<stage count> <taps...>", factors = per-stage decimation.
    // Delay (final points) = Σ (taps_k-1)/2 · D_{k-1} / D_total.
    // "6 2" / "41 74": (20·1 + 36.5·6)/12 = 19.9166…
    let g = group_delay(&params_with(&[
        ("DIGITAL_FILTER", "TRUE"),
        ("orders", "2 41 74"),
        ("factors", "6  2"),
    ]));
    assert!((g - 239.0 / 12.0).abs() < 1e-9, "got {g}");

    // "2 2" / "15 73": (7·1 + 36·2)/4 = 19.75.
    let g = group_delay(&params_with(&[
        ("DIGITAL_FILTER", "TRUE"),
        ("orders", "2 15 73"),
        ("factors", "2  2"),
    ]));
    assert!((g - 19.75).abs() < 1e-9, "got {g}");
}

#[test]
fn group_delay_gated_and_guarded() {
    // Filter off → no correction even with orders/factors present.
    let g = group_delay(&params_with(&[
        ("DIGITAL_FILTER", "FALSE"),
        ("orders", "2 41 74"),
        ("factors", "6  2"),
    ]));
    assert_eq!(g, 0.0);

    // Filter flag absent → no correction.
    assert_eq!(group_delay(&params_with(&[("orders", "2 41 74")])), 0.0);

    // Filter on but parameters missing → no correction, no panic.
    assert_eq!(
        group_delay(&params_with(&[("DIGITAL_FILTER", "TRUE")])),
        0.0
    );
}

#[test]
fn rejects_bad_magic() {
    let buf = vec![0u8; HEADER_LEN + 16];
    let err = read_jdf_bytes(&buf, "x".into()).unwrap_err();
    assert!(matches!(err, IoError::BadMagic));
}

#[test]
fn rejects_truncated_header() {
    let buf = vec![0u8; 100];
    let err = read_jdf_bytes(&buf, "x".into()).unwrap_err();
    assert!(matches!(err, IoError::Truncated { .. }));
}

#[test]
fn round_trips_a_hand_built_1d_le_file() {
    let npoints = 4usize;
    let rec_size = 64usize;
    let param_hdr = HEADER_LEN;
    let param_recs = param_hdr + 16;
    let data_start = param_recs + rec_size;
    let data_len = npoints * 8 * 2;
    let mut buf = vec![0u8; data_start + data_len];

    buf[..8].copy_from_slice(MAGIC);
    buf[off::ENDIAN] = 1; // little-endian body
    buf[off::DATA_DIMENSION_NUMBER] = 1;
    buf[off::DATA_AXIS_TYPE] = AXIS_COMPLEX;
    buf[off::DATA_POINTS..off::DATA_POINTS + 4].copy_from_slice(&(npoints as u32).to_be_bytes());
    buf[off::BASE_FREQ..off::BASE_FREQ + 8].copy_from_slice(&600.17f64.to_be_bytes());
    // SW = (npoints-1)/acq = 1000 Hz.
    let acq = (npoints as f64 - 1.0) / 1000.0;
    buf[off::DATA_AXIS_START..off::DATA_AXIS_START + 8].copy_from_slice(&0.0f64.to_be_bytes());
    buf[off::DATA_AXIS_STOP..off::DATA_AXIS_STOP + 8].copy_from_slice(&acq.to_be_bytes());
    buf[off::DATA_START..off::DATA_START + 4].copy_from_slice(&(data_start as u32).to_be_bytes());
    buf[off::DATA_LENGTH..off::DATA_LENGTH + 8].copy_from_slice(&(data_len as u64).to_be_bytes());

    buf[param_hdr..param_hdr + 4].copy_from_slice(&(rec_size as u32).to_le_bytes());
    buf[param_hdr + 8..param_hdr + 12].copy_from_slice(&0u32.to_le_bytes());
    buf[param_recs + 0x10..param_recs + 0x18].copy_from_slice(&4.7f64.to_le_bytes());
    buf[param_recs + 0x20..param_recs + 0x24].copy_from_slice(&2u32.to_le_bytes());
    let name = b"X_OFFSET";
    buf[param_recs + 0x24..param_recs + 0x24 + name.len()].copy_from_slice(name);

    for i in 0..npoints {
        let ro = data_start + i * 8;
        let io = data_start + npoints * 8 + i * 8;
        buf[ro..ro + 8].copy_from_slice(&((i as f64) + 1.0).to_le_bytes());
        buf[io..io + 8].copy_from_slice(&((i as f64) + 5.0).to_le_bytes());
    }

    let data = match read_jdf_bytes(&buf, "test.jdf".into()).unwrap() {
        Acquisition::D1(d) => d,
        Acquisition::D2(_) => panic!("expected 1D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!(data.len(), 4);
    // FID conjugated on read (imaginary channel negated).
    assert_eq!(data.points[0], Complex64::new(1.0, -5.0));
    assert_eq!(data.points[3], Complex64::new(4.0, -8.0));
    assert!((data.observe_freq_mhz - 600.17).abs() < 1e-6);
    assert!((data.spectral_width_hz - 1000.0).abs() < 1e-6);
    assert!(
        (data.carrier_ppm - 4.7).abs() < 1e-9,
        "carrier from X_OFFSET"
    );
    assert_eq!(data.nucleus, "1H");
}

#[test]
fn uses_real_point_count_over_padded_count_for_1d() {
    // 8 padded points, only 4 real (DATA_OFFSET_STOP = 3). The FID must be
    // truncated to 4 and the sweep width computed from the real count, not the
    // padded one — otherwise every ppm is scaled by (8-1)/(4-1).
    let npad = 8usize;
    let nreal = 4usize;
    let rec_size = 64usize;
    let param_hdr = HEADER_LEN;
    let param_recs = param_hdr + 16;
    let data_start = param_recs + rec_size;
    let data_len = npad * 8 * 2; // padded reals then padded imags, f64
    let mut buf = vec![0u8; data_start + data_len];

    buf[..8].copy_from_slice(MAGIC);
    buf[off::ENDIAN] = 1;
    buf[off::DATA_DIMENSION_NUMBER] = 1;
    buf[off::DATA_AXIS_TYPE] = AXIS_COMPLEX;
    buf[off::DATA_POINTS..off::DATA_POINTS + 4].copy_from_slice(&(npad as u32).to_be_bytes());
    buf[off::DATA_OFFSET_STOP..off::DATA_OFFSET_STOP + 4]
        .copy_from_slice(&((nreal - 1) as u32).to_be_bytes());
    buf[off::BASE_FREQ..off::BASE_FREQ + 8].copy_from_slice(&600.0f64.to_be_bytes());
    // acq time over the real count → SW = (nreal-1)/acq = 1000 Hz.
    let acq = (nreal as f64 - 1.0) / 1000.0;
    buf[off::DATA_AXIS_START..off::DATA_AXIS_START + 8].copy_from_slice(&0.0f64.to_be_bytes());
    buf[off::DATA_AXIS_STOP..off::DATA_AXIS_STOP + 8].copy_from_slice(&acq.to_be_bytes());
    buf[off::DATA_START..off::DATA_START + 4].copy_from_slice(&(data_start as u32).to_be_bytes());
    buf[off::DATA_LENGTH..off::DATA_LENGTH + 8].copy_from_slice(&(data_len as u64).to_be_bytes());

    buf[param_hdr..param_hdr + 4].copy_from_slice(&(rec_size as u32).to_le_bytes());
    buf[param_hdr + 8..param_hdr + 12].copy_from_slice(&0u32.to_le_bytes());

    for i in 0..npad {
        // Real channel: 1..=4 real, then padding sentinels that must be dropped.
        let ro = data_start + i * 8;
        let io = data_start + npad * 8 + i * 8;
        let re = if i < nreal { i as f64 + 1.0 } else { 999.0 };
        let im = if i < nreal { i as f64 + 5.0 } else { -999.0 };
        buf[ro..ro + 8].copy_from_slice(&re.to_le_bytes());
        buf[io..io + 8].copy_from_slice(&im.to_le_bytes());
    }

    let data = match read_jdf_bytes(&buf, "padded.jdf".into()).unwrap() {
        Acquisition::D1(d) => d,
        Acquisition::D2(_) => panic!("expected 1D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!(data.len(), nreal, "FID truncated to the real point count");
    assert_eq!(data.points[0], Complex64::new(1.0, -5.0));
    assert_eq!(data.points[3], Complex64::new(4.0, -8.0));
    assert!(
        (data.spectral_width_hz - 1000.0).abs() < 1e-6,
        "sweep width uses the real count, got {}",
        data.spectral_width_hz
    );
}

#[test]
fn rejects_ambiguous_sample_width() {
    // A data section that is neither 4× nor 8× the sample count must error rather
    // than silently pick a width and splice unrelated samples into garbage.
    let npoints = 4usize;
    let rec_size = 64usize;
    let param_hdr = HEADER_LEN;
    let param_recs = param_hdr + 16;
    let data_start = param_recs + rec_size;
    // components = 2 → total 8 samples; f32 wants 32 bytes, f64 wants 64. Give 48.
    let data_len = 48usize;
    let mut buf = vec![0u8; data_start + data_len];

    buf[..8].copy_from_slice(MAGIC);
    buf[off::ENDIAN] = 1;
    buf[off::DATA_DIMENSION_NUMBER] = 1;
    buf[off::DATA_AXIS_TYPE] = AXIS_COMPLEX;
    buf[off::DATA_POINTS..off::DATA_POINTS + 4].copy_from_slice(&(npoints as u32).to_be_bytes());
    buf[off::DATA_START..off::DATA_START + 4].copy_from_slice(&(data_start as u32).to_be_bytes());
    buf[off::DATA_LENGTH..off::DATA_LENGTH + 8].copy_from_slice(&(data_len as u64).to_be_bytes());
    buf[param_hdr..param_hdr + 4].copy_from_slice(&(rec_size as u32).to_le_bytes());
    buf[param_hdr + 8..param_hdr + 12].copy_from_slice(&0u32.to_le_bytes());

    let err = read_jdf_bytes(&buf, "ambiguous.jdf".into()).unwrap_err();
    assert!(matches!(err, IoError::Unsupported(_)), "got {err:?}");
}

fn write_param_record(buf: &mut [u8], rec: usize, name: &[u8], is_f64: bool, f: f64, s: &[u8]) {
    if is_f64 {
        buf[rec + 0x10..rec + 0x18].copy_from_slice(&f.to_le_bytes());
        buf[rec + 0x20..rec + 0x24].copy_from_slice(&2u32.to_le_bytes());
    } else {
        buf[rec + 0x10..rec + 0x10 + s.len()].copy_from_slice(s);
        buf[rec + 0x20..rec + 0x24].copy_from_slice(&0u32.to_le_bytes());
    }
    buf[rec + 0x24..rec + 0x24 + name.len()].copy_from_slice(name);
}

#[test]
fn de_tiles_a_hand_built_2d_across_tile_blocks() {
    // 64×32 padded (two F2 tile-blocks), 34×2 real, complex X / real Y.
    let (cols_pad, rows_pad) = (64usize, 32usize);
    let (cols_real, rows_real) = (34usize, 2usize);
    let planes = 2usize;
    let rec_size = 64usize;
    let param_hdr = HEADER_LEN;
    let param_recs = param_hdr + 16;
    let n_records = 4usize;
    let data_start = param_recs + n_records * rec_size;
    let data_len = cols_pad * rows_pad * planes * 8;
    let mut buf = vec![0u8; data_start + data_len];

    buf[..8].copy_from_slice(MAGIC);
    buf[off::ENDIAN] = 1; // little-endian body
    buf[off::DATA_DIMENSION_NUMBER] = 2;
    buf[off::DATA_AXIS_TYPE] = AXIS_REAL_COMPLEX;
    buf[off::DATA_AXIS_TYPE + 1] = AXIS_REAL_COMPLEX;
    buf[off::DATA_POINTS..off::DATA_POINTS + 4].copy_from_slice(&(cols_pad as u32).to_be_bytes());
    buf[off::DATA_POINTS + 4..off::DATA_POINTS + 8]
        .copy_from_slice(&(rows_pad as u32).to_be_bytes());
    buf[off::DATA_OFFSET_STOP..off::DATA_OFFSET_STOP + 4]
        .copy_from_slice(&((cols_real - 1) as u32).to_be_bytes());
    buf[off::DATA_OFFSET_STOP + 4..off::DATA_OFFSET_STOP + 8]
        .copy_from_slice(&((rows_real - 1) as u32).to_be_bytes());
    buf[off::BASE_FREQ..off::BASE_FREQ + 8].copy_from_slice(&600.0f64.to_be_bytes());
    buf[off::BASE_FREQ + 8..off::BASE_FREQ + 16].copy_from_slice(&150.0f64.to_be_bytes());
    buf[off::DATA_AXIS_STOP..off::DATA_AXIS_STOP + 8].copy_from_slice(&1e-3f64.to_be_bytes());
    buf[off::DATA_AXIS_STOP + 8..off::DATA_AXIS_STOP + 16].copy_from_slice(&2e-3f64.to_be_bytes());
    buf[off::DATA_START..off::DATA_START + 4].copy_from_slice(&(data_start as u32).to_be_bytes());
    buf[off::DATA_LENGTH..off::DATA_LENGTH + 8].copy_from_slice(&(data_len as u64).to_be_bytes());

    buf[param_hdr..param_hdr + 4].copy_from_slice(&(rec_size as u32).to_le_bytes());
    buf[param_hdr + 8..param_hdr + 12].copy_from_slice(&((n_records - 1) as u32).to_le_bytes());
    write_param_record(&mut buf, param_recs, b"X_OFFSET", true, 1.5, b"");
    write_param_record(
        &mut buf,
        param_recs + rec_size,
        b"Y_OFFSET",
        true,
        75.0,
        b"",
    );
    write_param_record(
        &mut buf,
        param_recs + 2 * rec_size,
        b"X_DOMAIN",
        false,
        0.0,
        b"Proton",
    );
    write_param_record(
        &mut buf,
        param_recs + 3 * rec_size,
        b"Y_DOMAIN",
        false,
        0.0,
        b"Carbon13",
    );

    // Fill the data section in JEOL tiled order (plane, F1-block, F2-block,
    // row-in-tile, col-in-tile) with a distinctive value per cell.
    let re_val = |row: usize, col: usize| 100.0 + row as f64 * 10.0 + col as f64;
    let im_val = |row: usize, col: usize| 1.0 + row as f64 + col as f64 * 0.5;
    let n_f2b = cols_pad / TILE;
    let n_f1b = rows_pad / TILE;
    let mut w = data_start;
    for plane in 0..planes {
        for fb in 0..n_f1b {
            for cb in 0..n_f2b {
                for r in 0..TILE {
                    for c in 0..TILE {
                        let (row, col) = (fb * TILE + r, cb * TILE + c);
                        let v = if row < rows_real && col < cols_real {
                            if plane == 0 {
                                re_val(row, col)
                            } else {
                                im_val(row, col)
                            }
                        } else {
                            0.0
                        };
                        buf[w..w + 8].copy_from_slice(&v.to_le_bytes());
                        w += 8;
                    }
                }
            }
        }
    }

    let two = match read_jdf_bytes(&buf, "t2d.jdf".into()).unwrap() {
        Acquisition::D2(d) => *d,
        Acquisition::D1(_) => panic!("expected 2D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!((two.cols, two.rows), (cols_real, rows_real));
    assert_eq!(two.data.len(), cols_real * rows_real);
    for row in 0..rows_real {
        for col in 0..cols_real {
            let got = two.data[row * cols_real + col];
            // Imaginary channel negated (conjugated on read).
            assert_eq!(
                got,
                Complex64::new(re_val(row, col), -im_val(row, col)),
                "cell ({row},{col}) mismatch (col {col} is in F2 block {})",
                col / TILE
            );
        }
    }
    assert_eq!(two.direct.nucleus, "1H");
    assert_eq!(two.indirect.nucleus, "13C");
    assert!((two.direct.carrier_ppm - 1.5).abs() < 1e-9);
    assert!((two.indirect.carrier_ppm - 75.0).abs() < 1e-9);
    assert_eq!(two.quad, QuadMode::Complex);
    assert!(two.indirect_conjugate);
}

#[test]
fn de_tiles_a_hand_built_hypercomplex_2d() {
    // Both axes `Complex` (States hypercomplex): four sample planes ordered
    // (F1-imag?, F2-imag?) with F2 toggling fastest — RR, RI, IR, II. Each t1
    // increment's cosine (F1-real) and sine (F1-imag) channel must be interleaved
    // as consecutive stored rows and tagged QuadMode::States.
    let (cols_pad, rows_pad) = (32usize, 32usize);
    let (cols_real, rows_real) = (3usize, 2usize);
    let planes = 4usize;
    let rec_size = 64usize;
    let param_hdr = HEADER_LEN;
    let param_recs = param_hdr + 16;
    let data_start = param_recs + rec_size;
    let data_len = cols_pad * rows_pad * planes * 8;
    let mut buf = vec![0u8; data_start + data_len];

    buf[..8].copy_from_slice(MAGIC);
    buf[off::ENDIAN] = 1; // little-endian body
    buf[off::DATA_DIMENSION_NUMBER] = 2;
    buf[off::DATA_AXIS_TYPE] = AXIS_COMPLEX;
    buf[off::DATA_AXIS_TYPE + 1] = AXIS_COMPLEX;
    buf[off::DATA_POINTS..off::DATA_POINTS + 4].copy_from_slice(&(cols_pad as u32).to_be_bytes());
    buf[off::DATA_POINTS + 4..off::DATA_POINTS + 8]
        .copy_from_slice(&(rows_pad as u32).to_be_bytes());
    buf[off::DATA_OFFSET_STOP..off::DATA_OFFSET_STOP + 4]
        .copy_from_slice(&((cols_real - 1) as u32).to_be_bytes());
    buf[off::DATA_OFFSET_STOP + 4..off::DATA_OFFSET_STOP + 8]
        .copy_from_slice(&((rows_real - 1) as u32).to_be_bytes());
    buf[off::DATA_START..off::DATA_START + 4].copy_from_slice(&(data_start as u32).to_be_bytes());
    buf[off::DATA_LENGTH..off::DATA_LENGTH + 8].copy_from_slice(&(data_len as u64).to_be_bytes());
    buf[param_hdr..param_hdr + 4].copy_from_slice(&(rec_size as u32).to_le_bytes());
    buf[param_hdr + 8..param_hdr + 12].copy_from_slice(&0u32.to_le_bytes());

    // Distinctive value per (plane, row, col); a single 32-tile so tiling is a
    // plain row-major fill within each plane.
    let val = |plane: usize, row: usize, col: usize| {
        1000.0 * plane as f64 + 10.0 * row as f64 + col as f64
    };
    let mut w = data_start;
    for plane in 0..planes {
        for row in 0..rows_pad {
            for col in 0..cols_pad {
                let v = if row < rows_real && col < cols_real {
                    val(plane, row, col)
                } else {
                    0.0
                };
                buf[w..w + 8].copy_from_slice(&v.to_le_bytes());
                w += 8;
            }
        }
    }

    let two = match read_jdf_bytes(&buf, "hc2d.jdf".into()).unwrap() {
        Acquisition::D2(d) => *d,
        Acquisition::D1(_) => panic!("expected 2D"),
        Acquisition::Electrophysiology(_) => panic!("expected NMR"),
        Acquisition::Afm(_) => panic!("expected NMR"),
    };
    assert_eq!(two.quad, QuadMode::States);
    assert_eq!(two.cols, cols_real);
    assert_eq!(
        two.rows,
        2 * rows_real,
        "cos/sin channels interleaved as rows"
    );
    assert_eq!(two.data.len(), 2 * rows_real * cols_real);
    for row in 0..rows_real {
        for col in 0..cols_real {
            // Cosine channel (F1-real): planes RR (0) and RI (1).
            let cos = two.data[(2 * row) * cols_real + col];
            assert_eq!(cos, Complex64::new(val(0, row, col), -val(1, row, col)));
            // Sine channel (F1-imag): planes IR (2) and II (3).
            let sin = two.data[(2 * row + 1) * cols_real + col];
            assert_eq!(sin, Complex64::new(val(2, row, col), -val(3, row, col)));
        }
    }
    assert!(two.indirect_conjugate);
}

#[test]
fn detects_nus_echo_antiecho_grid_from_rate() {
    let mut p = Params::empty();
    p.strings.insert("sampling".into(), "NUS (Auto)".into());
    p.strings.insert("pn_type".into(), "y".into());
    p.strings.insert("nus_mode".into(), "poisson gap".into());
    p.f64s.insert("sampling_rate".into(), 25.0);
    let nus = detect_nus(&[], &p, 32).expect("nus detected");
    assert_eq!(nus.acquired, 32);
    assert_eq!(nus.grid, 128, "grid = round(M / rate)");
    assert!(nus.echo_antiecho, "pn_type = y is echo/anti-echo");
    assert!(nus.schedule.is_none(), "schedule withheld until entered");
    assert_eq!(nus.mode, "poisson gap");
}

#[test]
fn detects_nus_phase_modulated_and_skips_linear() {
    // Type-4 NUS (HMBC): NUS but no P/N conversion.
    let mut p = Params::empty();
    p.strings.insert("sampling".into(), "NUS (Auto)".into());
    p.f64s.insert("sampling_rate".into(), 25.0);
    let nus = detect_nus(&[], &p, 64).expect("nus detected");
    assert_eq!(nus.grid, 256);
    assert!(!nus.echo_antiecho);

    // Uniform (Linear) sampling is not NUS.
    let mut lin = Params::empty();
    lin.strings.insert("sampling".into(), "Linear".into());
    assert!(detect_nus(&[], &lin, 256).is_none());

    // No sampling parameter at all is not NUS.
    assert!(detect_nus(&[], &Params::empty(), 128).is_none());
}

fn serialized_nuslist(name: &[u8], values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x271du32.to_be_bytes());
    bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&0x2b2au32.to_be_bytes());
    bytes.extend_from_slice(&(values.len() as u32).to_be_bytes());
    for value in values {
        bytes.extend_from_slice(&0x271au32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[test]
fn extracts_serialized_big_endian_nuslist() {
    let bytes = serialized_nuslist(b"Y_NUSLIST", &[1, 2, 5, 9, 16]);
    assert_eq!(
        extract_nuslist(&bytes, b"Y_NUSLIST"),
        Some(vec![1, 2, 5, 9, 16])
    );
}

#[test]
fn detect_nus_uses_valid_file_schedule_and_original_grid() {
    let bytes = serialized_nuslist(b"Y_NUSLIST", &[1, 2, 5, 9]);
    let mut p = Params::empty();
    p.strings.insert("sampling".into(), "NUS (Auto)".into());
    p.f64s.insert("sampling_rate".into(), 50.0);
    p.f64s.insert("Y_ORIG_POINTS".into(), 16.0);
    p.f64s.insert("nuslist_idx_base".into(), 1.0);

    let nus = detect_nus(&bytes, &p, 4).expect("nus detected");
    assert_eq!(nus.grid, 16);
    assert_eq!(nus.schedule, Some(vec![0, 1, 4, 8]));
}

#[test]
fn detect_nus_rejects_invalid_file_schedule() {
    let mut p = Params::empty();
    p.strings.insert("sampling".into(), "NUS (Auto)".into());
    p.f64s.insert("sampling_rate".into(), 25.0);

    let wrong_count = serialized_nuslist(b"Y_NUSLIST", &[1, 2, 3]);
    assert!(
        detect_nus(&wrong_count, &p, 4)
            .expect("nus detected")
            .schedule
            .is_none()
    );

    let duplicate = serialized_nuslist(b"Y_NUSLIST", &[1, 2, 2, 4]);
    assert!(
        detect_nus(&duplicate, &p, 4)
            .expect("nus detected")
            .schedule
            .is_none()
    );
}
