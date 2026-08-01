---
title: Importing data
description: Supported file formats and how to open them.
---

PlotX reads vendor LC–MS, NMR, AFM, and electrophysiology formats directly —
no conversion step is needed.

## Supported formats

| Format | Extension | Notes |
| --- | --- | --- |
| JEOL Delta | `.jdf` | 1D, 2D, and pseudo-2D (DOSY / T1 / T2) |
| Bruker TopSpin | `fid` / `ser` directories | 1D and 2D |
| Waters MassLynx RAW | `.raw` directory | Validated low-resolution runs, including SQD2 data |
| mzML | `.mzML` | Centroided or profile LC–MS spectra with 32-bit or 64-bit arrays, uncompressed or zlib-compressed |
| Bruker NanoScope AFM | `.spm` / `.pfc` | Images, force curves, force-volume and PeakForce Capture cubes |
| JCAMP-DX | `.dx` / `.jdx` / `.jcamp` | 1D frequency-domain NMR spectra |
| Axon Binary Format 2 | `.abf` | int16/float32, multiple channels and sweeps, embedded DAC/epoch stimuli |
| Tabular data | `.csv`, `.tsv`, `.txt`, `.xlsx` | Column types and empty cells preserved; one table per XLSX worksheet |
| Origin project (experimental) | `.opj`, `.opju` | Worksheets from the verified Origin 7.0552 and Origin 9.51 OPJ profiles; graphs are not imported, and `.opju` is detection-only. See [compatibility details](/reference/file-formats/). |
| Zip archive | `.zip` | An archived dataset folder |
| PlotX project | `.plotx` | Full project: data, processing, and layout |

## Opening files

Drag a file onto the PlotX window, or use the toolbar's open menu:
*Open File…*, *Open Folder…* (for acquisition directories such as Bruker
TopSpin and Waters MassLynx RAW), *Open Project…*, or *Import Table…*.
Each imported dataset appears in the Primary Side Bar and is placed on the
board automatically.
The file picker accepts several ABF files at once. Opening a folder recursively
imports every `.abf`, `.spm`, `.pfc`, and recognized `.raw` bundle below it.
A `.raw` directory is imported once as a complete run; its internal files are
not treated as separate datasets. For ABF files, each immediate parent folder
becomes the initial, editable cell ID.

## mzML

Open or drop a `.mzML` file. PlotX imports the spectra into the same LC–MS
dataset and chart workflow used for Waters runs. Spectra are grouped by MS
level and polarity; scan times recorded in seconds or minutes are displayed in
minutes. File-supplied chromatograms are not imported.

The importer accepts little-endian 32-bit and 64-bit floating-point m/z and
intensity arrays with no compression or zlib compression. Numpress, big-endian
arrays, and spectra without both required arrays stop the import with an error.

## Waters MassLynx RAW

Open or drop the `.raw` directory itself. PlotX imports its supported MS
functions and optical detector channels. Temperature, pressure, and other
readable auxiliary channels remain in the dataset but are not plotted by
default.

When optical detector data is present, the initial page places its UV channels
above the active function's total ion chromatogram (TIC) on a shared retention-
time axis. Multiple UV channels are overlaid; their legend uses stored
wavelengths such as `214 nm`. Select the UV plot and use **Legend & scales** in
the Object inspector to hide, move, or lay out that legend. Without optical
data, the initial page contains only the TIC.

Select the LC–MS dataset, then choose **Extract Mass Spectrum** on the
**Analyze** tab. PlotX opens **Dataset tools → Mass spectrometry** in the right
sidebar and activates retention-time range selection.

Click a TIC or UV chromatogram to show the nearest MS scan under **Scan
preview**. The preview identifies its retention time and native scan number; it
is neither added to the page nor saved as a result. Choose **Extract current
scan** to add that scan as a stick spectrum.

To extract from a time window, choose a **Method**, select **Select range**, and
drag across a TIC or UV chromatogram. **Extract spectrum** adds the peak-apex
scan, nearest scan, mean spectrum, or summed spectrum to the page. Each
extracted spectrum records its function, time range, and method. It does not
change when the preview cursor moves and appears under **Analysis** in the Data
browser.

If the run contains several supported MS functions, use **MS function** under
**Dataset tools → Mass spectrometry**. The initial active function is the first
non-reference MS function. Function changes and spectrum extractions can be
undone and redone with the standard Edit commands.

