---
title: Combining and aligning spectra
description: Spectrum arithmetic and multi-spectrum alignment.
---

Two tools operate across spectra rather than on one: **Spectrum arithmetic**
builds a new spectrum from two existing ones, and **Align spectra** shifts a
whole series so a reference peak lines up. Both are opened from the Analysis
panel of a 1D spectrum or from the command palette
(<kbd>Ctrl</kbd>+<kbd>K</kbd>).

## Spectrum arithmetic

**Spectrum arithmetic** combines 1D spectra into a new, independent
dataset — the source datasets are never modified.

Four operations are available:

- **A + k·B** and **A − k·B** — add or subtract a second spectrum,
  scaled by a coefficient *k* (default 1). Tuning *k* is how you null a
  solvent line in a subtraction, or balance a difference spectrum.
- **A × k** — multiply a spectrum by a constant.
- **A + c** — add a constant offset.

The two operands must share the same nucleus; mixed-nucleus pairs are
rejected with the reason shown in the dialog. If the ppm axes differ
(different point counts or ranges), the second spectrum is linearly
interpolated onto the first one's axis, and points outside the overlap
are treated as zero — the dialog notes when this will happen.

The result is an independent frequency-domain spectrum placed on its own page,
named after the expression (for example `A − 0.9·B`). It behaves like
any other 1D spectrum — peaks, integrals, regions, and export all work —
and creating it is a single undoable step.

## Aligning spectra

**Align spectra** shifts a series of 1D spectra so a shared reference
peak lands on one chemical-shift position — useful for reaction
monitoring, titrations, and other series where a reference line drifts
between acquisitions.

The dialog works on your Data-list multi-selection (or every dataset
when fewer than two are selected):

- Pick a **ppm window**; it defaults to the current view of the active
  spectrum. Each spectrum's tallest significant peak inside the window
  becomes its reference peak.
- Pick the **target**: the reference spectrum's own peak position (the
  active dataset leads), or a ppm value you type.
- A preview table lists every dataset with the peak found and the shift
  that will be applied. Spectra with no significant peak in the window,
  a different nucleus, or a non-1D type are skipped, with the reason
  shown.

The shift is written through each spectrum's **Reference** step — the
same chemical-shift referencing used for a single spectrum — so it stays
visible and editable in the pipeline afterwards. Applying the alignment
is a single undoable step across all datasets.
