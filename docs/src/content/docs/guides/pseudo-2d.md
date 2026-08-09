---
title: Pseudo-2D analysis
description: DOSY, T1, and T2 relaxation analysis and curve fitting.
---

A pseudo-2D dataset is a stack of 1D spectra acquired while one parameter
varies. PlotX reads that varying parameter from the acquisition parameters on
import: a gradient-strength series marks the dataset as DOSY, a delay series
as relaxation (T1 or T2 — which one is your choice of fit model).

The initial stack contains one plot series per increment. Select the plot and
use **Data** in the Object inspector to show or hide individual increments.
Use **Choose trace…** to replace a series with another increment from the same
dataset. **Add series…** adds every increment from a compatible dataset. Use
**Show all**, **Hide all**, the row checkboxes, or remove buttons to keep the
exact values you want to compare. To identify them on the plot, set
**Visibility** to **Show** under **Legend & scales**. The legend labels each
increment by its gradient strength, relaxation delay, or imported pseudo-axis
value and display unit.

To compare increments from multiple compatible datasets, select the datasets
in the Data browser and choose **Stack selected data**. The trace composer
starts with every increment included. Search by dataset, increment label, or
parameter value and adjust the row checkboxes, or use the global and filtered
selection controls. **Create stack** creates one plot from the chosen
increments; **Cancel** leaves the project unchanged. This remains available
while a dataset displays a DOSY map because the composer selects stable stack
increments rather than copying data from the displayed map.

To align peaks in a stack on the canvas, select the plot and choose **Align
Traces…** in the **Align** group on the **Analyze** tab. You can also choose
**Align traces…** under **Data** in the Object inspector. Choose a reference
increment and a peak window, then review the proposed shifts before selecting
**Apply**. Hidden increments and increments without a usable peak are listed as
**Skipped** and remain unchanged. **Trace start** is also available when you
need to align the first finite plotted sample instead of a peak.

For a manual correction, edit **X shift (ppm)** on the increment's line row.
These undoable shifts belong to the plot, so the processed spectra and DOSY or
ILT maps remain unchanged.

## Workflow

1. Import the pseudo-2D dataset.
2. On the **Analyze** tab, choose **Draw Regions** in the **Regions** group,
   then draw over the peaks of interest.
3. Choose **View extracted curves** in the Regions task card. PlotX selects and
   zooms to the new scatter plot, with one point series per region.
4. The **Curve Fit** task card opens beside the result. Check the suggested
   model, choose whether to fit every curve or one selected curve, then choose
   **Run Fit**. Use **View data** for the synchronized read-only values or
   **Back to regions** to return to the source.

## Fit models

- Mono-exponential decay
- Inversion recovery (T1), `a + b·exp(−x/T)`
- Saturation recovery
- Stejskal–Tanner diffusion decay (DOSY)
- Bi-exponential and stretched-exponential
- Linear

For DOSY, the **Experiment** card on the **Analyze** tab also offers a
regularized inverse Laplace transform (ILT), which produces a full
chemical-shift × diffusion map. Map builds run in the background and you can
keep working; **Cancel** discards one. A map cannot be rebuilt for the same
dataset until the current build finishes or is cancelled. Changing the
dataset's processing while a map is building cancels the build — the map would
no longer match the spectrum. The status bar tells you when that happens;
simply rebuild the map after the processing change. Changing the processing
also discards a finished map for the same reason, so the plot falls back to the
stack until you rebuild.

The ILT settings offered for the next build resolve in this order: a value you
type into the **Experiment** card for this dataset wins; otherwise the dataset's
previous ILT result supplies the settings it was built with; otherwise **λ**
comes from **Preferences → Processing** and the grid settings from PlotX's own
defaults. **λ** accepts `0.000001` to `1000`.

Both kinds of DOSY map are saved in the `.plotx` project along with your
**Show** and **DOSY method** choices, so reopening a project puts the same plot
back on screen without rebuilding. If a saved map was built from data that the
project's stored processing no longer reproduces, PlotX keeps showing the saved
map and warns you — in the status bar as the project opens, and in the
**Experiment** card. If a saved map cannot be read back at all, PlotX shows the
stack instead and says so in the same two places. Rebuilding the map clears the
warning.

Choosing **DOSY map** under **Show** while the selected **DOSY method** has no
map yet also draws the stack, and the **Experiment** card names the map to
build.

## Notes on intensities

Extracted intensities are signed (phased real-part projections), not
magnitudes, so inversion-recovery data fits correctly without folding.
