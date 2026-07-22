//! Bounded detection and engine-neutral transport types for Origin projects.
//!
//! Format detection is based only on the first line of the file. The module
//! does not use filename extensions, and OPJU input is detection-only.

mod opj;
mod opju;
mod reader;

const DEFAULT_MAX_HEADER_BYTES: usize = 128;
const MIB: usize = 1024 * 1024;
const OPJ_MAGIC: &[u8] = b"CPYA";
const OPJU_MAGIC: &[u8] = b"CPYUA";
const ORIGIN_7_V552_VERSION: &str = "4.2673 552";

/// Origin project container family detected from its file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginFormat {
    /// Classic Origin project container.
    Opj,
    /// Newer Unicode Origin project container.
    Opju,
}

/// Whether the detected container can be decoded by this parser profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginSupport {
    /// The exact producer profile is supported.
    Supported,
    /// The family is recognized but deliberately not decoded.
    RecognizedUnsupported,
}

/// Exact producer profiles with verified parsing rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginProfile {
    /// Classic Origin 7 project header `CPYA 4.2673 552#`.
    Origin7V552,
}

/// Byte order used by a verified Origin profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginByteOrder {
    /// Least-significant byte first.
    LittleEndian,
}

/// Integer components parsed from the producer version header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginHeaderVersion {
    /// Version component before the dot.
    pub major: u16,
    /// Version component after the dot.
    pub minor: u16,
    /// Numeric producer build.
    pub build: u32,
}

/// Result of bounded, content-based Origin format detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginProbe {
    /// Detected container family.
    pub format: OriginFormat,
    /// Version and build text exactly as written in the header.
    pub raw_version: String,
    /// Parsed integer version components.
    pub version: OriginHeaderVersion,
    /// Byte order of the detected family.
    pub byte_order: OriginByteOrder,
    /// Exact supported parser profile, if one is verified.
    pub profile: Option<OriginProfile>,
    /// Whether full decoding is supported.
    pub support: OriginSupport,
}

/// A cell decoded from an Origin worksheet column.
#[derive(Debug, Clone, PartialEq)]
pub enum OriginCell {
    /// Missing or invalid value.
    Null,
    /// Floating-point value.
    Float(f64),
    /// Signed integer value.
    Integer(i64),
    /// Text value.
    Text(String),
}

/// Logical storage class of an Origin worksheet column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginColumnType {
    /// Floating-point values and nulls.
    Float,
    /// Signed integer values and nulls.
    Integer,
    /// Text values and nulls.
    Text,
    /// A verified mixture of numeric and text values.
    Mixed,
}

/// One bounded metadata value retained from an Origin object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginMetadataEntry {
    /// Stable or source-derived metadata key.
    pub key: String,
    /// User-safe decoded value.
    pub value: String,
}

/// One named project note retained without concatenating independent content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginNote {
    /// Source note name.
    pub name: String,
    /// Decoded note content.
    pub content: String,
}

/// One worksheet column in the neutral import model.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginColumn {
    /// Source column name.
    pub name: String,
    /// Optional source long name.
    pub long_name: Option<String>,
    /// Optional Origin column role such as X or Y.
    pub role: Option<String>,
    /// Optional source units.
    pub units: Option<String>,
    /// Optional source comments.
    pub comments: Option<String>,
    /// Logical class of the decoded cells.
    pub column_type: OriginColumnType,
    /// Cells in source row order.
    pub cells: Vec<OriginCell>,
}

/// One worksheet decoded from an Origin workbook.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginWorksheet {
    /// Source worksheet name.
    pub name: String,
    /// Worksheet columns in source order.
    pub columns: Vec<OriginColumn>,
    /// Logical row count, including trailing null rows.
    pub row_count: usize,
    /// Bounded worksheet-level metadata.
    pub metadata: Vec<OriginMetadataEntry>,
}

/// One workbook decoded from an Origin project.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginWorkbook {
    /// Source workbook name.
    pub name: String,
    /// Worksheets in source order.
    pub worksheets: Vec<OriginWorksheet>,
}

/// Severity of a recoverable Origin parsing diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginDiagnosticSeverity {
    /// Informational detail that does not alter imported values.
    Info,
    /// A bounded object or value was skipped or degraded.
    Warning,
}

