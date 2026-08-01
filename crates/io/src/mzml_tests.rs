use super::*;
use flate2::{Compression, write::ZlibEncoder};
use std::io::{self, BufReader, Cursor, Read, Write};

struct ChunkedRead<R> {
    inner: R,
    chunk: usize,
}

impl<R: Read> Read for ChunkedRead<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let limit = output.len().min(self.chunk);
        self.inner.read(&mut output[..limit])
    }
}

struct RepeatingRead {
    prefix: Cursor<Vec<u8>>,
    repeated: u8,
    remaining: usize,
}

impl Read for RepeatingRead {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let prefix_read = self.prefix.read(output)?;
        if prefix_read != 0 {
            return Ok(prefix_read);
        }
        let count = output.len().min(self.remaining);
        output[..count].fill(self.repeated);
        self.remaining -= count;
        Ok(count)
    }
}

#[derive(Clone, Copy)]
enum TestPrecision {
    F32,
    F64,
}

fn encoded(values: &[f64], precision: TestPrecision, zlib: bool) -> String {
    let bytes = values
        .iter()
        .flat_map(|value| match precision {
            TestPrecision::F32 => (value.to_owned() as f32).to_le_bytes().to_vec(),
            TestPrecision::F64 => value.to_le_bytes().to_vec(),
        })
        .collect::<Vec<_>>();
    let bytes = if zlib {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap()
    } else {
        bytes
    };
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn array(accession: &str, values: &[f64], precision: TestPrecision, zlib: bool) -> String {
    let precision_accession = match precision {
        TestPrecision::F32 => "MS:1000521",
        TestPrecision::F64 => "MS:1000523",
    };
    let compression = if zlib { "MS:1000574" } else { "MS:1000576" };
    format!(
        "<binaryDataArray><cvParam accession=\"{accession}\"/><cvParam accession=\"{precision_accession}\"/><cvParam accession=\"{compression}\"/><binary>{}</binary></binaryDataArray>",
        encoded(values, precision, zlib)
    )
}

fn spectrum(
    id: &str,
    level: u8,
    polarity: &str,
    seconds: bool,
    precision: TestPrecision,
    zlib: bool,
) -> String {
    let (time, unit) = if seconds {
        ("90", "UO:0000010")
    } else {
        ("1.5", "UO:0000031")
    };
    format!(
        "<spectrum id=\"{id}\" defaultArrayLength=\"2\"><cvParam accession=\"MS:1000511\" value=\"{level}\"/><cvParam accession=\"{polarity}\"/><cvParam accession=\"MS:1000127\"/><scanList><scan><cvParam accession=\"MS:1000016\" value=\"{time}\" unitAccession=\"{unit}\"/></scan></scanList><binaryDataArrayList count=\"2\">{}{}</binaryDataArrayList></spectrum>",
        array("MS:1000514", &[100.25, 200.5], precision, zlib),
        array("MS:1000515", &[10.0, 30.0], precision, zlib)
    )
}

fn document(spectra: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><mzML><run id=\"run-1\"><spectrumList>{spectra}</spectrumList></run></mzML>"
    )
}

fn parsed(xml: String) -> MassSpecRun {
    parse(Cursor::new(xml), "fixture.mzML".to_owned()).unwrap()
}

#[test]
fn imports_uncompressed_f64_and_normalizes_seconds_and_minutes() {
    let run = parsed(document(
        &(spectrum("scan=1", 1, "MS:1000130", false, TestPrecision::F64, false)
            + &spectrum("scan=2", 1, "MS:1000130", true, TestPrecision::F64, false)),
    ));
    assert_eq!(run.streams.len(), 1);
    assert_eq!(run.streams[0].spectra[0].mz, [100.25, 200.5]);
    assert_eq!(run.streams[0].spectra[0].intensity, [10.0, 30.0]);
    assert_eq!(run.streams[0].spectra[0].retention_time_min, 1.5);
    assert_eq!(run.streams[0].spectra[1].retention_time_min, 1.5);
    assert_eq!(run.streams[0].spectra[0].polarity, Polarity::Positive);
}

#[test]
fn imports_zlib_f32_into_f64() {
    let run = parsed(document(&spectrum(
        "scan=1",
        1,
        "MS:1000129",
        false,
        TestPrecision::F32,
        true,
    )));
    assert_eq!(run.streams[0].spectra[0].mz, [100.25, 200.5]);
    assert_eq!(run.streams[0].spectra[0].polarity, Polarity::Negative);
}

