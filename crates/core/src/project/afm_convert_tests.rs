use super::*;
use plotx_io::{AfmForceSet, AfmFrameDirection, AfmImageChannel, AfmScale};

fn fixture() -> AfmData {
    AfmData {
        images: vec![AfmImageChannel {
            name: "Height".to_owned(),
            width: 2,
            height: 2,
            scan_size_x: 1.0,
            scan_size_y: 2.0,
            lateral_unit: "um".to_owned(),
            scale: AfmScale {
                multiplier: 0.25,
                offset: 1.0,
                unit: "nm".to_owned(),
            },
            raw: Arc::from([1, 2, 3, 4]),
            frame_direction: AfmFrameDirection::Trace,
        }],
        forces: Some(AfmForceSet {
            grid_width: 1,
            grid_height: 2,
            samples_per_curve: 4,
            raw: Arc::from([10, 11, 12, 13, 20, 21, 22, 23]),
            signal_scale: AfmScale {
                multiplier: 0.5,
                offset: 0.0,
                unit: "V".to_owned(),
            },
            sample_period_s: Some(1e-6),
            z_positions: Some(Arc::from([0.0, 1.0, 2.0, 3.0])),
            display_order: Arc::from([1, 0, 3, 2]),
            approach_samples: 2,
            deflection_sensitivity_m_per_v: Some(2e-8),
            spring_constant_n_per_m: Some(0.4),
        }),
        source: "synthetic.spm".to_owned(),
        import_warnings: vec!["synthetic warning".to_owned()],
    }
}

#[test]
fn binary_afm_round_trip_preserves_arrays_and_metadata() {
    let original = fixture();
    let encoded = encode_afm(&original).unwrap();
    let decoded = decode_afm_bytes(&encoded).unwrap();

    assert_eq!(decoded.images[0].raw.as_ref(), [1, 2, 3, 4]);
    assert_eq!(decoded.images[0].name, "Height");
    let forces = decoded.forces.unwrap();
    assert_eq!(forces.raw.as_ref(), [10, 11, 12, 13, 20, 21, 22, 23]);
    assert_eq!(forces.display_order.as_ref(), [1, 0, 3, 2]);
    assert_eq!(forces.z_positions.unwrap().as_ref(), [0.0, 1.0, 2.0, 3.0]);
    assert_eq!(forces.spring_constant_n_per_m, Some(0.4));
}

#[test]
fn binary_afm_rejects_truncation_and_trailing_bytes() {
    let encoded = encode_afm(&fixture()).unwrap();
    assert!(decode_afm_bytes(&encoded[..encoded.len() - 1]).is_err());
    let mut trailing = encoded;
    trailing.push(0);
    assert!(decode_afm_bytes(&trailing).is_err());
}

#[test]
fn scalar_arrays_are_written_in_bounded_chunks() {
    #[derive(Default)]
    struct WriteCounter {
        calls: usize,
        bytes: usize,
    }

    impl std::io::Write for WriteCounter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            self.bytes += buffer.len();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let values = vec![42_i32; VALUES_PER_CHUNK + 1];
    let mut output = WriteCounter::default();
    write_i32s(&mut output, &values).unwrap();

    assert_eq!(output.calls, 3);
    assert_eq!(output.bytes, 8 + values.len() * std::mem::size_of::<i32>());
}
