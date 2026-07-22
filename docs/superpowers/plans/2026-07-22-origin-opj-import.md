# Origin OPJ Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bounded, signature-driven Origin project importer that reliably imports the worksheet data proven by the public Origin 7.0552 OPJ fixture, recognizes OPJU and rejects it clearly, integrates with PlotX's existing table preview, and documents the exact experimental support boundary.

**Architecture:** plotx-io owns format probing and an engine-neutral Origin model; plotx-core consumes that model into typed table snapshots; the PlotX application opens an Origin path once, reuses the same regular-file handle for bounded classification and import, and then reuses the existing preview and commit flow. The OPJ parser is an idiomatic Rust reimplementation of the MIT-licensed OpenOPJ record descriptions with attribution. Liborigin source is not copied, translated, or linked; a separate executable was used only for correlated behavioral comparison because OpenOPJ acknowledges shared public reverse-engineering lineage. OPJU remains detection-only until a complete outer container profile can replace heuristic marker scanning.

**Tech Stack:** Rust 2024 workspace, thiserror, PlotX snapshot APIs, egui file-dialog flow, Rust unit and integration tests, Astro/Starlight documentation, GitHub CLI.

---

## Compatibility Decisions

- Successful import is limited to the exact Origin7V552 profile selected by the public OpenOPJ fixture signature and structural invariants.
- Supported real-fixture-backed OPJ cells are f64, f32, signed i32, signed i16, fixed-width ASCII text, mixed f64/text, nulls, and nonzero first-row offsets.
- Basic project parameters and notes are retained as source metadata where their framed records validate.
- Unsupported records are skipped only when a validated outer length makes them independent of decoded table geometry; otherwise the file is rejected.
- OPJU is recognized from CPYUA bytes and always returns UnsupportedOpjuVariant in this release. No byte-marker scanner or FPC decoder is enabled.
- The application adds only a Unix-targeted direct `libc` dependency for `O_NONBLOCK` file opening; it is MIT or Apache-2.0 licensed and was already in the lockfile. `encoding_rs` is not added to `plotx-io` because no verified code-page field exists for the supported profile.
- Every production allocation and offset derived from input is checked against OriginLimits before allocation or slicing.

## Task 1: Add Publicly Redistributable Fixtures

**Files:**

- Modify: .gitattributes
- Create: crates/io/tests/fixtures/origin/README.md
- Create: crates/io/tests/fixtures/origin/OPENOPJ-LICENSE.txt
- Create: crates/io/tests/fixtures/origin/test-origin-7.0552.opj
- Create: crates/io/tests/fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju

- [x] **Step 1: Mark Origin fixtures as binary**

Add:

~~~gitattributes
*.opj binary
*.opju binary
~~~

- [x] **Step 2: Download only the licensed public fixtures**

Run from the repository root:

~~~bash
curl -fL https://raw.githubusercontent.com/jgonera/openopj/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/support/test.opj \
  -o crates/io/tests/fixtures/origin/test-origin-7.0552.opj
curl -fL https://ndownloader.figshare.com/files/52794059 \
  -o crates/io/tests/fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju
~~~

Expected: both downloads succeed. Do not add any locally discovered or private Origin file.

- [x] **Step 3: Verify immutable fixture identity**

Run:

~~~bash
wc -c crates/io/tests/fixtures/origin/test-origin-7.0552.opj \
  crates/io/tests/fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju
shasum -a 256 crates/io/tests/fixtures/origin/test-origin-7.0552.opj \
  crates/io/tests/fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju
~~~

Expected:

~~~text
282034 test-origin-7.0552.opj
64954 RawData_Locust_Revision1_TIS_Mechanism.opju
ac7f71c367562e85e9d4bb4ae418cbcaaa1b5dff80436180e8d3331c7e1d6308
13c47a6a5daaf14493da59c8f1b284d9efb08129c8320b6ad9fd0b5191faa55f
~~~

- [x] **Step 4: Document provenance and license**

README.md must state source URL, original filename, byte length, SHA-256, license, attribution, and that neither fixture contains PlotX user data. Copy OpenOPJ's MIT license text to OPENOPJ-LICENSE.txt. Cite the Figshare DOI and CC BY 4.0 terms without adding an Origin logo.

As built, `xtask/about.hbs` also carries a static OpenOPJ notice with the complete MIT text, so generated `dist/THIRD-PARTY-LICENSES.html` includes the non-Cargo attribution. An `xtask` unit test compares the embedded notice with `OPENOPJ-LICENSE.txt` to prevent drift.

- [x] **Step 5: Verify the fixture diff**

Run:

~~~bash
git diff --check
git status --short
git check-attr diff -- crates/io/tests/fixtures/origin/test-origin-7.0552.opj
~~~

Expected: no whitespace errors, only intended files, and the OPJ fixture has binary diff handling.

- [x] **Step 6: Commit**

