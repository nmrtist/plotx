# Origin Project Import Design

**Date:** 2026-07-22

**Status:** Approved after self-review

**Feature label:** Origin project import (experimental)

## Summary

PlotX will import worksheet data directly from supported Origin project files without installing, launching, or automating Origin. The first release will provide a native Rust importer for a verified older binary `.opj` profile. It will recognize `.opju` by content and return a clear unsupported-variant error, but it will not offer a successful OPJU import because the available public implementation does not prove complete container boundaries. Both paths will use file signatures and structural validation rather than trusting filename extensions.

The importer will recover workbook and worksheet structure, supported numeric and text cells, column names, and basic column metadata. It will not claim full Origin project compatibility. Graphs, formula execution, scripts, analysis recomputation, matrices, embedded objects, attachments, and password-protected projects remain unsupported. Unknown structures will produce explicit warnings or errors; they will never be silently interpreted as valid table data.

## Goals

- Import useful worksheet data from supported `.opj` files on macOS without Origin or any proprietary runtime.
- Recognize `.opju` without misidentifying it as OPJ, and explain that its container variant is not supported yet.
- Preserve workbook and worksheet identity, column names, values, nulls, and metadata that can be structurally validated.
- Reuse PlotX's existing table import preview, typed snapshot conversion, source provenance, recent-file routing, and operation-reporting paths.
- Fail safely on malformed, truncated, oversized, encrypted, unknown, or unsupported input.
- Keep `plotx-data` engine-free and keep both the default and `datafusion` feature configurations compiling.
- Document exact compatibility boundaries in English and Simplified Chinese.

## Non-goals

The first implementation will not:

- render or reconstruct Origin graphs;
- execute formulas, LabTalk, Origin C, Python, or other scripts;
- recompute fitting, statistics, signal processing, or analysis results;
- import matrices, images, layouts, Excel objects, OLE objects, attachments, or arbitrary embedded content;
- decrypt password-protected projects;
- preserve every Origin display format or application-specific property;
- promise compatibility with every Origin release that can produce an `.opj` or `.opju` file;
- invoke Origin, Wine, a virtual machine, Python, C++, or an external parser at runtime.

## Evidence and feasibility

### OPJ

Public implementations and sample files show that classic OPJ is a little-endian binary format with a text signature beginning with `CPYA`. Its main framing uses length-prefixed blocks separated by line-feed bytes. Native parsers exist on Unix-like systems, which demonstrates that Origin is not required.

Relevant evidence:

- OriginLab documents `.opj` and `.opju` as Origin project file types and documents opening and saving projects without describing a public complete binary specification: [Origin file types](https://docs.originlab.com/user-guide/origin-file-types/) and [Origin project files](https://docs.originlab.com/origin-help/origin-project-file/).
- [liborigin](https://github.com/gerlachs/liborigin) is a mature GPL-3.0 native reader for multiple OPJ generations. GPL-3.0 is compatible with PlotX's GPL-3.0-or-later project license, but this implementation deliberately does not copy, translate, or link liborigin code. Keeping it as an independent behavioral oracle preserves a genuinely separate cross-check while the MIT-licensed OpenOPJ material supplies the attributed structure descriptions used by the Rust implementation.
- [OpenOPJ](https://github.com/jgonera/openopj) is MIT-licensed and documents OPJ structures with a redistributable Origin 7.0552 fixture. Its documented values provide an independent expected-data source.
- Local probes parsed an Origin 7.0552 OpenOPJ fixture and separate Origin 8.0 and 9.7 fixtures with an independently compiled liborigin build. The latter probes are feasibility evidence only and do not make Origin 8.0 or 9.7 part of the first release's compatibility claim.

OPJ is therefore suitable for a direct, data-first Rust implementation. Compatibility will be based on recognized structures and tests, not on broad version-number claims.

### OPJU

OPJU starts with `CPYUA` and is not a ZIP, XML, or JSON document. Public samples show a proprietary binary container that can contain embedded XML or preview images without making the overall file a standard archive. Numeric data in one observed variant uses variable-length integers, ZigZag values, and Burtscher FPC floating-point compression. Other files with similar top-level version text use different record layouts.

Relevant evidence:

- [quantized](https://github.com/pquarterman17/quantized), licensed Apache-2.0, contains an independent OPJU numeric decoder. Its implementation and tests are new and its real-file corpus is mostly private. Its decoder scans the entire file for `ff ff` candidates, labels them with the nearest preceding name-like byte sequence, and filters decoded values heuristically; it does not validate a complete top-level worksheet data region. It is evidence for one numeric encoding, not a container profile safe enough for PlotX import.
- The decoder was locally exercised against the public Figshare project `RawData_Locust_Revision1_TIS_Mechanism.opju`, where it recovered 36 numeric columns across five worksheet groups. The dataset is published under CC BY 4.0: [Figshare record](https://doi.org/10.6084/m9.figshare.28535426.v1).
- The same decoder recovered no columns from 13 of 14 public Altaxo OPJU fixtures, including workbook and mixed-column examples. These failures demonstrate meaningful, undocumented OPJU record variants.
- Removing the OPJU prelude and passing the remaining bytes to liborigin did not produce a valid OPJ parse, confirming that OPJU is not classic OPJ with a different filename or a trivial wrapper.

OPJU will therefore remain detection-only in the first release. PlotX will identify the `CPYUA` family and reject it with an actionable unsupported-variant message. A later change may decode the FPC numeric grammar only after a trustworthy top-level profile, data-region boundary, group table, and complete record consumption are independently established. A byte-pattern match inside an otherwise unknown payload is never enough.

## Chosen approach

The implementation will be native Rust in PlotX:

1. `plotx-io` probes and parses Origin project bytes into an engine-neutral project model plus diagnostics.
2. `plotx-core` converts supported worksheet candidates into PlotX `TableSnapshot` values and source metadata.
3. The desktop application reuses the existing table-import preview and commit flow, presenting parser warnings and errors through `OperationReport` and the established feedback state.

This approach keeps parsing out of the UI, avoids a runtime dependency on an external executable, and preserves the existing crate boundaries. The rejected alternatives are linking or copying GPL liborigin code, shipping a Python or C++ sidecar, and pretending OPJU is a standard archive.

The parser design may reimplement documented record layouts and algorithms from license-compatible public projects in idiomatic Rust. OpenOPJ's MIT-licensed structure descriptions may be adapted with attribution. Liborigin remains an independent GPL behavioral oracle only: PlotX contributors will not copy, translate, or derive source code from it. Quantized's Apache-2.0 implementation may inform later clean-room OPJU work with attribution, but its heuristic scanner is not a valid container parser and will not be reproduced in the first release.

## Compatibility contract

### File detection

The extension selects files in the dialog but never establishes their format. Detection reads a bounded header and validates both signature and initial structure:

- `CPYA ... #\n` is an OPJ candidate.
- `CPYUA ...\n` is an OPJU candidate.
- Unsupported or ambiguous signatures return a user-facing `Unrecognized Origin project format` error.
- A matching extension with a conflicting signature is rejected and identifies the mismatch.
- A recognized signature with malformed version text or impossible initial framing is reported as corrupt rather than passed to another importer.

The probe result records the container kind, raw version text, any parsed version components, byte order, and support tier. It never allocates from untrusted lengths.

### OPJ profile and supported data

The first implementation will support an `Origin7V552` profile represented by the public Origin 7.0552 OpenOPJ fixture. Version text selects a candidate profile, but it does not grant compatibility by itself. Each profile defines the permitted top-level framing, dataset-header lengths, version-sensitive offsets, value type and width combinations, window-record geometry, and metadata record boundaries. Every required invariant must match the selected profile; walking to end-of-file without an I/O error is not proof of compatibility. Unknown producer versions, including the exploratory Origin 8.0 and 9.7 probes, are rejected until a redistributable fixture and independent expected values justify a separate profile.

The first-release evidence matrix is intentionally narrower than the type codes described by reverse-engineered parsers:

| Capability | Real-file evidence | First-release behavior |
| --- | --- | --- |
| 64-bit floating-point columns | OpenOPJ `test.opj` and its published expected values | Supported |
| 32-bit floating-point columns | OpenOPJ `test.opj` and its published expected values | Supported |
| 32-bit signed `long` columns | OpenOPJ `test.opj` and its published expected values | Supported |
| 16-bit signed integer columns | OpenOPJ `test.opj` and its published expected values | Supported |
| Fixed-width text columns | OpenOPJ `test.opj` and its published expected values | Supported |
| Mixed numeric/text columns | OpenOPJ `test.opj` and its published expected values | Supported |
| Missing values and nonzero first-row offsets | OpenOPJ `test.opj` and its published expected values | Supported |
| Dataset and column names, project parameters, and project notes | OpenOPJ `test.opj` and its published expected values | Supported as basic metadata |
| 8-bit integers, unsigned integers, long names, units, comments, and column designations | No redistributable real fixture with independent values in the current evidence set | Not advertised or enabled until equivalent evidence is added |

Workbook and worksheet grouping are exposed only where the verified window and dataset records associate them unambiguously. Unequal supported column lengths are allowed and are padded with nulls during core conversion. The project format and detected producer-version text are retained as import provenance.

The public `Origin7V552` evidence does not expose a trustworthy project code-page field, so the first release guarantees ASCII text only. Non-ASCII bytes are never guessed as Windows-1252 or another legacy encoding merely because every byte could be mapped. Non-ASCII text cell data causes an unsupported-encoding error. Independent non-ASCII metadata may be skipped with a warning only when its omission cannot affect table geometry or cell alignment. Embedded NUL padding is removed only within the declared fixed-width cell. A later profile may add a legacy encoding only when a redistributable fixture and a validated code-page field establish it.

Unsupported OPJ value types or objects will be counted and reported. A worksheet may still be imported when at least one supported column is recovered and skipping an unsupported, length-framed object cannot alter the interpretation of supported columns. If structural uncertainty could shift record boundaries or cell alignment, the entire file is rejected.

### OPJU gate outcome

The investigated `FpcNumericV1` profile is disabled because the available public evidence does not satisfy all validation-gate conditions:

- the file must have a valid `CPYUA` header;
- the complete ordered top-level profile, data-region boundary, worksheet group table, and column-record boundaries must be derived from the public fixture and checked without scanning arbitrary payloads for magic bytes;
- every numeric column record in the bounded data region must match the verified marker, bounded variable-integer fields, declared row count, segment grammar, and complete FPC stream;
- decoded values are 64-bit floating point, with the Origin missing-value sentinel mapped to null;
- workbook or sheet group names and UTF-8 column labels are used only when their surrounding anchors and lengths validate; otherwise PlotX assigns deterministic column names such as `A`, `B`, and reports that metadata was not recovered;
- a file with no supported numeric worksheet columns returns an `Unsupported OPJU record variant` error rather than an empty successful import;
- unknown records inside the bounded worksheet data region reject the whole file;
- objects outside the worksheet data region are reported as not imported and are skipped only when validated top-level framing supplies their complete boundaries;
- changing the fixture so that a valid-looking column marker appears only inside an unrelated bounded payload must not produce a column.

The current scanner fails the complete-profile and false-marker conditions, so `FpcNumericV1` stays disabled. The first release detects OPJU by content and returns a clear unsupported-variant error for every OPJU file. This gate result is an explicit successful outcome; no deadline or desire for OPJU support permits marker scanning or ambiguous partial import.

A future FPC implementation may be clean-room Rust based on the public Burtscher algorithm and cross-checked against the Apache-2.0 implementation only after the outer profile is proven. Any directly adapted ideas or test vectors must be attributed in source comments and fixture documentation. PlotX will not depend on the Python package at runtime.

Numeric columns, text cells, dates, categorical columns, matrices, graphs, formula state, and alternative OPJU record grammars are all unsupported in the first release. The UI and documentation will describe `.opju` as recognized but not currently importable.

## Crate and module design

### `plotx-io`

New modules:

- `crates/io/src/origin.rs`: public model, probe API, top-level limits, errors, and dispatch;
- `crates/io/src/origin/reader.rs`: checked cursor, bounded block and string reads, integer conversion, and shared diagnostics;
- `crates/io/src/origin/opj.rs`: explicit OPJ profiles, framing, dataset decoding, workbook assembly, and metadata extraction;
- `crates/io/src/origin/opju.rs`: `CPYUA` header validation and the stable detection-only unsupported-variant error;
- `crates/io/src/origin/tests.rs`: synthetic positive and negative format tests, kept outside production modules to preserve the 800-line rule.

Proposed public surface:

```rust
pub fn probe_origin(bytes: &[u8]) -> Result<OriginProbe, OriginError>;

pub fn read_origin(
    bytes: &[u8],
    limits: OriginLimits,
) -> Result<OriginProject, OriginError>;
```

The neutral model contains:

- `OriginProject`: format, producer version, project parameters, project notes, workbooks, diagnostics, unsupported-object summary, and conservative resource-usage accounting;
- `OriginWorkbook`: name and worksheets;
- `OriginWorksheet`: name, columns, row count, and worksheet metadata;
- `OriginColumn`: names, role, units, comments, logical type, and cells;
- `OriginCell`: null, numeric, integer, or text;
- `OriginDiagnostic`: stable diagnostic code, severity, object location, and user-safe message.

These types do not depend on PlotX application state, DataFusion, SQL parsers, or UI types. `crates/io/src/lib.rs` will export the module. The existing acquisition-oriented `DataFormat` enum will not be overloaded with project-file semantics.

### `plotx-core`

New modules:

- `crates/core/src/origin.rs`: convert each supported Origin worksheet into the existing typed table snapshot and preview candidate representation;
- `crates/core/src/origin_tests.rs`: assert type inference, null handling, metadata, source provenance, warnings, and conversion errors.

Conversion rules will follow the existing XLSX import path where practical:

- each worksheet becomes a separately selectable preview candidate;
- homogeneous integer or numeric columns retain a numeric type;
- mixed numeric/text columns become text rather than discarding either representation, with numeric cells rendered using a round-trippable representation;
- unequal column lengths are padded with null cells to the worksheet row count;
- duplicate or empty column names are made unique by the same table-name rules used elsewhere in PlotX, while retaining original names in source metadata;
- parser diagnostics remain attached to the preview and final operation report.

The raw source bytes and bounded source metadata will be retained through `TableImportSource` using the existing provenance pattern. Project parameters and notes are stored in explicit bounded fields on `OriginProject`; core copies them into the source metadata attached to each worksheet candidate, rather than turning them into table cells. No Origin parsing logic will enter `plotx-data`.

Core conversion accepts `OriginProject` by value together with the same `OriginLimits` used for reading. It consumes and drains worksheet storage while constructing snapshots and carries forward the parser's conservative `OriginResourceUsage`. Every snapshot allocation is estimated with checked arithmetic and charged before allocation against the shared total-owned-bytes budget.

### Desktop application

A new sibling module `crates/app/src/ui/file_dialogs/origin.rs` will own the application glue. Existing files near the repository line limit will receive only small routing additions.

UI integration:

- add a file-picker entry named `Origin projects (experimental)` for `.opj` and `.opju`;
- route file-open, import, drag/drop if already supported by the common path, and recent-file reopen through content probing;
- read the selected file once after a filesystem metadata size check;
- display one preview candidate per worksheet and require the normal explicit import action;
- include skipped-object counts and decoding warnings in the visible operation result;
- surface corrupt, unsupported, oversized, or version-incompatible files through the normal error state, with no panic and no empty success notification.

Background parsing, if used by the current table-import path, will send either a complete preview result or a complete error back to application state. It will not mutate layout state or silently log an error that originated from a user action.

## Data flow

```text
file picker / recent file / drop
            |
            v
bounded file read -> probe_origin -> read_origin
                                     |
                                     v
                      OriginProject + diagnostics
                                     |
                                     v
                    core worksheet conversion
                                     |
                                     v
                 existing table import preview
                                     |
                                     v
                  explicit user import action
                                     |
                                     v
             TableSnapshot + source provenance
```

At every boundary, an error replaces downstream processing. Partial results are shown only when the parser proves that skipped objects cannot affect decoded worksheet cells.

## Error and warning model

Errors prevent preview creation. Warnings accompany valid preview candidates.

Required error categories:

- unrecognized or extension/signature-mismatched file;
- unsupported OPJ or OPJU structural variant;
- a recognized password-protection or encryption marker when that marker has public evidence; otherwise the file is conservatively rejected as an unsupported or malformed structural variant rather than mislabeled;
- truncated header, block, record, string, or numeric stream;
- invalid delimiter, impossible length, integer overflow, or invalid row geometry;
- configured resource limit exceeded;
- unsupported encoding where safe recovery is impossible;
- inconsistent declared and decoded row counts;
- no supported worksheet data found.

Required warning categories:

- unsupported object types were skipped;
- unsupported columns were skipped while independent supported columns remained valid;

Messages will use plain product language, for example: `This OPJU file uses a record layout that PlotX does not support yet. No data was imported.` Internal byte offsets and diagnostic codes may be included in expandable detail or logs, but the primary message will not require format expertise.

## Untrusted-input safety

The parser will treat every file byte and declared length as hostile input.

Default limits:

- maximum input file size: 128 MiB;
- maximum header line: 128 bytes;
- maximum individual framed block: 32 MiB;
- maximum individual decoded string: 1 MiB;
- maximum cumulative decoded text: 32 MiB;
- maximum parser-decoded allocation: 128 MiB, including vector capacities, strings, and decompression output;
- maximum total owned Origin-import bytes: 384 MiB across source bytes, parser storage, and final preview table values;
- maximum workbooks: 256;
- maximum source window records retained for worksheet association: 1,024;
- maximum worksheets per workbook: 128;
- maximum total columns: 4,096;
- maximum cumulative metadata records: 65,536;
- maximum rows per column: 1,000,000;
- maximum total decoded cells: 2,000,000;
- maximum metadata nesting depth: 32.

All additions, multiplications, signed-to-unsigned conversions, row-size calculations, offsets, and allocation sizes use checked arithmetic. Reads use checked slices or cursor methods. Parser capacity requests are preflighted within the remaining byte budgets. Incrementally retained metadata vectors use bounded geometric growth and, after reserve, charge the actual capacity delta; allocator rounding beyond a budget fails closed. `OriginResourceUsage` records input and parser charges, then core consumes the project by value and charges estimated snapshot capacities against the shared 384 MiB total before allocating. Accounting is conservative and is not refunded in a way that permits repeated allocation churn to bypass the cap. Production parsing code will not use `unwrap()`, unchecked indexing, unchecked allocation from file lengths, or unsafe code.

Window records have an independent default limit of 1,024, enforced while metadata is parsed and before any dataset association. Together with the 4,096 data-section limit and the fixed 25-byte Origin7V552 window-name field, this bounds the fallback longest-prefix scan to at most 4,194,304 short comparisons rather than allowing the broader 65,536-record metadata budget to multiply association work.

Unknown records are skipped only when a validated outer framing supplies a bounded length. If no trustworthy boundary exists, parsing stops with an error. Embedded paths are never joined to the filesystem. Attachments, preview images, OLE payloads, scripts, and embedded XML are never extracted or executed. OPJU compression is not decoded in the first release, so arbitrary compressed output cannot be allocated from that container.

Filesystem metadata is an early rejection hint, not the memory guard. The application computes the bounded-reader cap with `max_input_bytes.checked_add(1)` and a checked conversion to the reader's limit type; an unrepresentable custom limit is rejected before reading. It never reserves the untrusted metadata length, and the extra byte distinguishes an exact-limit file from an oversized or growing file before unbounded allocation occurs. Source bytes are retained once through shared ownership rather than copied and are charged to the shared total. Core conversion consumes worksheet values and releases parser storage as it creates snapshots, while conservative accounting still caps the cumulative owned capacity even where exact allocator liveness cannot be observed. A decoder cancellation check will be included if the existing background import API exposes cancellation.

## Dependencies and licensing

The implementation will not incorporate or link liborigin code. This is an engineering and independent-validation choice, not a claim that GPL-3.0 is incompatible with PlotX. No external Origin installation or runtime parser process will be introduced.

No new direct crate dependency is planned for the first release. `encoding_rs` already exists transitively in the workspace lockfile, but it will not be added to `plotx-io` until a verified Origin code-page field and redistributable fixture justify a specific decoder. Cargo and `cargo deny` will still verify the resulting dependency graph, licenses, and advisories.

Fixtures and borrowed test vectors will have a provenance document containing source URL, author or dataset citation, license, original filename, byte length, cryptographic checksum, and any transformation performed by PlotX. If implementation details or structure descriptions are adapted from OpenOPJ or quantized, the relevant MIT or Apache-2.0 attribution will also appear in source comments and the repository's third-party attribution material. No private or merely discoverable Origin project will be committed.

## Test strategy

### Unit tests

Synthetic bytes generated in tests will cover:

- OPJ and OPJU signature detection independent of extension;
- extension/signature mismatch reporting at the application routing layer;
- the real-fixture-backed 64-bit and 32-bit float, 32-bit and 16-bit signed integer, text, mixed-cell, null, unequal-length, and metadata paths;
- documented but disabled type codes rejecting safely until real-file evidence is added;
- ASCII characters, fixed-width padding, non-ASCII cell rejection, and safe skipping of structurally independent non-ASCII metadata with a warning;
- OPJU signature recognition, truncated headers, extension/signature mismatch, and a public OPJU fixture producing the stable unsupported-variant error with no partial result;
- truncated files at every framing boundary;
- invalid delimiters, zero or oversized records, illegal row counts, width/length mismatch, and arithmetic overflow;
- unsupported versions, profile mismatches, recognized protection markers when evidence exists, malformed structural variants, and OPJU variants;
- configured file, block, string, column, metadata-record, row, cell, and nesting limits;
- unsupported objects producing warnings rather than panics or silent success;
- core conversion of concrete values, types, names, nulls, metadata, and diagnostics.

Negative tests will use generated bytes and will verify exact error categories and useful message fragments, not only that an error occurred. Property-style truncation tests will run every prefix of each minimal valid synthetic fixture through the parser and assert that it never panics.

### Public integration fixtures

The repository will include only fixtures whose redistribution terms are explicit:

- OPJ: OpenOPJ's MIT-licensed `support/test.opj`, Origin 7.0552, 282,034 bytes, SHA-256 `ac7f71c367562e85e9d4bb4ae418cbcaaa1b5dff80436180e8d3331c7e1d6308`.
- OPJU detection-only regression: Figshare `RawData_Locust_Revision1_TIS_Mechanism.opju`, CC BY 4.0, 64,954 bytes, SHA-256 `13c47a6a5daaf14493da59c8f1b284d9efb08129c8320b6ad9fd0b5191faa55f`, from DOI `10.6084/m9.figshare.28535426.v1`.

The OPJ fixture test will assert workbook or group names, worksheet counts, column names, exact row counts, exact representative numeric and text values, null positions, and selected metadata. The OPJU fixture test will assert reliable OPJU detection and a clear unsupported-variant rejection with no partial result. A fixture README will carry required MIT attribution and CC BY citation details.

### Independent comparison

For OPJ, expected values will come from OpenOPJ's published tests and will be spot-checked with a separately built liborigin executable used only during development. The GPL executable and its outputs will not ship with PlotX.

For OPJU, no independent CSV or worksheet-value export has yet been established. Concrete expected values can be cross-checked against the independent but immature Apache-2.0 quantized decoder, and this correlated-parser risk must be stated in code documentation and the pull request. OPJU is not promoted beyond experimental on that evidence. Finding a publisher-provided CSV/XLSX export or a second independent parser would strengthen a later compatibility claim.

### Repository verification

Implementation completion requires:

- focused `plotx-io`, `plotx-core`, and application tests;
- default-backend compilation and tests;
- `datafusion` feature compilation and tests;
- formatting and Clippy with warnings denied;
- source-file line-limit check;
- dependency license and advisory checks;
- `cargo pr-check`;
- `npm run build` from `docs/`.

`cargo pr-check` currently cannot complete on this machine because `cargo-deny` and `protoc` are absent. Installation will be requested before the final verification phase, as required by the user's no-install-without-permission rule.

## Documentation

The English user manual is canonical, and its matching Simplified Chinese page will be updated in the same change. Documentation and relevant UI copy will say `Origin project import (experimental)` or `Origin OPJ import (experimental)`, depending on context, and will use the PlotX brand spelling.

The manual will state:

- accepted extensions and signature-based detection;
- the exact `Origin7V552` OPJ profile represented by the public regression fixture;
- that `.opju` is recognized but remains detection-only because no complete safe container profile has been verified;
- imported worksheet data and metadata;
- unsupported objects and data types;
- corrupt, incompatible, recognizably protected, and unsupported-variant behavior, without claiming that every encrypted file can be distinguished from other unknown structures;
- that Origin does not need to be installed;
- that compatibility claims are limited to actual regression fixtures and independent checks.

No Origin trademark icon, logo, or copyrighted visual asset will be added.

## Acceptance criteria

The implementation is ready for an upstream pull request only when all of the following are true:

- a valid supported OPJ fixture produces the expected worksheets, names, types, values, nulls, and metadata through the visible import preview and final table import;
- the public OPJU fixture and synthetic `CPYUA` inputs are recognized and rejected with a clear unsupported-variant error and no partial table data;
- invalid extensions cannot override content detection;
- every negative fixture returns a stable error or warning without panic, excessive allocation, path access, script execution, or silent data corruption;
- skipped unsupported objects are disclosed and structurally uncertain files are rejected;
- default and `datafusion` configurations compile and pass their relevant tests;
- `cargo pr-check` and the documentation build pass after required local prerequisites have been installed with permission;
- all Rust source files remain below 800 lines;
- fixture licenses, citations, and checksums are committed;
- user documentation accurately matches the implemented support tiers;
- the final diff contains no credentials, private data, generated directories, or unrelated edits.

## Later compatibility work

Subsequent changes can add independently verified OPJ generations, additional OPJU record grammars, text/date decoding, matrices, or richer metadata. Each expansion must arrive with a redistributable fixture, explicit expected values from an independent source, bounded parsing, and a documentation update. Unsupported features will not be inferred from nearby bytes or enabled solely because a producer version appears similar.