/// Stable category for a recoverable Origin parsing diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginDiagnosticCode {
    /// An independently framed unsupported object was skipped.
    UnsupportedObjectSkipped,
    /// An independently framed unsupported column was skipped.
    UnsupportedColumnSkipped,
    /// Nonessential metadata could not be retained.
    MetadataSkipped,
    /// Text or value decoding required a documented fallback.
    DecodingWarning,
}

/// Structured location of a recoverable parsing diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginObjectLocation {
    /// Workbook name, when known.
    pub workbook: Option<String>,
    /// Worksheet name, when known.
    pub worksheet: Option<String>,
    /// Column name, when known.
    pub column: Option<String>,
    /// Source byte offset, when meaningful.
    pub byte_offset: Option<usize>,
}

/// User-safe diagnostic emitted while retaining a usable project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginDiagnostic {
    /// Stable diagnostic category.
    pub code: OriginDiagnosticCode,
    /// Diagnostic severity.
    pub severity: OriginDiagnosticSeverity,
    /// Source object or byte location, when known.
    pub location: Option<OriginObjectLocation>,
    /// Plain-language message safe to present to a user.
    pub message: String,
}

/// Count of skipped Origin objects of one source-defined kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginUnsupportedObjectSummary {
    /// Source object kind, such as graph or matrix.
    pub kind: String,
    /// Number of skipped objects of this kind.
    pub count: usize,
}

/// Conservative resource accounting carried into later conversion stages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OriginResourceUsage {
    /// Retained source input bytes.
    pub input_bytes: usize,
    /// Parser-owned decoded and structural bytes.
    pub parser_bytes: usize,
    /// Decoded text bytes included in parser storage.
    pub decoded_text_bytes: usize,
    /// Cumulative owned-byte charge across import stages.
    pub total_owned_bytes: usize,
    /// Decoded workbook count.
    pub workbooks: usize,
    /// Decoded worksheet count.
    pub worksheets: usize,
    /// Decoded column count.
    pub columns: usize,
    /// Decoded cell count.
    pub cells: usize,
    /// Logical metadata records traversed, excluding list terminators.
    pub metadata_records: usize,
}

/// Complete engine-neutral result of an Origin project read.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginProject {
    /// Probe that selected the exact parsing profile.
    pub probe: OriginProbe,
    /// Bounded project parameters.
    pub parameters: Vec<OriginMetadataEntry>,
    /// Bounded project notes.
    pub notes: Vec<OriginNote>,
    /// Decoded workbooks in source order.
    pub workbooks: Vec<OriginWorkbook>,
    /// Recoverable parsing diagnostics.
    pub diagnostics: Vec<OriginDiagnostic>,
    /// Counts of independently skipped unsupported objects.
    pub unsupported_objects: Vec<OriginUnsupportedObjectSummary>,
    /// Conservative parser and allocation accounting.
    pub resource_usage: OriginResourceUsage,
}

/// Resource limits applied to untrusted Origin input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginLimits {
    /// Maximum complete source size.
    pub max_input_bytes: usize,
    /// Maximum first-line header size, including LF.
    pub max_header_bytes: usize,
    /// Maximum individual framed block size.
    pub max_block_bytes: usize,
    /// Maximum individual decoded string size.
    pub max_string_bytes: usize,
    /// Maximum cumulative decoded text size.
    pub max_decoded_text_bytes: usize,
    /// Maximum cumulative parser-owned allocation.
    pub max_parser_bytes: usize,
    /// Maximum cumulative owned allocation across import stages.
    pub max_total_owned_bytes: usize,
    /// Maximum workbook count.
    pub max_workbooks: usize,
    /// Maximum source window records retained for worksheet association.
    pub max_window_records: usize,
    /// Maximum worksheet count per workbook.
    pub max_worksheets_per_workbook: usize,
    /// Maximum total worksheet data column count.
    pub max_columns: usize,
    /// Maximum logical metadata records traversed, excluding list terminators.
    pub max_metadata_records: usize,
    /// Maximum logical rows in one column.
    pub max_rows_per_column: usize,
    /// Maximum total decoded cell count.
    pub max_cells: usize,
    /// Maximum metadata nesting depth accepted by readers.
    pub max_metadata_depth: usize,
}

