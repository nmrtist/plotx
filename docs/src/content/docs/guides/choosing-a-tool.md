---
title: Choosing an analysis tool
description: Which tool answers which question — peaks, regions, integrals, peak fitting, curve fitting, and statistics.
---

PlotX has several analysis tools with overlapping-sounding names. Start from
what you want to measure:

| You want to… | Use | Works on | Result |
| --- | --- | --- | --- |
| Locate and list the peaks in a spectrum | **Peaks** (`P`) | 1D spectra | Peak list |
| Follow a signal's intensity through an ordered series | **Regions** | Pseudo-2D and stacked series | Live series table, one column per region |
| Measure cross-peak volumes in a 2D spectrum | **Integrate** (`I`) | True 2D spectra (COSY, HSQC, …) | Integral table with normalized volumes |
| Separate overlapping spectral peaks into components | **Peak Fit** (`D`) | Any 1D trace | Per-peak position, height, width, area |
| Group fitted peaks into multiplets with J couplings | **Analyze multiplets** (in Peak Fit) | 1D spectra | Multiplet table and journal-style descriptors |
| Fit x-y data to a mathematical model | **Fit Curves** | Data tables (including series tables) | Parameters with uncertainties and diagnostics |
| Compare groups, test correlation or normality | **Statistics** | Data tables | Saved statistical results |

Rules of thumb when two tools seem to apply:

- **Peaks vs Peak Fit** — Peaks marks positions; Peak Fit deconvolves
  overlapping lines into Lorentzian/Gaussian/pseudo-Voigt components with
  areas and uncertainties. Pick marks first, fit when peaks overlap or you
  need quantitative areas.
- **Regions vs Integrate** — Regions measures the *same 1D interval* across
  every member of a series; Integrate measures a *rectangle* in a single true
  2D spectrum. A DOSY or relaxation dataset is a series → Regions; an HSQC is
  a true 2D spectrum → Integrate.
- **Peak Fit vs Fit Curves** — Peak Fit works on a spectrum's line shapes;
  Fit Curves works on tabulated x-y values (a decay, a titration, an IV
  curve). A pseudo-2D analysis uses both stages: Regions extracts the decay
  curves into a table, Fit Curves fits them.

Where to go next: [Peaks and regions](/guides/peaks-and-regions/),
[2D volume integration](/guides/2d-integration/),
[Curve fitting](/guides/curve-fitting/), [Statistics](/guides/statistics/),
and the per-technique guides for [pseudo-2D](/guides/pseudo-2d/) and
[electrophysiology](/guides/electrophysiology/).
