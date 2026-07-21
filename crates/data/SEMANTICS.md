# PlotX table semantics v1

This document is normative for persisted `plotx-data` v1 plans and snapshots.
Changing one of these rules requires a new function or operation identifier;
it does not change the PlotX project `schema_version`, which remains `1`.

## Identity and storage

- A table, revision, row, and column has an opaque stable ID. Row IDs survive
  project, filter, and stable-sort operations.
- Join, aggregate, pivot, and unpivot derive deterministic row IDs from their
  operation and input identities. Union first namespaces rows by source table.
- A snapshot is immutable. Its manifest refers to aligned, column-oriented
  chunks by an exact-byte SHA-256 hash and a separate canonical logical hash.
- Arrow IPC is the first v1 chunk codec, not a public API. A reader verifies
  both hashes and rejects missing, corrupt, or unknown-codec blocks.
- Equal encoded blocks are deduplicated. Raw imported bytes, including
  clipboard text, are separate content-addressed input objects.

## Types and missing values

- Built-ins are Null, Boolean, Int64, Float64, UTF-8, ordered categorical,
  Date, Time, UTC Timestamp with an IANA display timezone, and Duration.
- Registered reverse-domain extension types declare v1 storage and whether
  their semantics are critical. Unknown extensions are preserved. An unknown
  critical extension cannot participate in a calculation.
- Null is an independent validity bit. NaN, positive infinity, and negative
  infinity are valid Float64 values. Re-encoding may normalize storage bytes
  beneath nulls without changing logical content.
- A non-null schema rejects a null value. Business-key columns are non-null and
  are validated for uniqueness by the operation that uses the key.

## Units and uncertainty

- A unit converts to its canonical unit as `canonical = value * scale +
  offset`. Conversion is allowed only when dimensions, canonical unit, and any
  controlled extension ID agree.
- `ppm` is a ratio with scale `1e-6`. Domain units such as `a.u.` require a
  registered extension when they are not convertible.
- Uncertainty is a relation between ordinary columns. Symmetric,
  lower/upper, and confidence-interval forms are supported. Related columns
  must be numeric and unit-compatible; confidence levels are finite and in
  `(0, 1)`.

## Relation and expression rules

- Persisted plans are PlotX `RelPlanV1`, never SQL, Substrait, Arrow, or a
  backend logical plan. Cross, as-of, interval, and arbitrary non-equi joins
  are not v1 operations.
- Expressions use SQL three-valued Boolean logic. Filter retains only true;
  false and null are excluded, with null exclusions diagnosed.
- Equi-join null keys never match. Requested one-to-one, one-to-many, or
  many-to-one cardinality is checked before emitting rows.
- `count(*)` counts rows. `count(expr)` and numeric aggregates ignore null and
  report excluded-null counts. Grouping treats null keys as one group.
- Ascending Float64 order is negative infinity, finite values, positive
  infinity, NaN, then null by default. Null placement is explicit in the plan.
  Equal sort keys use RowId as the stable final key.
- Aggregation consumes rows in stable input order. Parallel backends must use a
  fixed merge tree so result values and fingerprints do not depend on thread or
  chunk count.
- Clock, random, and UUID functions are forbidden in rerunnable plans. Such
  values must first be materialized as explicit input columns.
- Unit conversion is an explicit plan node. Incompatible units fail before
  execution.

## Revisions, refresh, and patches

- A revision records exact input revisions and fingerprints, its PlotX plan,
  operation/function/software versions, result fingerprint, diagnostics,
  column lineage, and a compressed row mapping when the row set changes.
- Refresh creates a new revision. Existing downstream work stays pinned until
  an explicit rebase; automation may opt into following and must record the
  revisions actually used.
- A patch addresses only `(RowId, ColumnId)` and sets a typed value or null.
  Adding/removing rows or columns uses ordinary relation nodes.
- Patch rebase first consumes an exact source mapping supplied by provenance or
  a previously validated unique business key. Missing rows, removed columns,
  and type or unit changes are blocking conflicts. Position and fuzzy content
  matching are never attempted.

## Execution contract

- Execution is asynchronous and exposes progress, cancellation, a memory
  budget, diagnostics, and a backend identity. The reference interpreter is
  normative for semantics, not large-table performance.
- A production backend result is saveable only after differential tests show
  the same schema, row IDs, logical values, diagnostics, lineage, and
  fingerprint as the reference interpreter for the relevant v1 operations.
