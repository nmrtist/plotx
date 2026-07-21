---
title: Your first figure
description: From a raw dataset to an exported, publication-ready figure in one sitting.
---

This walkthrough takes one 1D NMR dataset from import to an exported figure.
Every step generalizes: the same flow works for a Bruker folder, a JCAMP-DX
file, a patch-clamp recording, or a plain CSV table.

## 1. Import

Drag your data file — for example a JEOL `.jdf` — onto the PlotX window, or
choose **Open File…** from the **File** menu. For a Bruker TopSpin experiment,
use **Open Folder…** and pick the experiment directory instead.

The dataset appears in the Primary Side Bar and is placed on the board
automatically, already plotted on its own page.

## 2. Process

Open the **Process** tab of the Ribbon. A new 1D dataset already carries the
standard pipeline — apodization, zero filling, FFT, phase correction, and
baseline correction — with automatic phasing enabled, so in most cases the
spectrum on screen is already usable.

To adjust it, edit the steps in the processing panel: every change is
previewed live and can be undone. Two edits cover most sessions:

- If the baseline rolls, enable the **Baseline correction** step.
- If the automatic phase is off, open the **Phase correction** step and adjust
  φ0 / φ1 with live preview.

See [Processing](/guides/processing/) for the full list of steps.

## 3. Analyze

Press `P` to select the **Peaks** tool, then drag the threshold line on the
plot: peaks above it are detected when you release. Add, move, or delete peak
marks by hand where the automatic detection needs help.

Extracted values — peak lists, integrals, fit parameters — land in data tables
linked to the spectrum. This walkthrough stops at peaks; the
[analysis tool overview](/guides/choosing-a-tool/) shows what else is
available and when to reach for it.

## 4. Lay out the page

Click the size chip above the page's top-left corner (or **Canvas Size &
Settings…** on the **Figure** Ribbon tab) and pick your target journal's
single-column preset. The page width snaps to the journal's artwork
specification; axis text is already at printed size.

Add a panel label or annotation if you need one — press `T` for the Text tool
and click the canvas. See [Annotations](/guides/annotations/).

## 5. Export

Open the export menu in the toolbar and choose a vector format — SVG for
further editing, PDF for manuscripts — or a raster preset such as
*Single column · 89 mm · 600 dpi · TIFF*. The export precheck warns if font
sizes or line widths violate the chosen preset before anything is written.

For a quick paste into a document, `Ctrl` + `C` copies the selected frame to
the clipboard as bitmap and vector at once.

## 6. Save the project

`Ctrl` + `S` saves everything — data, processing, analysis, and layout — as a
single `.plotx` file you can reopen later or send to a colleague. See
[File formats](/reference/file-formats/).