~~~bash
git add .gitattributes crates/io/tests/fixtures/origin
git commit -m "test: add licensed Origin project fixtures"
~~~

## Task 2: Define the Origin Model, Probe, Limits, and Fail-Closed OPJU Path

**Files:**

- Modify: crates/io/src/lib.rs
- Create: crates/io/src/origin.rs
- Create: crates/io/src/origin/opju.rs
- Create: crates/io/src/origin/tests.rs

- [x] **Step 1: Write failing public-behavior tests**

Add tests that establish these outcomes before production code exists:

~~~rust
#[test]
fn probes_opj_and_opju_by_content() {
    assert_eq!(
        probe_origin(b"CPYA 4.2673 552#\n").unwrap().format,
        OriginFormat::Opj
    );
    assert_eq!(
        probe_origin(b"CPYUA 4.3668 178\n").unwrap().format,
        OriginFormat::Opju
    );
}

#[test]
fn opju_is_recognized_but_not_partially_imported() {
    let error = read_origin(b"CPYUA 4.3668 178\nrest", OriginLimits::default())
        .unwrap_err();
    assert!(matches!(error, OriginError::UnsupportedOpjuVariant { .. }));
}

#[test]
fn rejects_unknown_or_truncated_headers() {
    assert!(matches!(
        probe_origin(b"CP"),
        Err(OriginError::Truncated { .. })
    ));
    assert!(matches!(
        probe_origin(b"not an origin file"),
        Err(OriginError::UnrecognizedFormat)
    ));
}
~~~

Also test a malformed version line, a header longer than 128 bytes, an input one byte over max_input_bytes, and the actual OPJU grammar. OPJU tests reject a missing version token, a missing numeric build field, an extra # before LF, and nonnumeric version or build fields.

Wire the tests into origin.rs so the focused command cannot silently run zero tests:

~~~rust
#[cfg(test)]
#[path = "origin/tests.rs"]
mod tests;
~~~

- [x] **Step 2: Run tests and observe RED**

~~~bash
cargo test -p plotx-io --lib origin::tests
~~~

Expected: compilation fails because plotx_io::origin and its public types do not exist. Record the failure in the task handoff.

- [x] **Step 3: Implement the minimal public model**

Define:

~~~rust
pub enum OriginFormat { Opj, Opju }
pub enum OriginSupport { Supported, RecognizedUnsupported }
pub enum OriginCell { Null, Float(f64), Integer(i64), Text(String) }
pub enum OriginColumnType { Float, Integer, Text, Mixed }

pub enum OriginByteOrder { LittleEndian }

pub struct OriginHeaderVersion {
    pub major: u16,
    pub minor: u16,
    pub build: u32,
}

pub struct OriginProbe {
    pub format: OriginFormat,
    pub raw_version: String,
    pub version: OriginHeaderVersion,
    pub byte_order: OriginByteOrder,
    pub profile: Option<OriginProfile>,
    pub support: OriginSupport,
}

pub struct OriginLimits {
    pub max_input_bytes: usize,
    pub max_header_bytes: usize,
    pub max_block_bytes: usize,
    pub max_string_bytes: usize,
    pub max_decoded_text_bytes: usize,
    pub max_parser_bytes: usize,
    pub max_total_owned_bytes: usize,
    pub max_workbooks: usize,
    pub max_window_records: usize,
    pub max_worksheets_per_workbook: usize,
    pub max_columns: usize,
    pub max_metadata_records: usize,
    pub max_rows_per_column: usize,
    pub max_cells: usize,
    pub max_metadata_depth: usize,
}
~~~

Add OriginProject with a public probe: OriginProbe field, plus OriginWorkbook, OriginWorksheet, OriginColumn, OriginDiagnostic, OriginResourceUsage, and stable thiserror variants matching the design. Public APIs:

~~~rust
pub fn probe_origin(bytes: &[u8]) -> Result<OriginProbe, OriginError>;
pub fn read_origin(
    bytes: &[u8],
    limits: OriginLimits,
) -> Result<OriginProject, OriginError>;
~~~

The default limits must equal the design values. Keep all fields engine-neutral and independent of UI and DataFusion types.

- [x] **Step 4: Implement bounded signature probing**

Parse only a bounded first line. Require CPYA for OPJ or CPYUA for OPJU, a terminating LF, printable ASCII version text, and the profile-specific terminator. Classic OPJ uses the verified CPYA 4.2673 552# LF grammar; the public OPJU regression fixture uses CPYUA 4.3668 178 LF with no #. Never select a format from an extension.

Origin 7.0552 maps to OriginProfile::Origin7V552. Other CPYA versions return UnsupportedVersion. CPYUA dispatches to opju.rs, which validates the family header and then returns UnsupportedOpjuVariant with a user-safe message.

- [x] **Step 5: Run tests and observe GREEN**

~~~bash
cargo test -p plotx-io --lib origin::tests
cargo check -p plotx-io --locked
~~~

