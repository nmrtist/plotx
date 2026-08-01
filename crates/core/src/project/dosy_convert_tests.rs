use super::*;

fn maps() -> (DiffusionMap, IltResult) {
    (
        DiffusionMap {
            ppm: vec![1.0, 2.0],
            d: vec![1.1e-9, 1.2e-9],
            amp: vec![4.0, 5.0],
        },
        IltResult {
            ppm: vec![1.0, 2.0],
            d_grid: vec![1e-10, 1e-9, 1e-8],
            amp: vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]],
        },
    )
}

#[test]
fn dosy_binary_round_trip_validates_shapes_truncation_and_trailing_data() {
    let (dosy, ilt) = maps();
    let (encoded, shapes) = encode_dosy(Some(&dosy), Some(&ilt)).unwrap();
    let decoded = decode_dosy_bytes(&encoded, &shapes).unwrap();
    let decoded_dosy = decoded.dosy_map.unwrap();
    assert_eq!(decoded_dosy.ppm, dosy.ppm);
    assert_eq!(decoded_dosy.d, dosy.d);
    assert_eq!(decoded_dosy.amp, dosy.amp);
    let decoded_ilt = decoded.ilt_map.unwrap();
    assert_eq!(decoded_ilt.ppm, ilt.ppm);
    assert_eq!(decoded_ilt.d_grid, ilt.d_grid);
    assert_eq!(decoded_ilt.amp, ilt.amp);

    let truncated = decode_dosy_bytes(&encoded[..encoded.len() - 1], &shapes)
        .expect_err("a truncated final ILT row must be rejected")
        .to_string();
    assert!(truncated.contains("truncated"), "{truncated}");

    let mut trailing = encoded.clone();
    trailing.push(0);
    let trailing = decode_dosy_bytes(&trailing, &shapes)
        .expect_err("trailing data must be rejected")
        .to_string();
    assert!(trailing.contains("trailing data"), "{trailing}");

    let malformed_dosy = DiffusionMap {
        ppm: vec![1.0, 2.0],
        d: vec![1.0],
        amp: vec![1.0, 2.0],
    };
    let error = encode_dosy(Some(&malformed_dosy), None)
        .expect_err("unequal diffusion vectors must be rejected")
        .to_string();
    assert!(
        error.contains("length 1 does not match expected length 2"),
        "{error}"
    );

    let malformed_ilt = IltResult {
        ppm: vec![1.0],
        d_grid: vec![1.0, 2.0],
        amp: vec![vec![1.0]],
    };
    let error = encode_dosy(None, Some(&malformed_ilt))
        .expect_err("ragged ILT rows must be rejected")
        .to_string();
    assert!(
        error.contains("length 1 does not match expected length 2"),
        "{error}"
    );
}

/// The encode-side checks above all run on values we still own. These cover the
/// decoder's own integrity guards, which are the ones that face a file we did
/// not write: every length prefix inside the blob is untrusted input.
#[test]
fn decoding_rejects_a_payload_that_disagrees_with_itself() {
    let (dosy, ilt) = maps();
    let (bytes, shapes) = encode_dosy(Some(&dosy), Some(&ilt)).expect("encode");
    assert!(decode_dosy_bytes(&bytes, &shapes).is_ok());

    let mut wrong_magic = bytes.clone();
    wrong_magic[0] ^= 0xff;
    let error = decode_dosy_bytes(&wrong_magic, &shapes)
        .expect_err("a payload with a foreign signature must be rejected")
        .to_string();
    assert!(error.contains("invalid signature"), "{error}");

    // The shapes the recipe extension declares and the shapes inside the blob
    // are two independent statements; a project where they disagree must not
    // decode into whichever one happens to be read second.
    let mut claimed = shapes.clone();
    claimed.diffusion = Some(DiffusionMapShape { len: 3 });
    let error = decode_dosy_bytes(&bytes, &claimed)
        .expect_err("declared shapes that disagree with the blob must be rejected")
        .to_string();
    assert!(error.contains("do not match expected shapes"), "{error}");

    // A row count nothing has proven the payload can hold must fail as a project
    // error rather than by attempting the allocation it names.
    let huge = DosyShapes {
        diffusion: None,
        ilt: Some(IltMapShape {
            ppm_len: usize::MAX / 16,
            d_grid_len: 3,
        }),
    };
    assert!(decode_dosy_bytes(&bytes, &huge).is_err());
}

#[test]
fn decoding_rejects_a_truncated_array_inside_a_well_formed_header() {
    let (dosy, _) = maps();
    let (bytes, shapes) = encode_dosy(Some(&dosy), None).expect("encode");
    let truncated = &bytes[..bytes.len() - 8];
    let error = decode_dosy_bytes(truncated, &shapes)
        .expect_err("a truncated array must be rejected")
        .to_string();
    assert!(error.contains("truncated"), "{error}");
}
