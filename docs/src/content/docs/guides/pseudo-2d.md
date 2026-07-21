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

For DOSY, a regularized inverse Laplace transform (ILT) is also available
and can produce a full chemical-shift × diffusion map. Map builds run in the
background and you can keep working; **Cancel** discards one. A map cannot be
rebuilt for the same dataset until the current build finishes or is cancelled.
Changing the dataset's processing while a map is building cancels the build —
the map would no longer match the spectrum. The status bar tells you when
that happens; simply rebuild the map after the processing change.

## Notes on intensities

Extracted intensities are signed (phased real-part projections), not
magnitudes, so inversion-recovery data fits correctly without folding.