#[test]
fn groups_by_ms_level_and_polarity_with_stable_ids() {
    let xml = document(
        &(spectrum("first", 2, "MS:1000129", false, TestPrecision::F64, false)
            + &spectrum("second", 1, "MS:1000130", false, TestPrecision::F64, false)
            + &spectrum("third", 2, "MS:1000129", false, TestPrecision::F64, false)),
    );
    let run = parsed(xml);
    assert_eq!(run.streams.len(), 2);
    assert_eq!(run.streams[0].id, AcquisitionStreamId::new(1));
    assert_eq!(
        run.streams[0].spectra[0].source_native_id.as_deref(),
        Some("second")
    );
    assert_eq!(
        run.streams[1]
            .spectra
            .iter()
            .map(|s| s.id.get())
            .collect::<Vec<_>>(),
        [1, 3]
    );
}

#[test]
fn rejects_invalid_base64_corrupt_zlib_and_mismatched_or_missing_arrays() {
    let valid = spectrum("bad", 1, "MS:1000130", false, TestPrecision::F64, false);
    let invalid_b64 = document(&valid.replace(
        &encoded(&[100.25, 200.5], TestPrecision::F64, false),
        "!!!!",
    ));
    assert!(
        parse(Cursor::new(invalid_b64), "x".into())
            .unwrap_err()
            .to_string()
            .contains("invalid base64")
    );

    let corrupt = document(&valid.replace("MS:1000576", "MS:1000574"));
    assert!(
        parse(Cursor::new(corrupt), "x".into())
            .unwrap_err()
            .to_string()
            .contains("zlib")
    );

    let mismatch = document(&valid.replace("defaultArrayLength=\"2\"", "defaultArrayLength=\"3\""));
    assert!(
        parse(Cursor::new(mismatch), "x".into())
            .unwrap_err()
            .to_string()
            .contains("declares 3")
    );

    let missing = document(&valid.replacen(
        &array("MS:1000515", &[10.0, 30.0], TestPrecision::F64, false),
        "",
        1,
    ));
    assert!(
        parse(Cursor::new(missing), "x".into())
            .unwrap_err()
            .to_string()
            .contains("missing the intensity")
    );
}

#[test]
fn rejects_numpress_and_implausible_declared_length_before_binary_decode() {
    let numpress = document(
        &spectrum(
            "numpress",
            1,
            "MS:1000130",
            false,
            TestPrecision::F64,
            false,
        )
        .replace("MS:1000576", "MS:1002312"),
    );
    assert!(
        parse(Cursor::new(numpress), "x".into())
            .unwrap_err()
            .to_string()
            .contains("Numpress")
    );
    let malicious = document("<spectrum id=\"huge\" defaultArrayLength=\"5000001\"></spectrum>");
    assert!(
        parse(Cursor::new(malicious), "x".into())
            .unwrap_err()
            .to_string()
            .contains("more than")
    );
}

#[test]
fn extension_dispatch_is_ascii_case_insensitive() {
    for name in ["run.mzML", "run.MZML"] {
        assert_eq!(crate::detect_format(name).unwrap(), DataFormat::MzMl);
    }
}

#[test]
fn bounds_retained_attributes_and_xml_tokens_before_parser_buffer_growth() {
    let oversized_id = "x".repeat(MAX_ATTRIBUTE_BYTES + 1);
    let error = parse(
        Cursor::new(document(&format!(
            "<spectrum id=\"{oversized_id}\" defaultArrayLength=\"0\"></spectrum>"
        ))),
        "attribute.mzML".into(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("attribute exceeds"), "{error}");

    for prefix in [
        b"<!--".as_slice(),
        b"<![CDATA[".as_slice(),
        b"<mzML><run><x>".as_slice(),
    ] {
        let input = RepeatingRead {
            prefix: Cursor::new(prefix.to_vec()),
            repeated: b'x',
            remaining: MAX_XML_EVENT_BYTES + 4096,
        };
        let error = parse(BufReader::with_capacity(257, input), "token.mzML".into())
            .unwrap_err()
            .to_string();
        assert!(error.contains("event exceeds"), "{error}");
    }

    let unterminated = RepeatingRead {
        prefix: Cursor::new(b"<mzML><run><spectrum id=\"".to_vec()),
        repeated: b'x',
        remaining: MAX_XML_EVENT_BYTES + 4096,
    };
    let error = parse(
        BufReader::with_capacity(257, unterminated),
        "unterminated.mzML".into(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("event exceeds"), "{error}");
}

#[test]
fn ordinary_chunked_xml_still_parses() {
    let xml = document(&spectrum(
        "scan=1",
        1,
        "MS:1000130",
        false,
        TestPrecision::F64,
        false,
    ));
    let input = ChunkedRead {
        inner: Cursor::new(xml),
        chunk: 3,
    };
    let run = parse(BufReader::with_capacity(5, input), "chunked.mzML".into()).unwrap();
    assert_eq!(run.streams[0].spectra[0].mz, [100.25, 200.5]);
}

#[test]
fn zlib_requires_one_complete_member_and_no_trailing_input() {
    let valid = {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&1.0_f64.to_le_bytes()).unwrap();
        encoder.finish().unwrap()
    };
    assert_eq!(
        decompress_zlib_exact(&valid, "valid").unwrap(),
        1.0_f64.to_le_bytes()
    );

    let truncated = &valid[..valid.len() - 1];
    let error = decompress_zlib_exact(truncated, "truncated")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("truncated") || error.contains("failed"),
        "{error}"
    );

    let mut trailing = valid.clone();
    trailing.extend_from_slice(b"junk");
    assert!(
        decompress_zlib_exact(&trailing, "trailing")
            .unwrap_err()
            .to_string()
            .contains("trailing")
    );

    let mut second = valid.clone();
    second.extend_from_slice(&valid);
    assert!(
        decompress_zlib_exact(&second, "second")
            .unwrap_err()
            .to_string()
            .contains("trailing")
    );
}

