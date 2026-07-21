//! Streaming CSV/TSV writer shared by every numerical export path, and the
//! delimiter vocabulary shared with table parsing in `plotx-core`.

use std::fmt;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delimiter {
    Comma,
    Tab,
    Semicolon,
}

impl Delimiter {
    /// Every supported delimiter, in the order parsers try them during
    /// auto-detection.
    pub const ALL: [Self; 3] = [Self::Comma, Self::Tab, Self::Semicolon];

    pub const fn as_char(self) -> char {
        match self {
            Self::Comma => ',',
            Self::Tab => '\t',
            Self::Semicolon => ';',
        }
    }

    const fn byte(self) -> u8 {
        self.as_char() as u8
    }
}

impl fmt::Display for Delimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Comma => "comma",
            Self::Tab => "tab",
            Self::Semicolon => "semicolon",
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Field<'a> {
    Text(&'a str),
    Number(f64),
    Empty,
}

pub struct DelimitedWriter<W> {
    inner: W,
    delimiter: Delimiter,
}

impl<W: Write> DelimitedWriter<W> {
    pub fn new(inner: W, delimiter: Delimiter) -> Self {
        Self { inner, delimiter }
    }

    pub fn write_record(&mut self, fields: &[Field<'_>]) -> io::Result<()> {
        for (index, field) in fields.iter().enumerate() {
            if index != 0 {
                self.inner.write_all(&[self.delimiter.byte()])?;
            }
            match field {
                Field::Text(value) => self.write_text(value)?,
                Field::Number(value) if value.is_nan() => self.inner.write_all(b"NaN")?,
                Field::Number(value) if *value == f64::INFINITY => self.inner.write_all(b"+Inf")?,
                Field::Number(value) if *value == f64::NEG_INFINITY => {
                    self.inner.write_all(b"-Inf")?
                }
                Field::Number(value) => write!(self.inner, "{value}")?,
                Field::Empty => {}
            }
        }
        self.inner.write_all(b"\n")
    }

    fn write_text(&mut self, value: &str) -> io::Result<()> {
        let delimiter = self.delimiter.byte() as char;
        if !value.contains([delimiter, '"', '\n', '\r']) {
            return self.inner.write_all(value.as_bytes());
        }
        self.inner.write_all(b"\"")?;
        for part in value.split_inclusive('"') {
            self.inner.write_all(part.as_bytes())?;
            if part.ends_with('"') {
                self.inner.write_all(b"\"")?;
            }
        }
        self.inner.write_all(b"\"")
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_and_tsv_escape_only_their_own_delimiter() {
        let fields = [
            Field::Text("plain"),
            Field::Text("comma,value"),
            Field::Text("tab\tvalue"),
            Field::Text("quote \" value"),
            Field::Text("line\r\nbreak"),
            Field::Text("光谱"),
        ];
        let mut csv = DelimitedWriter::new(Vec::new(), Delimiter::Comma);
        csv.write_record(&fields).unwrap();
        assert_eq!(
            String::from_utf8(csv.into_inner()).unwrap(),
            "plain,\"comma,value\",tab\tvalue,\"quote \"\" value\",\"line\r\nbreak\",光谱\n"
        );
        let mut tsv = DelimitedWriter::new(Vec::new(), Delimiter::Tab);
        tsv.write_record(&fields).unwrap();
        assert_eq!(
            String::from_utf8(tsv.into_inner()).unwrap(),
            "plain\tcomma,value\t\"tab\tvalue\"\t\"quote \"\" value\"\t\"line\r\nbreak\"\t光谱\n"
        );
    }

    #[test]
    fn non_finite_float_values_are_not_confused_with_missing_cells() {
        let mut writer = DelimitedWriter::new(Vec::new(), Delimiter::Comma);
        writer
            .write_record(&[
                Field::Number(f64::NAN),
                Field::Number(f64::INFINITY),
                Field::Number(f64::NEG_INFINITY),
                Field::Empty,
            ])
            .unwrap();
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "NaN,+Inf,-Inf,\n"
        );
    }

    #[test]
    fn numbers_round_trip_and_non_finite_values_are_explicit() {
        let value = 1.234_567_890_123_456_7_f64;
        let mut writer = DelimitedWriter::new(Vec::new(), Delimiter::Comma);
        writer
            .write_record(&[
                Field::Number(value),
                Field::Number(f64::NAN),
                Field::Number(f64::INFINITY),
                Field::Empty,
            ])
            .unwrap();
        let text = String::from_utf8(writer.into_inner()).unwrap();
        assert_eq!(
            text.trim_end().split(',').next().unwrap().parse::<f64>(),
            Ok(value)
        );
        assert_eq!(text, "1.2345678901234567,NaN,+Inf,\n");
    }

    struct Fails;

    impl Write for Fails {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("deliberate failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_errors_are_propagated() {
        let error = DelimitedWriter::new(Fails, Delimiter::Comma)
            .write_record(&[Field::Text("value")])
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }
}