impl Default for OriginLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 128 * MIB,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_block_bytes: 32 * MIB,
            max_string_bytes: MIB,
            max_decoded_text_bytes: 32 * MIB,
            max_parser_bytes: 128 * MIB,
            max_total_owned_bytes: 384 * MIB,
            max_workbooks: 256,
            max_window_records: 1024,
            max_worksheets_per_workbook: 128,
            max_columns: 4096,
            max_metadata_records: 65_536,
            max_rows_per_column: 1_000_000,
            max_cells: 2_000_000,
            max_metadata_depth: 32,
        }
    }
}

impl OriginLimits {
    /// Rejects unusable custom limits before any input is parsed.
    pub fn validate(&self) -> Result<(), OriginError> {
        let values = [
            ("max_input_bytes", self.max_input_bytes),
            ("max_header_bytes", self.max_header_bytes),
            ("max_block_bytes", self.max_block_bytes),
            ("max_string_bytes", self.max_string_bytes),
            ("max_decoded_text_bytes", self.max_decoded_text_bytes),
            ("max_parser_bytes", self.max_parser_bytes),
            ("max_total_owned_bytes", self.max_total_owned_bytes),
            ("max_workbooks", self.max_workbooks),
            ("max_window_records", self.max_window_records),
            (
                "max_worksheets_per_workbook",
                self.max_worksheets_per_workbook,
            ),
            ("max_columns", self.max_columns),
            ("max_metadata_records", self.max_metadata_records),
            ("max_rows_per_column", self.max_rows_per_column),
            ("max_cells", self.max_cells),
            ("max_metadata_depth", self.max_metadata_depth),
        ];
        if let Some((name, value)) = values.into_iter().find(|(_, value)| *value == 0) {
            return Err(OriginError::InvalidLimit {
                name,
                value,
                reason: "the limit must be greater than zero",
            });
        }
        if self.max_input_bytes.checked_add(1).is_none() {
            return Err(OriginError::InvalidLimit {
                name: "max_input_bytes",
                value: self.max_input_bytes,
                reason: "the limit must leave room for an oversize sentinel byte",
            });
        }
        if self.max_header_bytes.checked_add(1).is_none() {
            return Err(OriginError::InvalidLimit {
                name: "max_header_bytes",
                value: self.max_header_bytes,
                reason: "the limit is too large for bounded header probing",
            });
        }
        Ok(())
    }
}

