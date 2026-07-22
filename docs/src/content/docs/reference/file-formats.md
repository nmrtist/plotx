---
title: File formats
description: Native PlotX files, imported formats, and their compatibility boundaries.
---

## `.plotx` projects

A `.plotx` file stores the whole session in one file: imported data,
processing pipelines, analysis results, board layout, any saved workflows, and
a record of every automation run. That makes it self-contained — copy it to
another machine or send it to a colleague and it opens exactly as you saved
it, with no side files to remember. Keeping your original instrument data is
still good practice, as with any analysis software.

Until PlotX 1.0, the project format may change between releases. When a newer
format meets an older PlotX (or the other way around), the file is rejected
with a clear "unsupported version" message — your file is never modified or
migrated silently. If that happens, update PlotX and reopen.

**Preferences → General → Project backup copies** keeps previous saves as
hidden files beside the project, so an accidental overwrite can be recovered.

## `.plotxproc` processing recipes

A `.plotxproc` file stores one processing pipeline, without any data — save a
recipe once and apply it to a whole series of similar experiments, on any
machine. See [Recipes and templates](/guides/templates/). Version handling
works the same way as for projects: a mismatched file is rejected with a clear
message, never misread.

## Workflow and run-record files

An [automation](/guides/automation/) workflow file is a JSON description of a
batch run — which files to import, what to apply, what to export. Running one
produces a run-record (manifest) file stating exactly what happened to which
dataset, so a batch result is always traceable. Both are plain JSON, and both
are covered in [the command line](/reference/cli/).

A workflow is not a recipe: a recipe holds one processing pipeline, while a
workflow describes a whole run and may reference a recipe as one of its steps.

## Origin project import (experimental)

Origin project import is experimental. Successful import is limited to the
classic OPJ profile verified against a real Origin 7.0552 fixture.
Compatibility claims are limited to that committed regression fixture and
independent verification evidence, and expand only when new evidence is added.

Verified worksheet cell forms are `f64`, `f32`, signed `i32`, signed `i16`,
fixed-width ASCII text, mixed numeric/text cells, nulls, and nonzero row
offsets. Mixed columns are retained as text, and unequal column lengths are
padded with nulls.

PlotX preserves workbook and worksheet names and column names. Project
parameters and notes are retained as source metadata, not inserted as table
cells. There is no verified-support claim for long names, units, comments,
column designations, dates, categorical values, or code pages.

An `.opju` file is recognized from its CPYUA content signature, but `.opju` is
not importable in this release and PlotX creates no partial OPJU result.

Unsupported content includes graphs, formulas, scripts, analysis
recomputation, saved analysis results as executable analyses, matrices,
embedded objects, non-ASCII text, encrypted or protected projects, unverified
OPJ versions or profiles, and unverified OPJU containers.

PlotX never silently or heuristically guesses an import. Corrupt or truncated
files, files above the current 128 MiB input cap, extension/signature-family
mismatches, and malformed or otherwise unsupported files produce a clear error
before any table is committed. Inside an otherwise supported OPJ, an
unsupported worksheet column may be omitted, or an unsupported non-table object
skipped, only when each is independently framed and its outer boundaries are
trusted. PlotX shows warnings for every such omission; an imported worksheet
may therefore contain only the supported columns, not every source column. If
framing is ambiguous or untrusted, PlotX rejects the file rather than guessing
boundaries or silently shifting data.

Origin need not be installed, launched, or called during import.

## Data you import and export

See [Importing data](/guides/importing-data/) for the supported instrument and
tabular formats, and [Exporting](/guides/exporting/) for figure and data
export — including the `.plotx-schema.json` companion that lets an exported
CSV/TSV round-trip back into PlotX with its column types, units, and error
bars intact.