Expected: all new probe and OPJU tests pass; the test runner reports the named tests above rather than zero tests; plotx-io compiles without a new dependency.

- [x] **Step 6: Inspect public documentation and line counts**

~~~bash
cargo fmt --check
cargo doc -p plotx-io --no-deps
wc -l crates/io/src/origin.rs crates/io/src/origin/opju.rs crates/io/src/origin/tests.rs
~~~

Expected: public APIs have useful rustdoc and every Rust file is under 800 lines.

- [x] **Step 7: Commit**

~~~bash
git add crates/io/src/lib.rs crates/io/src/origin.rs crates/io/src/origin
git commit -m "feat(io): detect Origin project formats"
~~~

## Task 3: Build the Checked OPJ Reader and Exact Origin7V552 Framing

**Files:**

- Create: crates/io/src/origin/reader.rs
- Create: crates/io/src/origin/reader_tests.rs
- Create: crates/io/src/origin/opj.rs
- Extend: crates/io/src/origin/tests.rs

- [x] **Step 1: Write reader tests before implementation**

Cover:

- checked little-endian u16, u32, i16, i32, f32, and f64 reads;
- checked slice reads at exact end and one byte past end;
- checked addition and multiplication overflow;
- framed blocks shaped as little-endian u32 length, LF, payload, LF;
- the null block shaped as zero u32 followed by LF;
- invalid delimiters;
- a declared block larger than max_block_bytes;
- parser and text allocation budgets being charged before reserve;
- every truncated prefix of a minimal valid block returning an error without panic;
- ASCII strings with fixed-width NUL padding;
- non-ASCII cell text returning UnsupportedEncoding.

Wire reader_tests.rs from origin.rs before running RED:

~~~rust
#[cfg(test)]
#[path = "origin/reader_tests.rs"]
mod reader_tests;
~~~

- [x] **Step 2: Run tests and observe RED**

~~~bash
cargo test -p plotx-io --lib reader_tests
~~~

Expected: the named reader tests are discovered and fail to compile because the checked reader does not exist.

- [x] **Step 3: Implement the checked reader**

The reader owns an immutable byte slice, current offset, OriginLimits reference, and OriginResourceUsage. All methods return Result and include the current offset in structural errors. It must use checked slices and checked arithmetic. It must not use unsafe, unwrap, or unchecked indexing on external data.

Preflight requested Vec and String capacities against the parser and cumulative budgets before allocation. Incrementally retained metadata vectors grow geometrically within the remaining budget, then charge the actual capacity delta reported after reserve; allocator rounding that crosses a budget fails closed. The parser also rejects more than 1,024 retained source windows before any dataset-to-window association.

- [x] **Step 4: Write profile-framing tests**

Construct synthetic bytes using test-only helpers. Assert:

~~~rust
#[test]
fn accepts_only_exact_origin_7_v552_header_and_framing() {
    let bytes = minimal_origin7_v552_project();
    let probe = probe_origin(&bytes).unwrap();
    assert_eq!(probe.profile, Some(OriginProfile::Origin7V552));
}
~~~

Mutate producer version, header terminator, first block delimiter, declared length, and final delimiter separately. Assert a specific UnsupportedVersion or CorruptStructure category.

- [x] **Step 5: Run the profile tests and observe RED**

~~~bash
cargo test -p plotx-io --lib origin::tests::origin7_profile
~~~

Expected: the probe passes but read_origin fails because OPJ framing is not implemented.

- [x] **Step 6: Implement exact OPJ block traversal**

Implement only the top-level grammar needed by Origin7V552. Comments must cite the relevant OpenOPJ MIT source or documentation file and explain the checked boundary. Unknown length-framed blocks may be retained as UnsupportedObjectSummary entries only where skipping cannot alter later record alignment.

- [x] **Step 7: Run focused tests**

~~~bash
cargo test -p plotx-io --lib reader_tests
cargo test -p plotx-io --lib origin::tests::origin7_profile
cargo clippy -p plotx-io --all-targets -- -D warnings
~~~

Expected: all pass.

- [x] **Step 8: Commit**

~~~bash
git add crates/io/src/origin
git commit -m "feat(io): add bounded OPJ block reader"
~~~

## Task 4: Decode Real-Fixture-Backed OPJ Column Records

**Files:**

- Create: crates/io/src/origin/opj/records.rs
- Create: crates/io/src/origin/opj/records_tests.rs
- Modify: crates/io/src/origin/opj.rs

- [x] **Step 1: Write synthetic record tests**

Build test records using the verified Origin7V552 offsets:

- type at 0x16 as u16;
- secondary type at 0x18;
- total rows at 0x19;
- first row at 0x1d;
- last row at 0x21;
- value width at 0x3d;
- unsigned flag field at 0x3f;
- dataset name at 0x58 with a 25-byte bounded region;
- tertiary type at 0x71.

Assert exact decoding for:

~~~rust
vec![OriginCell::Float(0.4), OriginCell::Null]
vec![OriginCell::Float(345.600006103515625)]
vec![OriginCell::Integer(345), OriginCell::Integer(-100000)]
vec![OriginCell::Integer(34), OriginCell::Integer(-1000)]
vec![OriginCell::Text("test string 123".into())]
vec![OriginCell::Text("text".into()), OriginCell::Float(3.14)]
~~~

The f64 value -1.23456789E-300 maps to OriginCell::Null. The public fixture proves
that `lastRow` is an exclusive end index and that payload slots before `firstRow`
already contain the missing-value sentinel. Decode `[0, lastRow)` and validate
`[0, firstRow)` as null; do not prepend synthetic nulls or skip those payload slots.

- [x] **Step 2: Add negative record tests**

Test width/type mismatches, geometry that violates
`firstRow <= lastRow <= totalRows`, negative or overflowing geometry, incomplete
fixed text, mixed-cell prefixes other than `[0, 0]` or `[1, 0]`, truncated f64
after a numeric mixed prefix, non-ASCII text, unsupported 8-bit integer, and
unverified unsigned flags. Require the content block length to equal
`totalRows * valueWidth` exactly.

For every minimal valid record, run every truncated prefix inside catch_unwind and assert no panic plus a structured error.

Wire records_tests.rs from records.rs with:

~~~rust
#[cfg(test)]
#[path = "records_tests.rs"]
mod records_tests;
~~~

- [x] **Step 3: Run tests and observe RED**

~~~bash
cargo test -p plotx-io --lib records_tests
~~~

Expected: the named record tests are discovered and compilation fails because record decoding functions do not exist.

- [x] **Step 4: Implement only evidence-backed decoders**

Map:

- width 8 numeric to f64;
- width 4 floating to f32 converted losslessly to f64;
- width 4 signed integer to i32 then i64;
- width 2 signed integer to i16 then i64;
- fixed-width ASCII to text after trimming in-field NUL padding;
- mixed prefix 0 to the bounded f64 payload and prefix 1 to bounded ASCII text.

Reject rather than infer any type combination not represented by the public fixture. Never reinterpret malformed text bytes as numbers.

- [x] **Step 5: Run focused tests and static checks**

~~~bash
cargo test -p plotx-io --lib records_tests
cargo test -p plotx-io --lib origin::tests
cargo clippy -p plotx-io --all-targets -- -D warnings
rg -n "unwrap\(|unsafe\s*\{" crates/io/src/origin
~~~

Expected: tests and Clippy pass. The source scan has no production external-input unwrap or unsafe block.

- [x] **Step 6: Commit**

~~~bash
git add crates/io/src/origin
git commit -m "feat(io): decode supported OPJ worksheet columns"
~~~

## Task 5: Assemble Workbooks, Metadata, and Verify Real Fixtures

**Files:**

- Create: crates/io/src/origin/opj/metadata.rs
- Create: crates/io/src/origin/opj/metadata_tests.rs
- Modify: crates/io/src/origin/opj.rs
- Create: crates/io/tests/origin_fixtures.rs

- [x] **Step 1: Write real OPJ fixture assertions first**

Wire metadata_tests.rs from metadata.rs:

~~~rust
#[cfg(test)]
#[path = "metadata_tests.rs"]
mod metadata_tests;
~~~

The public integration test file is discovered automatically from crates/io/tests.

The integration test must call only public plotx-io APIs and assert concrete values published with the OpenOPJ fixture:

~~~rust
#[test]
fn imports_openopj_origin_7_v552_fixture() {
    let bytes = include_bytes!("fixtures/origin/test-origin-7.0552.opj");
    let project = read_origin(bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.probe.profile, Some(OriginProfile::Origin7V552));
    assert_eq!(cell(&project, "Data1", "INJV", 0), Some(&OriginCell::Float(0.4)));
    assert_eq!(cell(&project, "Data1", "INJV", 19), Some(&OriginCell::Float(2.0)));
    assert_eq!(cell(&project, "Data1", "INJV", 20), Some(&OriginCell::Null));
    assert_eq!(
        cell(&project, "TestW", "TextNumeric", 0),
        Some(&OriginCell::Text("text".into()))
    );
    assert_eq!(
        cell(&project, "TestW", "TextNumeric", 1),
        Some(&OriginCell::Float(3.14))
    );
}
~~~

Define cell as a private integration-test iterator helper rather than adding a public convenience API solely for tests. Also assert f32 approximately 345.60001 and -100000.20313, i32 values 345 and -100000, i16 values 34 and -1000, text values "test string 123" and "only text", first-row null/5.23/-7 behavior, parameters ERR=1, SYRNG_C_DATA1=1.25, CELL_C_DATA1=.1246, S=1.28889201142965, and the Results note content.

- [x] **Step 2: Write OPJU fixture rejection first**

