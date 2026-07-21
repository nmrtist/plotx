---
title: Troubleshooting
description: What to check when an import, analysis, export, or update doesn't behave.
---

## Importing

**My ABF file won't open.** PlotX reads ABF 2.x with int16 or float32
samples. ABF1 files and compressed ABF2 data are not currently supported —
re-export the recording as uncompressed ABF2 from your acquisition software.

**A numeric column imported as text.** PlotX only types values automatically
when they are unmistakable: `true`/`false`, base-10 integers, `YYYY-MM-DD`
dates, and UTC timestamps. Locale-specific formats (comma decimals, regional
dates) and columns that mix numbers with text stay as text rather than being
guessed at. Check the **Review table import** dialog's diagnostics, fix the
source file, or convert the column after import.

**Cells from my Excel sheet came in empty.** PlotX reads the value Excel
cached for each formula but never recalculates formulas. A formula cell with
no cached value imports as empty and is listed in the import diagnostics —
open the workbook in Excel once and save it to refresh the cached values.

## Processing and analysis

**Peak Fit refuses to run with too many components.** A single fit handles at
most 24 components. Narrow the fit range or remove peak marks so fewer fall
inside.

**My DOSY map build was cancelled.** Changing a dataset's processing while a
map is building cancels the build, because the map would no longer match the
spectrum. Finish the processing changes, then rebuild the map.

**A statistics or window-statistics result reports an error instead of a
number.** PlotX refuses to fabricate values from empty windows, non-finite
samples, or excluded cells; the message states what was wrong. Adjust the
window or confirm the exclusion it proposes.

## Exporting

**The export precheck flags my figure.** Font sizes or line widths violate
the selected journal preset. Fix them on the board — **Figure Typography…**
sets all axis text sizes at once — or pick a preset that matches your
intended size.

## Projects and updates

**"Unsupported version" when opening a project.** The file was saved by a
different PlotX with an incompatible project format (possible between
releases until 1.0). Your file is intact — update PlotX and try again.

**I overwrote a project by mistake.** If **Preferences → General → Project
backup copies** is on, previous saves are kept as hidden files beside the
project file.

**No update arrives.** Failed background checks (for example, offline) are
silent by design. Use **Check now** in **Preferences → General** to get an
explicit result, and check your update channel — switching to a more stable
channel never downgrades, it waits for the next release.

## Reporting a problem

If none of this helps, report the issue on the
[GitHub issue tracker](https://github.com/nmrtist/plotx/issues) with your
PlotX version (shown in **Preferences → General**), your platform, and, when
possible, a file that reproduces the problem.
