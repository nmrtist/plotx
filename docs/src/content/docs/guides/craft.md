---
title: CRAFT for 1D NMR
description: Fit a one-dimensional complex FID to resonance frequencies, amplitudes, linewidths, and phases.
---

CRAFT fits the original complex FID from a one-dimensional NMR acquisition and
reports resonance components directly. This is useful when signals overlap or
when you want to inspect the FID without relying on a fully processed spectrum.

## Run CRAFT

1. Import a one-dimensional NMR acquisition that contains its original complex
   FID. CRAFT is not available for an imported spectrum without an FID or for
   data that is not one-dimensional.
2. If the chemical-shift axis needs calibration, set **Reference** in
   **Processing**, then choose **Process → CRAFT…**. The reference is applied to
   the signal ranges you select and to the reported chemical shifts.
3. Choose an analysis goal:
   - **Explore full bandwidth** looks across the whole acquisition. Treat this
     as exploratory.
   - **Measure selected signals** reports components inside one or more groups
     that you draw on the spectrum.
   - **Compare two signals** requires exactly two non-overlapping groups and
     reports their phase-aware coherent-amplitude ratio.
4. For either selected-signal goal, choose **Select on spectrum**. PlotX opens
   or focuses a frequency-domain spectrum for the dataset. Drag across each
   peak or multiplet to create a signal group.
5. Keep **Conventional FID** for an ordinary acquisition. Choose
   **SSFP / interrupted FID** only when the acquisition uses that sequence.
6. Check the readiness summary. **Ready** means the input passed its checks;
   **Ready with warnings** means it can run but needs review. When the summary
   says **Cannot run**, follow the action shown below it.
7. Choose **Run CRAFT**. The calculation runs in the background; choose
   **Cancel CRAFT** to stop it.

The setup page shows the acquisition carrier, the applied reference shift, the
acquisition duration, and the number of usable points. Invalid FID samples or
acquisition metadata, an invalid reference, overlapping or out-of-band groups,
or too few usable points prevent a run. A short record, no clear signal, or a
crowded group produces a warning instead; inspect the result before relying on
it.

Clear-signal positions appear as ticks on the spectrum. Hover a tick to see its
ppm value. Double-click a tick to create a 90 Hz-wide group. Drag a group's body
to move it or either edge to resize it. Edges snap to nearby clear signals; hold
<kbd>Alt</kbd> while dragging to suppress snapping. Press <kbd>Esc</kbd> to
cancel a drag, <kbd>Delete</kbd> to remove the selected group, or use the arrow
keys to move it by one spectral point (<kbd>Shift</kbd> moves ten). The numeric
fields under **Signal groups** are available when you need exact bounds.

A signal group can contain several fitted components. A component is a fitted
resonance contribution, not a compound identification or a guaranteed visible
multiplet. Fixed modeling windows determine how the FID is solved; the signal
group boundary only determines which completed components belong to the group.

## Advanced component settings

Leave **Advanced component settings** closed for routine work. The conventional
profile uses these defaults:

- **Minimum A/N**: 3.3. Lower values retain weaker candidates but increase the
  chance of fitting noise; values below 3.3 are flagged for review.
- **Maximum model order**: 15 (allowed range 1–64) for each modeling window.
  Reaching the limit is reported as a diagnostic warning.
- **Component linewidth range (Hz)**: 0.05–20 Hz. The bound applies to each
  component, not the frequency range modeled at once. The 20 Hz default is a
  typical starting point rather than a universal constant. Change it only when
  the acquisition and expected line shape justify a different bound.

The fixed modeling bandwidth is 250 Hz for Conventional and 2000 Hz for SSFP.
This is the actual frequency width of a modeling window, not a linewidth and
not a quantitative tuning control. A modeling window can contain many component
lines, each still constrained by the separate component-linewidth range.

Use **Reset** beside an edited value to restore the value inherited from the
selected run or the profile default. Changing profiles keeps the selected
groups and loads the new profile's settings. Conventional FID is always the
default; PlotX does not infer SSFP from the waveform.

CRAFT keeps Bruker acquisition `GRPDLY` separate from its own FIR filtering.
The importer/FFT path uses `GRPDLY` to define the acquisition time origin; the
499-tap CRAFT FIR has an independent edge transient handled by phase-conjugate
precharge. Neither delay is silently folded into the other.

The **SSFP / interrupted FID** profile starts with **Skip initial** at 0.5 ms
and **Extend reconstructed FID** enabled for 1.2 s. Skipping early points can
remove fast-decaying background; reconstruction extends the modeled FID for
that profile. Use SSFP results for screening or relative comparisons unless
the experiment has been validated for quantitative work; a completed fit is
not automatically an absolute qNMR result.

## Review a run

Completed runs are saved with the PlotX project and remain attached to the
source dataset. Open **Results**, select a run, and choose **Open result canvas**
to inspect it in the normal PlotX canvas. The canvas links three views: the
observed and reconstructed spectra, a signal-group comparison, and the complex
residual. They share the horizontal ppm range. **Magnitude** is the default
channel; use the channel control to inspect **Real** or **Imaginary**. Use
**Normalize rows** only to compare shapes, because normalized rows do not show
relative quantitative amplitudes.

Use the result tabs as follows:

- **Overview** shows each group's coherent amplitude and, for exactly two
  groups, their ratio. The ratio uses phase-aware coherent amplitudes rather
  than adding component magnitudes.
- **Signals** lists the fitted components. Filter by signal group, sort by
  chemical shift or amplitude-to-noise ratio, and expand a component to see its
  values and available uncertainties.
- **Diagnostics** shows warnings and fit-quality information. Use **Adjust
  setup** beside a warning to edit the run's settings.

Use **Adjust & rerun…** to edit a run as the starting point for a new run, or
**Rerun unchanged** to repeat it. A run is marked **Stale** when its source FID
or enabled **Reference** step changes; rerun it before interpreting the result.
Runs with warnings or a partial fit are marked **Needs review** even when the
calculation completes.

CRAFT performs deterministic boundary-perturbation checks around each selected
group. Small shifts, expansions, contractions, and one-sided moves must keep
amplitudes and ratios within 1%. A run that fails this stability gate keeps its
complete component table and residual for inspection, but cannot create or
export a quantitative amplitude report.

Choose **Export components…** to open the standard CSV, TSV, XLSX, or clipboard
export dialog. Under **Signals**, **Create data table** creates a sortable PlotX
table without leaving CRAFT. Choose **View data table** to inspect, chart, or
export it, and **Add to board** only when you want the table on a board sheet.

## CRAFT amplitude reports

The model's **Minimum A/N** is a trust criterion for retaining fitted components.
The **Reports** tab is a separate reporting layer: its **Report threshold**
selects which retained components are included, without refitting or changing
the complete component table. **Segment width** is the total fixed frequency
window (Hz) around each selected peak; overlapping windows are merged. Reports
show both the scalar sum of component amplitudes and the phase-aware coherent
amplitude. These segment amplitudes are summaries of fitted components, not
integrals of frequency-domain bins. A report whose source run changes is marked
for review rather than silently recalculated.

## Interpretation

CRAFT components describe the selected FID; they do not identify compounds or
replace concentration calibration. For a quantitative fingerprint, combine
complex component amplitudes within a selected group before taking the
magnitude. Validate the result with an independent acquisition or method,
especially when the run has warnings, reaches a limit, or uses SSFP.