~~~rust
#[test]
fn recognizes_public_opju_fixture_without_partial_output() {
    let bytes = include_bytes!(
        "fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju"
    );
    let probe = probe_origin(bytes).unwrap();
    assert_eq!(probe.format, OriginFormat::Opju);
    assert!(matches!(
        read_origin(bytes, OriginLimits::default()),
        Err(OriginError::UnsupportedOpjuVariant { .. })
    ));
}
~~~

- [x] **Step 3: Run tests and observe RED**

~~~bash
cargo test -p plotx-io --lib metadata_tests
cargo test -p plotx-io --test origin_fixtures
~~~

Expected: the named metadata unit tests are discovered and fail because metadata parsing is absent; OPJU integration rejection passes; OPJ integration assertions fail because workbook assembly and metadata traversal are incomplete.

- [x] **Step 4: Implement window and dataset association**

Reimplement the MIT OpenOPJ Origin 7.0552 window traversal in Rust with a source citation. Dataset names use the validated window or group prefix and column suffix. Choose the longest validated matching prefix. Each associated group uses one generated `Sheet1` worksheet because this profile does not decode a separately verified source worksheet label. If no unambiguous association exists, create a deterministic fallback group name and emit a stable warning; never attach a column to a nearby name by byte proximity.

Enforce workbook, worksheet, column, row, cell, decoded-text, parser-allocation,
cumulative metadata-record, and nesting limits while assembling. Metadata records
have their own 65,536-record default budget and do not consume the data-column
limit; every logical record is charged before it can be retained or safely skipped.

- [x] **Step 5: Implement framed parameters and notes**

Parse only validated parameter lines and note blocks needed by the public fixture. Retain bounded key/value strings and note names/content. Unsupported note properties and project objects become diagnostics, not executable content. Non-ASCII independent metadata may be skipped with a warning only if the next record boundary is already validated.

- [x] **Step 6: Run real and adversarial tests**

~~~bash
cargo test -p plotx-io --test origin_fixtures -- --nocapture
cargo test -p plotx-io --lib
cargo clippy -p plotx-io --all-targets -- -D warnings
~~~

Expected: concrete OPJ values and metadata pass; OPJU remains a clear error; all synthetic corruption tests pass.

- [x] **Step 7: Commit**

~~~bash
git add crates/io/src/origin crates/io/tests/origin_fixtures.rs
git commit -m "feat(io): import Origin 7.0552 OPJ worksheets"
~~~

## Task 6: Convert Origin Worksheets into PlotX Typed Snapshots

**Files:**

- Modify: crates/core/src/lib.rs
- Create: crates/core/src/origin.rs
- Create: crates/core/src/origin_tests.rs

- [x] **Step 1: Write core conversion tests before production code**

Build a small OriginProject in memory and assert:

- one preview candidate per nonempty worksheet;
- f64 and f32 source columns become Float64;
- signed integer source columns become Int64;
- fixed text becomes Utf8;
- mixed numeric/text becomes Utf8 with Rust round-trip numeric strings;
- shorter columns are padded to worksheet row_count with invalid validity bits;
- empty names become Column 1, Column 2;
- duplicate names become name, name (2), name (3);
- the original source names, project format, producer version, parameters, notes, and parser diagnostics remain in bounded source metadata;
- an empty project returns NoSupportedWorksheet rather than an empty preview;
- checked snapshot-capacity estimates reject a project exceeding max_total_owned_bytes before allocation.

A representative assertion:

~~~rust
#[test]
fn converts_mixed_cells_without_dropping_numbers() {
    let imported = import_origin_project(
        project_with_mixed_column(),
        &store(),
        &codecs(),
        OriginLimits::default(),
    )
    .unwrap();

    assert_eq!(imported.len(), 1);
    assert_eq!(
        imported[0].snapshot.schema.columns[0].logical_type,
        LogicalType::Utf8
    );
    assert_eq!(utf8_values(&imported[0].snapshot, 0), ["text", "3.14"]);
}
~~~

Wire the sibling test file from crates/core/src/lib.rs:

~~~rust
#[cfg(test)]
#[path = "origin_tests.rs"]
mod origin_tests;
~~~

- [x] **Step 2: Run tests and observe RED**

~~~bash
cargo test -p plotx-core --lib origin_tests
~~~

Expected: the named core Origin tests are discovered and compilation fails because import_origin_project and its result/error types do not exist.

- [x] **Step 3: Implement deterministic schema conversion**

Use ColumnSchema, TableSchema, SnapshotBuilder, ColumnChunk, ColumnValues, and Validity directly. Process rows in chunks of 65,536 as the existing XLSX importer does. Consume OriginProject by value and drain cell storage rather than cloning it.

Expose:

~~~rust
pub const ORIGIN_IMPORT_OPERATION: &str = "plotx.import.origin.v1";

pub fn import_origin_project(
    project: OriginProject,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
    limits: OriginLimits,
) -> Result<Vec<ImportedOriginWorksheet>, OriginImportError>;
~~~