#[test]
fn rejects_zlib_output_beyond_the_array_limit() {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let zeros = [0_u8; 64 * 1024];
    let mut remaining = MAX_DECODED_BYTES_PER_ARRAY + 1;
    while remaining != 0 {
        let count = remaining.min(zeros.len());
        encoder.write_all(&zeros[..count]).unwrap();
        remaining -= count;
    }
    let bomb = encoder.finish().unwrap();
    let error = decompress_zlib_exact(&bomb, "bomb")
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceeds limit"), "{error}");
}

#[test]
fn reports_claimed_metadata_and_validation_failures() {
    let base = spectrum("scan=1", 1, "MS:1000130", false, TestPrecision::F64, false);
    let malformed = "<mzML><run><spectrum>";
    assert!(
        parse(Cursor::new(malformed), "malformed".into())
            .unwrap_err()
            .to_string()
            .contains("truncated")
    );

    let unsupported_unit = document(&base.replace("UO:0000031", "UO:9999999"));
    assert!(
        parse(Cursor::new(unsupported_unit), "unit".into())
            .unwrap_err()
            .to_string()
            .contains("unsupported scan start time unit")
    );

    let profile = parsed(document(&base.replace("MS:1000127", "MS:1000128")));
    assert_eq!(
        profile.streams[0].spectra[0].representation,
        SpectrumRepresentation::Profile
    );

    let missing = base
        .replace("<cvParam accession=\"MS:1000511\" value=\"1\"/>", "")
        .replace(
            "<cvParam accession=\"MS:1000016\" value=\"1.5\" unitAccession=\"UO:0000031\"/>",
            "",
        );
    let warned = parsed(document(&missing));
    assert_eq!(warned.import_warnings.len(), 2);
    assert!(warned.import_warnings[0].contains("assumed MS1"));
    assert!(warned.import_warnings[1].contains("used 0 min"));

    for invalid in [
        spectrum("scan=1", 1, "MS:1000130", false, TestPrecision::F64, false).replace(
            &encoded(&[100.25, 200.5], TestPrecision::F64, false),
            &encoded(&[f64::NAN, 200.5], TestPrecision::F64, false),
        ),
        spectrum("scan=1", 1, "MS:1000130", false, TestPrecision::F64, false).replacen(
            &encoded(&[10.0, 30.0], TestPrecision::F64, false),
            &encoded(&[f64::INFINITY, 30.0], TestPrecision::F64, false),
            1,
        ),
        base.replace("value=\"1.5\"", "value=\"NaN\""),
    ] {
        assert!(
            parse(Cursor::new(document(&invalid)), "nonfinite".into())
                .unwrap_err()
                .to_string()
                .contains("invalid spectrum")
        );
    }
}

#[test]
fn accepts_explicit_cvparam_end_tags() {
    let xml = document(
        &spectrum("scan=1", 1, "MS:1000130", false, TestPrecision::F64, false)
            .replace("/>", "></cvParam>"),
    );
    let run = parsed(xml);
    assert_eq!(run.streams[0].spectra[0].ms_level, 1);
    assert_eq!(
        run.streams[0].spectra[0].representation,
        SpectrumRepresentation::Centroid
    );
}
