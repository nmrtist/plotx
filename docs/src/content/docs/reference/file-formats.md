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

**Preferences → General → Project backup copies** keeps previous saves as
hidden files beside the project, so an accidental overwrite can be recovered.
Automatic crash-recovery checkpoints are separate internal files. They are
updated only after new edits and are not portable project copies.

For homonuclear 2D review, the project stores cross-peak marks, reciprocal
pair links, and each mark's review state. The pinned cursor position and
symmetry audit results are not stored.

For pseudo-2D DOSY, a project also holds the per-column and ILT maps
themselves, the ILT settings each was built with, your **Show** and **DOSY
method** choices, and enough about how each map was produced for PlotX to warn
you on reopening if the map no longer matches the data the project
reconstructs. The contour drawing is not stored — PlotX redraws it from the
saved map when the project opens.

## Embedded images

PlotX accepts PNG, JPEG, TIFF, WebP, and BMP images and stores them in the
`.plotx` project. Duplicating an image does not duplicate its stored source, but
each copy can have its own crop, rotation, opacity, fit, and interpolation.
Export and clipboard output sample the embedded pixels and preserve those
settings, Panel clipping, and z-order. SVG output embeds image data and never
depends on the original local path.

SVG is not supported. For an animated PNG or WebP, **Add Images…** reports that
the image is animated; use **Add Animated Image First Frame…** to display its
first frame. **Add Images Without Metadata…** converts the pixels to PNG before
storing them.

PlotX rejects images above 500 megapixels or an estimated 2 GiB when decoded.
It asks for confirmation above 100 megapixels or 512 MiB. For a multi-page TIFF,
**Add Images…** imports the first page and **Add All TIFF Pages…** imports every
page that PlotX can read. Each imported page can be edited independently.

PlotX checks every embedded image when a project opens. A missing, damaged, or
mismatched resource produces a diagnostic and a replaceable placeholder
without discarding the page or image item. PlotX does not silently save that
degraded state. Publication export blocks by default; an explicit Export-dialog
option can emit a labelled placeholder for review.

## `.plotxproc` processing recipes

A `.plotxproc` file stores one processing pipeline, without any data — save a
recipe once and apply it to a whole series of similar experiments, on any
machine. See [Recipes and templates](/guides/templates/).

## Workflow and run-record files

An [automation](/guides/automation/) workflow file is a JSON description of a
batch run — which files to import, what to apply, what to export. Running one
produces a run-record (manifest) file stating exactly what happened to which
dataset, so a batch result is always traceable. Both are plain JSON, and both
are covered in [the command line](/reference/cli/).

A workflow is not a recipe: a recipe holds one processing pipeline, while a
workflow describes a whole run and may reference a recipe as one of its steps.

## XPS import

VAMAS `.vms` support is intentionally limited to content-signed ISO 14976
`NORM` / `REGULAR` XPS blocks with regular energy rulers. One file remains one
experiment with stable measurement and region identities. PlotX retains native
energy, counts, CPS, photon energy, dwell time, sweeps, position, acquisition
conditions, and free metadata. Non-XPS blocks can be skipped with warnings only
when their declared block boundaries are trusted; malformed XPS payloads reject
the file. Ordinate extrema are metadata rather than data points, and ordinate
labels plus signal mode determine whether pulse counts are converted to CPS.

CasaXPS `.txt` is recognized by its eight-line structure, not by `.txt` alone.
Its source arrays and fitted parameters are stored as an `Imported` result.
Unstructured text continues through table import. See [XPS](/guides/xps/) for
the energy conversion, processing, fitting, and export contract.

## Origin project import (experimental)

Origin project import is experimental. Successful import is limited to two
exact, content-detected OPJ producer profiles:

- Origin 7.0552 (`CPYA 4.2673 build 552`) imports verified `f64`, `f32`, signed
  `i32`, signed `i16`, fixed-width ASCII text, mixed numeric/text cells, nulls,
  and nonzero row offsets. Project parameters and notes are retained as source
  metadata.
- Origin 9.51 build 195 W64 (`CPYA 4.3268 build 195 W64`) imports worksheet
  names, column names, numeric `f64` values, nulls, and validated empty columns.
  This modern profile does not yet import text cells, project parameters, or
  notes.

Compatibility claims are limited to the committed Origin 7 regression fixture
and the exact Origin 9.51 profile checked against two real projects, companion
CSV exports, and an independent parser comparison. They do not extend to other
files merely because the extension or major Origin version is the same.

PlotX preserves validated Origin window or group names and column names. Each
supported window is represented as one table under the generated worksheet
name `Sheet1`; this release does not claim to decode original worksheet labels.
Mixed Origin 7 columns are retained as text, and unequal column lengths are
padded with nulls. There is no verified-support claim for long names, units,
comments, column designations, dates, categorical values, or unverified code
pages.

An `.opju` file is recognized from its CPYUA content signature, but `.opju` is
not importable in this release and PlotX creates no partial OPJU result.

Unsupported content includes graphs, formulas, scripts, analysis
recomputation, saved analysis results as executable analyses, matrices,
embedded objects, modern OPJ text cells, non-ASCII text without a verified code
page, encrypted or protected projects, unverified OPJ versions or profiles, and
unverified OPJU containers. For the supported Origin 9.51 profile, PlotX stops
after the validated window-list boundary and warns that the remaining project
objects were not imported.

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