ImportedOriginWorksheet contains the candidate label, TableSnapshot, bounded source metadata, and parser diagnostics. The application creates TypedTableState with imported_with_operation and constructs TableImportSource with media type application/x-origin-project, the selected filename, the shared source bytes, and this metadata.

- [x] **Step 4: Handle names, nulls, metadata, and resource accounting**

Name normalization is deterministic and case-sensitive. Preserve every changed original name in metadata. Do not set ColumnSchema.unit for text columns. Notes and parameters remain source metadata and are never converted into table cells.

Use checked arithmetic to estimate validity buffers, numeric arrays, string offsets, and UTF-8 data capacity before constructing each batch. Carry OriginResourceUsage forward and reject the total owned-byte budget before reserve.

- [x] **Step 5: Run both backend configurations**

~~~bash
cargo test -p plotx-core --lib origin_tests
cargo check -p plotx-core --locked
cargo check -p plotx-core --features datafusion --locked
cargo clippy -p plotx-core --all-targets -- -D warnings
~~~

Expected: all pass and plotx-data has no dependency change.

- [x] **Step 6: Commit**

~~~bash
git add crates/core/src/lib.rs crates/core/src/origin.rs crates/core/src/origin_tests.rs
git commit -m "feat(core): convert Origin worksheets to tables"
~~~

## Task 7: Integrate Origin with File Open, Preview, Recent Files, and Errors

**Files:**

- Modify: crates/app/src/ui/file_dialogs.rs
- Create: crates/app/src/ui/file_dialogs/origin.rs
- Create: crates/app/src/ui/file_dialogs/origin_tests.rs
- Modify: crates/app/src/ui/file_dialogs/recent.rs
- Modify: crates/app/src/ui/commands/identity.rs
- Modify: crates/app/src/ui/file_dialogs/preview.rs
- Modify: crates/app/src/ui/shortcuts.rs
- Modify: crates/app/src/ui/canvas/mod.rs

- [x] **Step 1: Write routing and user-error tests before implementation**

Cover:

- Import Table accepts .opj and .opju in its filter while retaining CSV, TSV, TXT, and XLSX;
- Open File accepts Origin projects and routes through the same content-probing path as recent files and drag/drop;
- extension OPJ with non-Origin bytes returns a signature-mismatch error;
- valid CPYA bytes under a different extension are identified as Origin by content;
- OPJU becomes a user-facing unsupported message and creates neither a preview nor a recent entry;
- an input of exactly 128 MiB is accepted by the bounded reader and 128 MiB plus one byte is rejected;
- a custom OriginLimits with max_input_bytes equal to usize::MAX returns InvalidLimit without overflow or panic;
- one selected OPJ path is opened once, the same regular-file handle is reused from classification through the bounded read, and the resulting `Arc<[u8]>` is shared by all worksheet candidates;
- no recent entry is added until the user confirms an imported candidate;
- parser/core failures become OperationReport failure entries;
- a project with zero supported candidates returns an error without indexing an empty vector.

Wire origin_tests.rs from origin.rs:

~~~rust
#[cfg(test)]
#[path = "origin_tests.rs"]
mod origin_tests;
~~~

After GREEN, list the tests with cargo test -p plotx origin -- --list and verify the expected names appear.

- [x] **Step 2: Run tests and observe RED**

~~~bash
cargo test -p plotx origin -- --nocapture
~~~

Expected: tests fail because Origin routing and the recent-file variant do not exist.

- [x] **Step 3: Add a focused application adapter**

The new origin.rs module, together with the shared classifier:

1. treats path metadata only as an early directory or file hint, then opens an apparent regular file once;
2. uses `O_NONBLOCK` on Unix, validates the opened handle itself as a regular file, and retains its size only as an early rejection hint;
3. reads the classification header from that handle and transfers the same handle to the Origin adapter without reopening the path;
4. selects the smaller of max_input_bytes and max_total_owned_bytes, computes its one-byte sentinel with checked arithmetic, converts the limit to u64 for the metadata hint with a checked conversion, and returns InvalidLimit if either operation fails;
5. rewinds the retained handle and reads manually in fixed 16 KiB chunks without reserving the metadata length, using fallible exact reservation and checking actual Vec capacity against the total-owned budget;
6. rejects an extra byte as too large, checks Vec capacity plus final length as the source-payload peak, and converts the retained Vec once into Arc<[u8]>;
7. calls plotx_io::origin::probe_origin and read_origin;
8. calls plotx_core::origin::import_origin_project;
9. creates TableImportSource values that share the one Arc slice and then creates the existing TableImportPreviewState;
10. passes warnings and errors to normal application feedback.

Keep parsing out of file_dialogs.rs. Do not add a new command ID; reuse CommandId::ImportTable and keep stable machine ID file.import_table.

- [x] **Step 4: Extend filters and common routing**