PlotX supports the low-resolution MassLynx encoding validated with SQD2 runs.
If a required MS function uses another encoding, the import stops and
identifies the function and instrument. Unsupported optional or reference
functions produce an import warning when the rest of the run is readable.

There is no LC–MS processing pipeline. The imported run, active function,
detector channels, extracted spectra, and page layout are saved in the
`.plotx` project. The scan preview is temporary and is cleared when the project
is reopened.

Tables can also be pasted straight from the clipboard with
`Ctrl` + `Shift` + `V` — comma-, tab-, or semicolon-delimited text becomes a
new data table.

Importing a table, from a file or the clipboard, first opens a **Review table
import** dialog. It shows each column's inferred type and unit, whether the
column allows empty cells, a preview of the first rows, and any import
diagnostics. Choose **Import table** to add it, or **Cancel** to leave your
project and recent-file list untouched. An XLSX workbook with several sheets
adds a **Table** selector so you can preview each worksheet; a single **Import
table** brings them all in as separate tables.

PlotX keeps Boolean, whole-number, decimal, text, and empty cells distinct. A
column that mixes kinds of value, or whose values are ambiguous, is kept as text
rather than dropped. Unless the file carries PlotX's own type information (see
below), only unmistakable values are typed automatically: `true`/`false`,
base-10 integers, `YYYY-MM-DD` dates, and `YYYY-MM-DDTHH:MM:SSZ` UTC timestamps.
Locale-specific dates and columns that mix numbers with text stay as text, so
PlotX never guesses a regional format.

When PlotX exports a CSV or TSV, it writes a companion `.plotx-schema.json` file
next to it, and Copy TSV puts the same information on the clipboard beside the
plain text (on Windows). Reopening either restores the original column types,
units, and error-bar relationships. Without that companion, PlotX infers the
types on import and flags anything ambiguous in the review dialog.

In an `.xlsx` workbook, each visible worksheet imports as its own table, and
PlotX keeps its type information on a hidden worksheet. PlotX reads the value
Excel cached for each formula but does not recalculate formulas itself; a
formula cell with no cached value imports as empty and is listed in the
diagnostics. Exported XLSX files hold plain values, so they never depend on
Excel recalculating them.

## Origin project import (experimental)

Origin `.opj` and `.opju` files appear in the file picker for both *Open
File…* and *Import Table…*. Both routes identify the format from file
content and signatures rather than relying only on the extension.

When a supported `.opj` yields worksheets, PlotX opens the existing **Review
table import** preview so you can inspect every candidate table. Confirm once
to import all candidates, or cancel to leave the current project and recent-file
list unchanged. While a preview is pending, selecting a second table path is
rejected with a clear message; finish or cancel the current preview first.

Origin does not need to be installed or launched, and PlotX does not automate
or invoke it. See [File formats](/reference/file-formats/) for the exact,
evidence-limited compatibility boundary.

## Pseudo-2D experiments

DOSY, T1, and T2 experiments are detected automatically from the acquisition
parameters and get their own analysis tools — see
[Pseudo-2D analysis](/guides/pseudo-2d/).

For patch-clamp sweeps, filtering, time-window statistics, stimulus handling,
and IV analysis, see [Electrophysiology](/guides/electrophysiology/).

## Bruker NanoScope AFM

PlotX imports NanoScope `.spm` images, force curves, and force-volume grids,
plus PeakForce Capture `.pfc` data cubes. Image channels plot as maps at the
recorded scan size, in the file's physical units, with the aspect ratio locked.
Force curves plot as separate approach and retract branches; when the file
records a deflection sensitivity, the vertical axis is deflection in
nanometres, otherwise the curve stays in the unit stored in the file. PlotX
shows the acquired data as is — it does not infer a contact point, indentation,
or modulus, and does not fit a contact-mechanics model.

A PeakForce Capture file usually has an AllImages `.spm` export saved beside
it. PlotX finds that companion, checks that its image grid matches the force
grid, and imports the pair as one dataset; opening a folder also imports the
pair once, not as two datasets. The default canvas places the channel map
beside a force curve from the centre pixel of the grid. If no companion is
found, or its grid does not match, the `.pfc` file still imports with its
force curves alone.

PeakForce Capture curves are the per-pixel signals as acquired. Derived QNM
maps such as modulus arrive as their own image channels; PlotX does not
recompute them from the curves.
