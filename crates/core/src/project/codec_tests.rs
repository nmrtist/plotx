use super::*;
use std::io::{Cursor, Error, ErrorKind};

#[test]
fn bounded_reader_accepts_exact_eof_and_reports_path() {
    let mut reader =
        EntryReader::new(Cursor::new(b"abc"), "objects/a.bin", "test data", 3, 3).unwrap();
    let mut bytes = [0_u8; 3];
    reader.read_exact(&mut bytes).unwrap();
    reader.finish().unwrap();
    let error = EntryReader::new(Cursor::new(b"abcd"), "objects/a.bin", "test data", 4, 3)
        .err()
        .unwrap();
    assert!(error.to_string().contains("objects/a.bin"));
}

#[test]
fn bounded_reader_rejects_actual_bytes_beyond_declaration_or_budget() {
    let mut reader = EntryReader::new(Cursor::new(b"abcd"), "x.bin", "blob", 3, 8).unwrap();
    let mut bytes = [0_u8; 4];
    assert!(
        reader
            .read_exact(&mut bytes)
            .unwrap_err()
            .to_string()
            .contains("declared size")
    );
    let error = EntryReader::new(Cursor::new(b"abcd"), "x.bin", "blob", 4, 3)
        .err()
        .unwrap();
    assert!(error.to_string().contains("3-byte limit"));
}

#[test]
fn bounded_reader_detects_trailing_and_truncation() {
    let mut trailing = EntryReader::new(Cursor::new(b"ab"), "x.bin", "blob", 2, 2).unwrap();
    trailing.read_exact(&mut [0_u8; 1]).unwrap();
    assert!(
        trailing
            .finish()
            .unwrap_err()
            .to_string()
            .contains("trailing data")
    );
    let mut reader = EntryReader::new(Cursor::new(b"a"), "x.bin", "blob", 2, 2).unwrap();
    assert_eq!(
        reader.read_exact(&mut [0_u8; 2]).unwrap_err().kind(),
        ErrorKind::UnexpectedEof
    );
}

#[test]
fn bounded_reader_propagates_io_errors() {
    struct Fails;
    impl Read for Fails {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(Error::other("injected read failure"))
        }
    }
    let mut reader = EntryReader::new(Fails, "broken.bin", "blob", 1, 1).unwrap();
    assert!(
        reader
            .read_exact(&mut [0_u8; 1])
            .unwrap_err()
            .to_string()
            .contains("injected")
    );
}

#[test]
fn zip_entries_are_scoped_and_deflated_data_loads_sequentially() {
    let path =
        std::env::temp_dir().join(format!("plotx-limited-reader-{}.zip", uuid::Uuid::new_v4()));
    let file = File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_bytes(&mut writer, options, "one.bin", b"one").unwrap();
    write_bytes(&mut writer, options, "two.bin", b"two").unwrap();
    writer.finish().unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let one = read_entry(&mut archive, "one.bin", "test", 16, |reader| {
        let mut b = [0; 3];
        reader.read_exact(&mut b)?;
        Ok(b)
    })
    .unwrap();
    let two = read_entry(&mut archive, "two.bin", "test", 16, |reader| {
        let mut b = [0; 3];
        reader.read_exact(&mut b)?;
        Ok(b)
    })
    .unwrap();
    assert_eq!((&one, &two), (b"one", b"two"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn highly_compressible_entry_is_rejected_from_declared_size() {
    let path = std::env::temp_dir().join(format!("plotx-zip-bomb-{}.zip", uuid::Uuid::new_v4()));
    let file = File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_bytes(&mut writer, options, "large.bin", &vec![0_u8; 1024 * 1024]).unwrap();
    writer.finish().unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let error = read_entry(&mut archive, "large.bin", "synthetic", 1024, |_| Ok(())).unwrap_err();
    assert!(error.to_string().contains("exceeding the 1024-byte limit"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn crc_failure_surfaces_when_success_path_consumes_eof() {
    let path = std::env::temp_dir().join(format!("plotx-crc-{}.zip", uuid::Uuid::new_v4()));
    let payload = b"unique-crc-payload-19f247";
    let file = File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    write_bytes(&mut writer, options, "crc.bin", payload).unwrap();
    writer.finish().unwrap();
    let mut archive_bytes = std::fs::read(&path).unwrap();
    let offset = archive_bytes
        .windows(payload.len())
        .position(|window| window == payload)
        .unwrap();
    archive_bytes[offset] ^= 0xff;
    std::fs::write(&path, archive_bytes).unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let error = read_entry(&mut archive, "crc.bin", "CRC test", 1024, |reader| {
        let mut bytes = vec![0; payload.len()];
        reader.read_exact(&mut bytes)?;
        Ok(())
    })
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("crc.bin"), "{message}");
    assert!(
        message.to_ascii_lowercase().contains("crc") || message.contains("checksum"),
        "{message}"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn complex_decoder_uses_constant_sized_read_requests() {
    struct Probe {
        inner: Cursor<Vec<u8>>,
        largest_request: usize,
    }
    impl Read for Probe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.largest_request = self.largest_request.max(buffer.len());
            self.inner.read(buffer)
        }
    }
    let probe = Probe {
        inner: Cursor::new(vec![0_u8; 16 * 100_000]),
        largest_request: 0,
    };
    let mut reader = EntryReader::new(probe, "nmr.bin", "NMR", 1_600_000, 1_600_000).unwrap();
    let values = complex_from_reader(&mut reader).unwrap();
    assert_eq!(values.len(), 100_000);
    assert!(reader.inner.largest_request <= 16);
    reader.finish().unwrap();
}