Change the visible command label from Import Table / CSV… to Import Table…. Add Origin projects (experimental) to the import filter. Extend the recent enum with OriginProject for .opj and .opju fallback classification.

The shared classify_open_path helper first uses path metadata as an early hint and immediately returns Folder for a directory, preserving Bruker and folder workflows. For an apparent regular file, it opens the path once with `O_NONBLOCK` on Unix, rejects a non-regular opened handle using handle metadata (`fstat` on Unix), and reads at most 129 header bytes into a fixed small buffer before extension routing. If those bytes begin with CPYA or CPYUA, it validates them with probe_origin and transfers the still-open handle to the Origin adapter regardless of extension. If an .opj or .opju path lacks Origin magic, it still transfers the handle to the Origin adapter so the user receives a signature-mismatch error. All other headers fall back to the existing extension-based Project, DelimitedTable, XlsxTable, or DataFile route without reading the whole file. File picker, Open File, recent reopen, and drag/drop all call this helper.

Generalize preview copy from Worksheet and worksheet(s) to Table and table(s). The selector changes only which candidate is previewed; the summary explicitly says all candidate tables will be imported, matching the existing commit loop.

- [x] **Step 5: Verify the normal import lifecycle**

The preview must appear before table state changes. Confirming imports all candidates using TypedTableState::imported_with_operation with plotx.import.origin.v1 and only then records the path in recent files. Cancellation changes neither tables nor recent files.

- [x] **Step 6: Run focused and feature checks**

~~~bash
cargo test -p plotx origin -- --nocapture
cargo test -p plotx recent_entries_route_to_their_import_path
cargo check -p plotx --locked
cargo check -p plotx --features datafusion --locked
cargo clippy -p plotx --all-targets -- -D warnings
wc -l crates/app/src/ui/file_dialogs.rs \
  crates/app/src/ui/file_dialogs/origin.rs \
  crates/app/src/ui/file_dialogs/preview.rs
~~~

Expected: tests and checks pass, every Rust source file remains below 800 lines, and the existing default and DataFusion frontends compile.

- [x] **Step 7: Commit**

~~~bash
git add crates/app/src
git commit -m "feat(app): add experimental Origin project import"
~~~

## Task 8: Document the Exact English and Simplified Chinese Support Boundary

**Files:**

- Modify: docs/src/content/docs/guides/importing-data.md
- Modify: docs/src/content/docs/zh-cn/guides/importing-data.md
- Modify: docs/src/content/docs/reference/file-formats.md
- Modify: docs/src/content/docs/zh-cn/reference/file-formats.md

- [x] **Step 1: Confirm the paired pages still exist**

~~~bash
test -f docs/src/content/docs/guides/importing-data.md
test -f docs/src/content/docs/zh-cn/guides/importing-data.md
test -f docs/src/content/docs/reference/file-formats.md
test -f docs/src/content/docs/zh-cn/reference/file-formats.md
~~~

Expected: all four commands succeed. Edit these existing owners rather than creating duplicate navigation pages.

- [x] **Step 2: Update English documentation**

State all of these facts:

- picker extensions .opj and .opju, with content-based detection;
- successful import is experimental and limited to the tested Origin 7.0552 OPJ profile;
- imported cells: f64, f32, signed i32, signed i16, ASCII fixed text, mixed numeric/text, nulls, names, parameters, and notes as documented metadata;
- OPJU is recognized but not importable in this release;
- graphs, formulas, scripts, analysis recomputation, matrices, embedded objects, non-ASCII text, encryption, and unverified versions are unsupported;
- corrupt, oversized, mismatched, and unsupported files produce an error instead of partial or silent import;
- Origin need not be installed, launched, or called;
- compatibility claims follow the committed regression fixture and can expand only with evidence.

Use PlotX for human-readable product text and lowercase plotx only for machine identifiers or URLs.

- [x] **Step 3: Mirror the content in Simplified Chinese**

Keep claims and limitations aligned with English. Translate the user-facing error behavior plainly. Do not imply all OPJ files work and do not present OPJU as partially supported.

- [x] **Step 4: Build documentation**

Run from docs:

~~~bash
npm run build
~~~

Expected: Astro/Starlight build completes with no broken links or missing pages. Do not commit docs/dist or docs/node_modules.

- [x] **Step 5: Scan terminology**

~~~bash
rg -n "\bPlotx\b|full Origin|完全兼容|OPJU.*importable|OPJU.*可导入" docs/src
git status --short
~~~

Expected: no incorrect product spelling or exaggerated support claim, and no generated directory is staged.

- [x] **Step 6: Commit**

~~~bash
git add docs/src
git commit -m "docs: describe experimental Origin import"
~~~

## Task 9: Full Verification, Security Review, Branch Review, and Pull Request

**Files:**

- Review all files changed from upstream/main
- Update fixture README or docs only if verification reveals a factual mismatch

- [x] **Step 1: Run formatting and focused test suites**