/// Errors returned while probing or reading untrusted Origin projects.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OriginError {
    /// The leading bytes do not match an Origin project family.
    #[error("the file is not a recognized Origin project")]
    UnrecognizedFormat,

    /// Input ended before a required bounded field was complete.
    #[error("Origin project data is truncated at byte {offset}: need {needed} bytes, have {have}")]
    Truncated {
        /// Offset of the incomplete field.
        offset: usize,
        /// Bytes required at the offset.
        needed: usize,
        /// Bytes available at the offset.
        have: usize,
    },

    /// The first line cannot fit within the configured header bound.
    #[error("Origin project header exceeds the configured {limit}-byte limit")]
    HeaderTooLong {
        /// Configured maximum bytes, including LF.
        limit: usize,
    },

    /// The family signature is present but the version line is invalid.
    #[error("malformed Origin project header: {detail}")]
    MalformedHeader {
        /// User-safe description of the failed grammar rule.
        detail: String,
    },

    /// A valid classic header names a producer profile without verified rules.
    #[error("Origin project version {raw_version} is not supported")]
    UnsupportedVersion {
        /// Version and build text from the validated header.
        raw_version: String,
    },

    /// OPJU was recognized, but this release intentionally does not decode it.
    #[error("{message}")]
    UnsupportedOpjuVariant {
        /// Plain user-safe explanation that no data was imported.
        message: String,
    },

    /// A caller supplied a limit that cannot be applied safely.
    #[error("invalid Origin limit {name}={value}: {reason}")]
    InvalidLimit {
        /// Public field name of the invalid limit.
        name: &'static str,
        /// Invalid value.
        value: usize,
        /// Stable reason the value cannot be used.
        reason: &'static str,
    },

    /// A verified resource count exceeds its configured bound.
    #[error("Origin import {resource} is {actual}, exceeding the configured limit of {limit}")]
    LimitExceeded {
        /// Resource being bounded.
        resource: &'static str,
        /// Configured maximum.
        limit: usize,
        /// Observed or requested amount.
        actual: usize,
    },

    /// Checked arithmetic could not represent a derived size or offset.
    #[error("Origin import size calculation overflowed for {resource}")]
    ArithmeticOverflow {
        /// Resource whose calculation overflowed.
        resource: &'static str,
    },

    /// A bounded allocation could not be reserved.
    #[error("Origin import could not reserve {requested} bytes for {resource}")]
    AllocationFailed {
        /// Allocation purpose.
        resource: &'static str,
        /// Requested capacity in bytes.
        requested: usize,
    },

    /// Validated framing contains an impossible or unsupported structure.
    #[error("corrupt Origin project structure at byte {offset}: {detail}")]
    CorruptStructure {
        /// Source byte offset.
        offset: usize,
        /// User-safe structural description.
        detail: String,
    },

    /// A value uses an encoding that cannot be decoded without guessing.
    #[error("unsupported Origin text encoding at byte {offset}: {encoding}")]
    UnsupportedEncoding {
        /// Source byte offset.
        offset: usize,
        /// Source encoding description.
        encoding: String,
    },

    /// A structurally valid project uses an unimplemented independent feature.
    #[error("unsupported Origin project feature: {feature}")]
    UnsupportedFeature {
        /// User-safe feature description.
        feature: String,
    },

    /// Declared and decoded row geometry disagree.
    #[error("Origin column {column} declares {declared} rows but contains {decoded} decoded rows")]
    InconsistentRowCount {
        /// Source column name.
        column: String,
        /// Declared logical row count.
        declared: usize,
        /// Decoded row count.
        decoded: usize,
    },

    /// Parsing completed without any worksheet that can be imported.
    #[error("the Origin project contains no supported worksheet data")]
    NoSupportedWorksheet,
}

/// Detects an Origin family and exact supported profile from bounded content.
pub fn probe_origin(bytes: &[u8]) -> Result<OriginProbe, OriginError> {
    probe_origin_with_limit(bytes, DEFAULT_MAX_HEADER_BYTES)
}

/// Reads a complete Origin project under explicit resource limits.
pub fn read_origin(bytes: &[u8], limits: OriginLimits) -> Result<OriginProject, OriginError> {
    limits.validate()?;
    enforce_limit("input bytes", bytes.len(), limits.max_input_bytes)?;
    enforce_limit(
        "total owned bytes",
        bytes.len(),
        limits.max_total_owned_bytes,
    )?;

    let probe = probe_origin_with_limit(bytes, limits.max_header_bytes)?;
    match probe.format {
        OriginFormat::Opju => opju::read(probe),
        OriginFormat::Opj => opj::read(bytes, &limits, probe),
    }
}

