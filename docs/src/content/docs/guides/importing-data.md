---
title: Importing data
description: Supported file formats and how to open them.
---

PlotX reads vendor LC–MS, NMR, XPS, AFM, and electrophysiology formats directly —
no conversion step is needed.

## Supported formats

| Format | Extension | Notes |
| --- | --- | --- |
| JEOL Delta | `.jdf` | 1D, 2D, and pseudo-2D (DOSY / T1 / T2) |
| Bruker TopSpin | `fid` / `ser` directories | 1D and 2D |
| Varian/Agilent VnmrJ | `.fid` directory | Raw time-domain 1D and conventional 2D |
| Waters MassLynx RAW | `.raw` directory | Validated low-resolution runs, including SQD2 data |
| SCIEX legacy WIFF | `.wiff` + `.wiff.scan` | Single- and multi-sample legacy runs; both files must remain together |
| Rigaku powder XRD | `.rasx`, FI `.raw`, RAS_RAW `.txt` | Diffraction pattern, acquisition metadata, and attenuation when available |
| mzML | `.mzML` | Centroided/profile spectra and TIC, BPC, SIM, or SRM chromatograms with 32-bit or 64-bit arrays |
| Bruker NanoScope AFM | `.spm` / `.pfc` | Images, force curves, force-volume and PeakForce Capture cubes |
| JCAMP-DX | `.dx` / `.jdx` / `.jcamp` | 1D frequency-domain NMR spectra |
| Axon Binary Format 2 | `.abf` | int16/float32, multiple channels and sweeps, embedded DAC/epoch stimuli |
| VAMAS XPS | `.vms` | ISO 14976 `NORM` / `REGULAR` XPS blocks, including multiple measurement positions and regions |
| CasaXPS text | `.txt` | Structured eight-line export with raw spectrum, background, envelope, components, and fitted parameters |
| Tabular data | `.csv`, `.tsv`, `.txt`, `.xlsx` | Column types and empty cells preserved; one table per XLSX worksheet |
| Origin project (experimental) | `.opj`, `.opju` | Worksheets from the verified Origin 7.0552 and Origin 9.51 OPJ profiles; graphs are not imported, and `.opju` is detection-only. See [compatibility details](/reference/file-formats/). |
| Zip archive | `.zip` | An archived dataset folder |
| PlotX project | `.plotx` | Full project: data, processing, and layout |

## Opening files

Drag a file onto the PlotX window, or use the toolbar's open menu:
*Open File…*, *Open Folder…* (for acquisition directories such as Bruker
TopSpin, Varian/Agilent VnmrJ, and Waters MassLynx RAW), *Open Project…*, or
*Import Table…*.
Each imported dataset appears in the Primary Side Bar and is placed on the
board automatically.
The file picker accepts several ABF files at once. Opening a folder recursively
imports every `.abf`, `.spm`, `.pfc`, `.vms`, `.wiff`, structured CasaXPS
`.txt`, and recognized `.raw` bundle below it. A `.wiff.scan` companion is
never imported as a separate dataset.
A `.raw` directory is imported once as a complete run; its internal files are
not treated as separate datasets. For ABF files, each immediate parent folder
becomes the initial, editable cell ID.
CasaXPS `.txt` files are recognized from their structured header, not from the
extension alone. Other `.txt` files continue through table import. See the
[XPS workflow](/guides/xps/) for energy-axis and fitting details.

## Varian/Agilent VnmrJ

To import a raw 1D or conventional 2D acquisition, choose **Open Folder…** and
select its `.fid` directory. You can instead choose **Open File…** and select
the `fid` file inside. Keep the `fid` and `procpar` files together in the same
directory.

Processed spectra, 3D or 4D experiments, imaging, pseudo-2D experiments,
non-uniform sampling, and other arrayed experiments are not supported. See
[File formats](/reference/file-formats/) for compatibility details.

## mzML

Open or drop a `.mzML` file. PlotX imports the spectra into the same LC–MS
dataset and chart workflow used for Waters runs. Spectra are grouped by MS
level and polarity, while each scan retains acquisition-function metadata such
as its mzML instrument configuration, preset scan configuration, and filter
string when present. Scan times recorded in seconds or minutes are displayed
in minutes. PlotX prefers
each spectrum's file-supplied TIC and base-peak summaries over values derived
from profile samples, and retains whether each summary came from the source or
was derived. PlotX also imports file-supplied TIC, BPC, SIM, and SRM
chromatograms, including chromatogram-only acquisitions. Transition
precursor/product m/z, polarity, collision energy, and activation method are
retained when present.
For MS2 and higher spectra, PlotX separately retains the precursor spectrum
reference, selected-ion m/z, selected-ion intensity and charge, isolation-window
target and offsets, collision energy, and activation method. This distinction
preserves DIA isolation targets even when a selected ion is not present. The
current data model stores one precursor and one selected ion per spectrum; an
import warning identifies spectra with additional values and states that only
the first was retained. Scientific Script scan snapshots expose the summary
provenance, instrument configuration, source event or preset, and filter string.