~~~bash
cargo fmt --all -- --check
cargo test -p plotx-io
cargo test -p plotx-core origin
cargo test -p plotx origin
cargo check -p plotx --locked
cargo check -p plotx --features datafusion --locked
~~~

Expected: every command exits successfully.

- [x] **Step 2: Obtain permission for missing tools when needed**

Check:

~~~bash
command -v cargo-deny
command -v protoc
~~~

If either is absent, pause once and ask the user for installation permission with the exact proposed commands. Do not claim cargo pr-check passed while a prerequisite is missing. After permission, install the minimum required tools and report what changed.

- [x] **Step 3: Run the repository gate**

~~~bash
cargo pr-check
~~~

Expected: formatting, source-size, dependency license/advisory, default frontend, DataFusion frontend, Clippy, and both test configurations pass.

If a failure is caused by the implementation, use superpowers:systematic-debugging, add or refine a failing regression test, fix it, and rerun the failed command followed by cargo pr-check. If the failure is external and cannot be fixed without new authority, preserve the evidence and explain the blocker.

- [x] **Step 4: Rebuild documentation from a clean source state**

~~~bash
cd docs
npm run build
cd ..
git status --short
~~~

Expected: build passes and generated directories remain ignored and unstaged.

- [x] **Step 5: Perform the final hostile-input audit**

~~~bash
rg -n "unwrap\(|expect\(|unsafe\s*\{|read_to_end|with_capacity|reserve" \
  crates/io/src/origin crates/core/src/origin.rs \
  crates/app/src/ui/file_dialogs/origin.rs
rg -n "datafusion|sqlparser" crates/data/Cargo.toml crates/data/src
find crates -name "*.rs" -print0 | xargs -0 wc -l | sort -nr | head -20
~~~

Manually verify every match involving external bytes is bounded or locally invariant, all capacity requests are checked, no embedded path is opened, and no source file exceeds 800 lines.

- [x] **Step 6: Review the complete diff**

~~~bash
git diff --check upstream/main...HEAD
git diff --stat upstream/main...HEAD
git status --short
git log --oneline upstream/main..HEAD
~~~

Inspect every changed file. Confirm there are no credentials, private data, target, dist, docs/dist, docs/node_modules, unrelated edits, false compatibility claims, copied or translated liborigin source, or unsupported provenance or legal conclusions.

- [x] **Step 7: Run an independent final code review**

Use superpowers:requesting-code-review on upstream/main...HEAD. Resolve every critical or important finding through a failing test and a focused fix, then rerun the relevant focused tests and cargo pr-check.

- [ ] **Step 8: Push the branch**

~~~bash
git push -u origin codex/origin-project-import
~~~

Expected: branch appears on the user's fork at github.com/Limdongcheng/plotx.

- [ ] **Step 9: Write the evidence-backed pull request body**

Create /tmp/plotx-origin-pr.md with apply_patch after all verification results are known. The complete body must include:

- functional summary;
- architecture and license-compatible Rust reimplementation approach;
- exact OPJ Origin7V552 scope;
- OPJU detection-only behavior;
- known unsupported objects and encodings;
- hostile-input safeguards and limits;
- focused, backend, cargo pr-check, and docs results;
- both fixture sources, licenses, sizes, and SHA-256 values;
- follow-up path for additional OPJ profiles and proven OPJU containers.

Do not leave template markers or claim a check passed unless its successful output was observed.

- [ ] **Step 10: Create the upstream pull request**

Create a ready pull request to nmrtist/plotx:main with gh:

Run:

~~~bash
gh pr create --repo nmrtist/plotx --base main \
  --head Limdongcheng:codex/origin-project-import \
  --title "feat: import Origin 7.0552 OPJ worksheets" \
  --body-file /tmp/plotx-origin-pr.md
~~~

Expected: gh prints the upstream pull-request URL. Do not merge, release, or change repository settings.

## Plan Self-Review Checklist

- [x] Every design acceptance criterion maps to at least one implementation step and one verification step.
- [x] OPJ claims are limited to values present in the OpenOPJ real fixture.
- [x] OPJU has no success path, marker scan, decompressor, or partial project result.
- [x] The plan never instructs copying, translating, or linking liborigin source and characterizes executable comparison as correlated corroboration, not independent proof or a legal conclusion.
- [x] Every implementation task observes a failing test before production code.
- [x] Every task ends with focused verification and a small commit.
- [x] Default and DataFusion configurations are both checked.
- [x] User-visible errors, preview lifecycle, and recent-file timing are tested.
- [x] Fixture provenance and license terms are committed.
- [x] Installation, login, and destructive actions remain explicit permission boundaries.
- [x] The only new direct dependency is the Unix-only `libc` entry needed for `O_NONBLOCK`; it is MIT or Apache-2.0 licensed and was already present in the workspace lockfile. `encoding_rs` remains transitive and unused by `plotx-io`.
- [x] All Rust files stay below 800 lines.