fn enforce_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), OriginError> {
    if actual > limit {
        return Err(OriginError::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

fn probe_origin_with_limit(
    bytes: &[u8],
    max_header_bytes: usize,
) -> Result<OriginProbe, OriginError> {
    let format = identify_family(bytes)?;
    let line = bounded_first_line(bytes, max_header_bytes)?;
    if !line.iter().all(|byte| matches!(byte, b' '..=b'~')) {
        return malformed("the version line must contain printable ASCII only");
    }

    let line = std::str::from_utf8(line)
        .map_err(|_| malformed_error("the version line must contain printable ASCII only"))?;
    let raw_version = match format {
        OriginFormat::Opj => line
            .strip_prefix("CPYA ")
            .and_then(|value| value.strip_suffix('#'))
            .ok_or_else(|| {
                malformed_error("classic OPJ headers must end with exactly one # before LF")
            })?,
        OriginFormat::Opju => line.strip_prefix("CPYUA ").ok_or_else(|| {
            malformed_error("OPJU headers must contain one space after the CPYUA signature")
        })?,
    };
    let version = parse_version(raw_version)?;
    let raw_version = copy_header_version(raw_version)?;

    match format {
        OriginFormat::Opj if raw_version != ORIGIN_7_V552_VERSION => {
            Err(OriginError::UnsupportedVersion { raw_version })
        }
        OriginFormat::Opj => Ok(OriginProbe {
            format,
            raw_version,
            version,
            byte_order: OriginByteOrder::LittleEndian,
            profile: Some(OriginProfile::Origin7V552),
            support: OriginSupport::Supported,
        }),
        OriginFormat::Opju => Ok(OriginProbe {
            format,
            raw_version,
            version,
            byte_order: OriginByteOrder::LittleEndian,
            profile: None,
            support: OriginSupport::RecognizedUnsupported,
        }),
    }
}

fn identify_family(bytes: &[u8]) -> Result<OriginFormat, OriginError> {
    if bytes.starts_with(OPJU_MAGIC) {
        return Ok(OriginFormat::Opju);
    }
    if bytes.starts_with(OPJ_MAGIC) {
        return Ok(OriginFormat::Opj);
    }

    let needed = if OPJ_MAGIC.starts_with(bytes) {
        Some(OPJ_MAGIC.len())
    } else if OPJU_MAGIC.starts_with(bytes) {
        Some(OPJU_MAGIC.len())
    } else {
        None
    };
    match needed {
        Some(needed) => Err(OriginError::Truncated {
            offset: 0,
            needed,
            have: bytes.len(),
        }),
        None => Err(OriginError::UnrecognizedFormat),
    }
}

fn bounded_first_line(bytes: &[u8], max_header_bytes: usize) -> Result<&[u8], OriginError> {
    let scan_len = bytes.len().min(max_header_bytes);
    let scan = bytes.get(..scan_len).ok_or(OriginError::Truncated {
        offset: 0,
        needed: scan_len,
        have: bytes.len(),
    })?;
    if let Some(newline) = scan.iter().position(|byte| *byte == b'\n') {
        return scan.get(..newline).ok_or(OriginError::Truncated {
            offset: 0,
            needed: newline,
            have: scan.len(),
        });
    }
    if bytes.len() >= max_header_bytes {
        return Err(OriginError::HeaderTooLong {
            limit: max_header_bytes,
        });
    }
    Err(OriginError::Truncated {
        offset: bytes.len(),
        needed: 1,
        have: 0,
    })
}

fn parse_version(raw_version: &str) -> Result<OriginHeaderVersion, OriginError> {
    let mut fields = raw_version.split(' ');
    let version = fields.next().unwrap_or_default();
    let build = fields.next().unwrap_or_default();
    if version.is_empty() || build.is_empty() || fields.next().is_some() {
        return malformed("the header must contain one version token and one numeric build field");
    }

    let (major, minor) = version
        .split_once('.')
        .ok_or_else(|| malformed_error("the version token must contain major.minor integers"))?;
    if !is_ascii_digits(major)
        || !is_ascii_digits(minor)
        || minor.contains('.')
        || !is_ascii_digits(build)
    {
        return malformed("the version and build fields must contain decimal digits only");
    }

    let major = major
        .parse::<u16>()
        .map_err(|_| malformed_error("the major version is outside the supported integer range"))?;
    let minor = minor
        .parse::<u16>()
        .map_err(|_| malformed_error("the minor version is outside the supported integer range"))?;
    let build = build
        .parse::<u32>()
        .map_err(|_| malformed_error("the build is outside the supported integer range"))?;
    Ok(OriginHeaderVersion {
        major,
        minor,
        build,
    })
}

fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn copy_header_version(raw_version: &str) -> Result<String, OriginError> {
    let requested = raw_version.len();
    let mut owned = String::new();
    owned
        .try_reserve_exact(requested)
        .map_err(|_| OriginError::AllocationFailed {
            resource: "header version text",
            requested,
        })?;
    owned.push_str(raw_version);
    Ok(owned)
}

fn malformed<T>(detail: &str) -> Result<T, OriginError> {
    Err(malformed_error(detail))
}

fn malformed_error(detail: &str) -> OriginError {
    OriginError::MalformedHeader {
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
#[path = "origin/reader_tests.rs"]
mod reader_tests;

#[cfg(test)]
#[path = "origin/tests.rs"]
mod tests;