TIC and BPC provenance is kept separate. A file-supplied TIC or BPC channel is
marked as a source chromatogram and is used for its bound acquisition stream
before any fallback. When that channel is absent, PlotX uses the per-spectrum
source summary when available; otherwise it deterministically derives TIC from
the intensity array or BPC from the largest non-negative peak. A mixed run can
therefore report both source summaries and array-derived points. An unbound
source chromatogram in a multi-stream run is kept as its own channel rather
than guessed as a replacement for a stream's TIC or BPC. Field metadata,
Scientific Summary, and Scientific Script expose the selected provenance.

For runs with many chromatogram channels, open **Dataset tools → Mass
spectrometry** from **Extract Mass Spectrum** or the command palette. The
**Chromatogram channels** browser lists a stable count and ordering, with TIC
and BPC ahead of SIM/SRM transitions. Search matches channel names and native
IDs. Structured transitions can also be filtered by precursor m/z, product
m/z, polarity, collision energy, and activation method. Numeric fields accept
an exact value, comparisons such as `>=400`, or a range such as `400..500`.

Selecting a row replaces the series on the current LC–MS chromatogram plot with
that single channel through the normal PlotX field and binding workflow. Mass-
spectrum plots are never retargeted. The choice is undoable and is saved with
the page in a `.plotx` project. Chromatogram-only runs use the same browser.
The panel reports when a run has no plottable channels, no structured transition
metadata, or no channels matching the current filters. The list is virtualized,
so only visible rows are created for large scheduled-MRM runs.

The importer accepts little-endian 32-bit and 64-bit floating-point m/z, time,
and intensity arrays with no compression or zlib compression. Numpress,
big-endian arrays, and spectra or chromatograms without their required arrays
stop the import with an error.

## SCIEX legacy WIFF

Open or drop the `.wiff` file. Keep the paired file with `.scan` appended to
the full filename beside it, for example `sample.wiff` and
`sample.wiff.scan`. PlotX imports native scan IDs, retention times, m/z and
intensity arrays, precursor details when available, polarity, instrument and
acquisition-start metadata, and a separate TIC for each verified experiment.
Spectra are grouped into independent sample/experiment acquisition streams and
retain cycle order, including zero-TIC and empty DDA slots. Duplicate sample
names remain separate and are displayed with stable suffixes such as
`yjs_10ppm #1` and `yjs_10ppm #2`.

PlotX rejects a missing companion, an empty container, or an unrecognized WIFF
layout rather than creating a partial dataset. SCIEX `.wiff2` and `.timeseries.data` are not supported;
convert those acquisitions to mzML before opening them in PlotX.

## Rigaku powder XRD

Open the `.rasx` file when it is available. PlotX reads the measured 2theta,
intensity, and attenuation columns together with the instrument name, X-ray
target, Kalpha1 wavelength, tube voltage/current, scan step, and scan speed.
The initial page is an XRD pattern. **Processing** provides optional SNIP
background subtraction, Savitzky-Golay smoothing, and normalization by maximum
intensity or integrated area; these settings and the original observations are
saved in the PlotX project.

Rigaku `FI`-layout `.raw` files and profile `.txt` exports whose header identifies
`RAS_RAW` also open as XRD. The binary reader retains the scan ruler, intensity,
X-ray target, Kalpha1 wavelength, tube voltage/current, and scan speed. Other
`.raw` signatures are rejected explicitly because the extension is shared by
incompatible vendor formats. Other `.txt` files keep the normal table-import
preview, so a generic numeric table is not silently reclassified.

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

To create an extracted ion chromatogram (XIC; also called an EIC), open
**Dataset tools → Mass spectrometry** and choose **Select m/z range**. Drag
across the current mass-spectrum plot, confirm the displayed m/z interval and
acquisition stream, then choose **Extract ion chromatogram**.

The resulting line plot contains one point per scan at its retention time in
minutes. Each point is the sum of intensities within the selected m/z interval,
including both endpoints. The result is saved and does not change when you move
the scan preview or select another acquisition stream. XIC extraction does not
integrate chromatographic peaks or calculate peak area.

If the run contains several supported MS functions, use **Acquisition stream**
under **Dataset tools → Mass spectrometry**. The initial stream is the first
non-reference MS function. Stream changes, spectrum extractions, and XIC
extractions can be undone and redone with the standard Edit commands.

PlotX supports the low-resolution MassLynx encoding validated with SQD2 runs.
If a required MS function uses another encoding, the import stops and
identifies the function and instrument. Unsupported optional or reference
functions produce an import warning when the rest of the run is readable.

There is no LC–MS processing pipeline. The imported run, active acquisition
stream, detector channels, extracted spectra, extracted ion chromatograms, and
page layout are saved in the `.plotx` project. The scan preview is temporary
and is cleared when the project is reopened.

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
