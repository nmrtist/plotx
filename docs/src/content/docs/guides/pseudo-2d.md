---
title: Pseudo-2D analysis
description: DOSY, T1, and T2 relaxation analysis and curve fitting.
---

A pseudo-2D dataset is a stack of 1D spectra acquired while one parameter
varies. PlotX reads that varying parameter from the acquisition parameters on
import: a gradient-strength series marks the dataset as DOSY, a delay series
as relaxation (T1 or T2 — which one is your choice of fit model).

## Workflow

1. Import the pseudo-2D dataset.
2. On the **Analyze** tab, choose **Draw Regions** in the **Regions** group,
   then draw over the peaks of interest.
3. Choose **Continue to Series Table** in the Regions task card.
4. The **Curve Fit** task card opens with the series table. Check the suggested
   model, choose whether to fit every curve or one selected curve, then choose
   **Run Fit**.

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
