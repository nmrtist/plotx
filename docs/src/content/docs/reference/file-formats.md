---
title: File formats
description: What PlotX's own files contain and how safely they can be shared.
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

## Data you import and export

See [Importing data](/guides/importing-data/) for the supported instrument and
tabular formats, and [Exporting](/guides/exporting/) for figure and data
export — including the `.plotx-schema.json` companion that lets an exported
CSV/TSV round-trip back into PlotX with its column types, units, and error
bars intact.
