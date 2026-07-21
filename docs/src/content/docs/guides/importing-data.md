---
title: Importing data
description: Supported file formats and how to open them.
---

PlotX reads vendor NMR and electrophysiology formats directly — no conversion step is needed.

## Supported formats

| Format | Extension | Notes |
| --- | --- | --- |
| JEOL Delta | `.jdf` | 1D, 2D, and pseudo-2D (DOSY / T1 / T2) |
| Bruker TopSpin | `fid` / `ser` directories | 1D and 2D |
| JCAMP-DX | `.dx` / `.jdx` / `.jcamp` | 1D frequency-domain NMR spectra |
| Axon Binary Format 2 | `.abf` | int16/float32, multiple channels and sweeps, embedded DAC/epoch stimuli |
| Tabular data | `.csv`, `.tsv`, `.txt`, `.xlsx` | Column types and empty cells preserved; one table per XLSX worksheet |
| Zip archive | `.zip` | An archived dataset folder |
| PlotX project | `.plotx` | Full project: data, processing, and layout |

## Opening files

Drag a file onto the PlotX window, or use the toolbar's open menu:
*Open File…*, *Open Folder…* (for acquisition directories such as Bruker
TopSpin), *Open Project…*, or *Import Table / CSV…*. Each imported dataset
appears in the Primary Side Bar and is placed on the board automatically.
The file picker accepts several ABF files at once. Opening a folder recursively
imports every `.abf` below it; each immediate parent folder becomes the initial,
editable cell ID.

Tables can also be pasted straight from the clipboard with
`Ctrl` + `Shift` + `V` — comma-, tab-, or semicolon-delimited text becomes a
new data table.

Importing a table, from a file or the clipboard, first opens a **Review table
import** dialog. It shows each column's inferred type and unit, whether the
column allows empty cells, a preview of the first rows, and any import
diagnostics. Choose **Import table** to add it, or **Cancel** to leave your
project and recent-file list untouched. An XLSX workbook with several sheets
adds a **Worksheet** selector so you can preview each one; a single **Import
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

## Pseudo-2D experiments

DOSY, T1, and T2 experiments are detected automatically from the acquisition
parameters and get their own analysis tools — see
[Pseudo-2D analysis](/guides/pseudo-2d/).

For patch-clamp sweeps, filtering, time-window statistics, stimulus handling,
and IV analysis, see [Electrophysiology](/guides/electrophysiology/).
