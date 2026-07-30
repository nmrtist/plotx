---
title: Homonuclear 2D symmetry review
description: Compare cross-diagonal partners, pick related peaks, and review artifact suggestions in COSY, TOCSY, NOESY, and ROESY spectra.
---

In a homonuclear 2D spectrum, a cross peak at `(a, b)` often has related
evidence near `(b, a)`. **Symmetry review** keeps both positions visible
so you can compare them without repeatedly reading and swapping coordinates.
The reflected position is evidence, not proof: real peaks can be asymmetric
or missing, and artifacts can also look symmetric.

The tool is available for true-2D COSY, TOCSY, NOESY, and ROESY spectra when
both frequency axes use the same nucleus and their chemical-shift ranges overlap.
It is not offered for heteronuclear, pseudo-2D, or stacked data.

## Inspect one signal

1. Select the spectrum, then choose **Analyze → Review → Symmetry review**.
   From outside the cursor family, you can also press `C` three times:
   **Inspect**, **Delta**, then **Symmetry review**.
2. Move over a cross peak. The solid marker is the position under the cursor;
   the dashed marker is its reflection across the diagonal.
3. Hold `Shift` to snap temporarily to a nearby candidate. Enable **Snap
   automatically** if you want snapping on continuously.
4. Click to pin the comparison. A partner outside the visible viewport is
   reported at the plot edge; PlotX does not move the viewport automatically.

The readout reports both coordinates and intensities. After the symmetry audit
finishes, it also reports signal-to-noise for a detected pair or explains the
comparison status:

- **partner found** — one clear candidate was associated with the reflected
  position.
- **ambiguous** — more than one plausible candidate is nearby.
- **no counterpart detected** — the reflected position is within the acquired
  range, but no candidate met the association criteria.
- **partner outside acquired range** — the reflected coordinate cannot be
  evaluated from this spectrum.
- **on diagonal** — the two positions coincide and do not form an independent
  cross-diagonal pair.

## Audit the spectrum

Activating **Symmetry review** starts a spectrum-wide audit and lists candidate
pairs in the **Symmetry review** panel. If the audit does not start
automatically, choose **Run symmetry audit**. The results are review
suggestions, not decisions about whether a peak is genuine.

Use **Show** to filter the list to **Paired**, **Unpaired**, **Ambiguous**, or
**Suggestions**. Selecting a row pins that comparison on the plot.

## Record decisions

- **Pick both peaks** stores the pinned peak and its detected partner as a
  reciprocal pair.
- **Pick paired** stores all paired results from the audit.
- **Mark possible artifact** flags the pinned position for review.
- **Mark suggestions** flags the audit's review suggestions.

Set a stored mark to **Confirmed**, **Uncertain**, or **Possible artifact**,
or remove it from the list. These edits support Undo and Redo. Cross-peak
coordinates, reciprocal links, and review states are saved in the `.plotx`
project. The pinned cursor position and audit results are not saved.
