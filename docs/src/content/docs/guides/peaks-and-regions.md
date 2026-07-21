---
title: Peaks and regions
description: Peak picking and interactive region analysis.
---

Not sure whether you need peaks, regions, a peak fit, or a curve fit? Start
with [Choosing an analysis tool](/guides/choosing-a-tool/).

## Peak picking

The **Peaks** tool detects peaks by prominence. Drag the threshold line on
the plot to adjust detection — peaks are recomputed when you release it.
Detected peaks can be edited, added, and removed by hand.
Choose **Export Data…** and **Peak table** to save or copy the current peak list.

## Regions

Regions measure the same x-axis interval across every member of a series. This
is useful whenever you want to follow a signal through an ordered series,
including DOSY, relaxation, and other series data.

Open the **Analyze** tab and choose **Draw Regions** in the **Regions** group,
then drag across each signal of interest. A Regions task card opens at the
upper-right of the canvas with the drawing instructions, measurement choice,
and region list. Drag the handle at the lower-right corner of the card to
adjust its height. The regions remain on the plot and can be moved or resized.

When the regions are ready, choose **Continue to Series Table** in the task
card. Each region becomes a column in a live table that stays synchronized with
later region changes. The **Series Table** button in the Ribbon opens the same
table. Use **Save Snapshot** only when you need an independent copy that will
not change with the regions.

To export either the linked table or a frozen Series in full, select that table
and use **Export Data…** → **Complete typed table / series**.

For pseudo-2D analysis, each region becomes one decay curve — see
[Pseudo-2D analysis](/guides/pseudo-2d/).

## Peak fitting

The **Peak Fit** tool (`D`) deconvolves a region of any dataset with a 1D
trace — an NMR spectrum or an imported table/CSV spectrum — into
overlapping Lorentzian, Gaussian, or pseudo-Voigt components. Drag across
the region to set the fit range, pick a shape, and press **Run Peak Fit**. The fit
runs in the background and you can keep working; **Cancel** discards it.
Peak marks inside the range seed the components; without any, peaks are
auto-detected. A single fit handles at most 24 components — narrow the
range or remove peak marks if more fall inside.

Each fit stores per-peak position, height, width, and area with standard
errors and draws the total and per-component curves over the spectrum.
Fits stay in the panel until you choose **Add result to board**, which
creates the parameter table on its own page. Fits can be deleted
individually; everything is undoable.

## Multiplet analysis

With the **Peak Fit** tool, select a region on a 1D spectrum and press
**Analyze multiplets**. The peaks in the range — fitted components when a
fit covers them, peak marks otherwise — are grouped into multiplets and
classified as singlet, doublet, triplet, quartet, doublet of doublets, or
unresolved multiplet (s/d/t/q/dd/m). Coupling constants are reported in
Hz; the summary table includes their uncertainties when the fit provides
them for every multiplet.

Results land in a summary table on a new canvas, and each multiplet is
listed in the panel as a copyable journal-style descriptor such as
`2.35 (dd, J = 12.0, 4.0 Hz)`; **Copy all** joins them into one listing.
A single undo removes the whole analysis.
