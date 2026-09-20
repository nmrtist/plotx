---
title: Processing
description: The ordered processing pipeline — apodization, FFT, phase, and baseline correction.
---

Processing in PlotX is an **ordered list of steps** applied to the raw data.
Steps can be added, removed, edited, and reordered at any time — use a step's
⋯ menu (**Move earlier** / **Move later**), or click a step and press
<kbd>Alt</kbd>+<kbd>↑</kbd> / <kbd>Alt</kbd>+<kbd>↓</kbd>. The result is
recomputed and previewed live. Large 2D spectra recompute without blocking the
app — you can keep zooming, panning, and editing while the updated spectrum
appears moments later.

XPS uses its own ordered recipe per spectrum region rather than the NMR
pipeline. It contains an energy window, smoothing, and normalization, while
charge correction is shared by all regions at one measurement position. See
[XPS](/guides/xps/).

## A typical 1D spectrum

A time-domain 1D dataset with known digital-filter delay carries the standard pipeline —
apodization, zero filling, FFT, phase correction, and baseline correction, in
that order — with automatic phasing enabled. In most cases the spectrum on
screen is immediately usable, and a session touches at most three things:

1. **Phase** — if the automatic result is off, open the Phase correction step
   and adjust φ0 / φ1 manually with live preview, or switch the automatic
   method.
2. **Baseline** — baseline correction is off by default; enable the step when
   the baseline rolls or offsets.
3. **Reference** — add a Reference step to pin a known peak to its
   chemical-shift position, typing the source position or picking it directly
   on the spectrum. Peak marks are calibrated with the spectrum: editing the
   Reference step moves existing marks along with the axis. See
   [Reference](#reference).

2D datasets get a cosine-bell apodization enabled by default. A true 2D
acquisition shows two pipelines, **F2 (direct)** then **F1 (indirect)**, in the
order they are processed. A dataset that arrives already transformed is marked
**Imported spectrum** and has no time-domain steps and no FFT: PlotX does not
invent an FID for data it never acquired.

**Reset to default** restores the import settings. Raw data with unknown
filter delay remains uncorrected; imported spectra have no FFT, and spectra
containing only real values have no phase-correction step.

## Check calibration before analysis

Check the axis units before choosing an analysis: FIDs use seconds, while spectra
use Hz or ppm. Missing calibration is not treated as zero.

- **Reference** requires a ppm axis and shifts coordinates without changing intensities.
- **CRAFT** requires a complex FID with known spectral width, observe frequency,
  chemical-shift reference, and digital-filter delay.
- **DOSY maps** require a frequency-domain series calibrated in ppm.
- **Multiplet analysis** requires a ppm spectrum and a known chemical-shift reference
  frequency to report coupling constants in Hz.

The observe frequency and chemical-shift reference frequency serve different
purposes; do not substitute one for the other when interpreting ppm-to-Hz
conversions. For imported Bruker processed spectra, `SF` provides the reference
for converting ppm intervals to Hz. It does not supply missing acquisition
frequency or carrier information, or confirm that digital-filter correction was applied.

## Reconstruct a non-uniformly sampled spectrum

Non-uniform sampling (NUS) records only selected points along the indirect time
axis. PlotX uses the sampling table to reconstruct the full grid before the F1
FFT produces a 2D spectrum. The grid size need not be a power of two.

1. Import a supported Bruker NUS or JEOL acquisition with its sampling table.
   If the table is missing, restore the vendor companion files or use
   [Import NMR with Sampling Table](/guides/importing-data/#supplying-a-missing-nmr-sampling-table).
2. Allow the default processing to finish. PlotX estimates noise automatically;
   you do not need to select a noise region or enter a noise value.
3. Check the status bar for errors before interpreting the result. If reconstruction
   fails during import, the original observations remain available, but their
   display is **not a reconstructed spectrum**. If a later processing change
   fails, the last successful display remains visible.

### Noise and convergence settings

The automatic noise estimate uses the spectrum after the current F2 processing
steps and updates when those steps change. It requires at least 48 observations
and 32 F2 frequency points, or 32 observations and 128 points for an estimate
checked against held-out observations. Meeting these sizes alone does not
ensure a reliable estimate; additional noise-quality checks must also pass.

If you have an independently determined noise standard deviation, enable
**Override automatic noise estimate** under **Non-uniform sampling**. Enter it
in the amplitude units of the spectrum after F2 processing. A value of 0 means
noiseless input, not automatic estimation. Update the value if you change F2
processing in a way that changes the noise scale.

Set the maximum iteration count between 1 and 2048. Reaching this limit without
convergence reports an error. Keep the F2 and F1 FFT steps enabled to obtain
both frequency axes. Noise settings are saved in the project but are not
transferred to other acquisitions by reusable processing recipes.

### Input limitations

Reconstruction requires a sampling table without repeated coordinates and
enough real and imaginary signal information to reconstruct the indirect axis.
Not all acquisition arrangements are supported; an unsupported arrangement
reports an error. Repeated observations can
be imported and saved, but cannot currently be reconstructed; do not delete
repeats from the sampling table to bypass this restriction.

## Where processing lives

Processing opens as a card at the upper right of the canvas — from the
**Process** tab of the Ribbon, or by activating a processing setting found with
<kbd>Ctrl</kbd>+<kbd>K</kbd>. If the active dataset is not an NMR dataset, the
Operation history reads *Select an NMR dataset before opening Processing.* instead.

The card can also host the CRAFT, Regions, Curve Fit, and Statistics tasks.
When two or more tasks are open, tabs appear along the top — **Process**,
**CRAFT**, **Regions**, **Fit**, **Stats** — and one page is shown at a time.
Switching tabs keeps each page's settings; close a page with ✕ to discard them.
Selecting a tab also makes that page's dataset active, so the controls always
belong to the spectrum on screen.

Above the step list, the card names the dataset, its source (**Raw FID**, **2D
acquisition**, or **Imported spectrum**), whether the recipe is still the
default one, and whether the pipeline currently ends in **Time-domain output**
or **Frequency-domain output**. The ⋮ menu holds **Reset to default**, **Load
scheme…**, **Save scheme…**, **Save as template…**, **Apply template…**, and an
**Advanced** section.

## The FFT step

FFT is an ordinary step of type *Time to Frequency*, not a fixed divider in the
list. Add it from **Add step → Time domain → FFT · Time to Frequency**, and
delete, disable, or move it like any other step. A pipeline holds at most one,
so **Duplicate** is unavailable on it.

Remove or disable it on a raw acquisition and the canvas switches to the FID,
plotted against acquisition time in seconds. The frequency-domain steps stay in
the recipe, marked *Disabled: requires Frequency input*, so adding FFT back
restores their settings rather than making you re-enter them.

Time-domain steps run in the order shown, on either side of that choice: two
zero fills compose, and an apodization moved below zero filling sees the padded
length.

While the output is an FID, the analysis that needs a spectrum is disabled and
says why — peak detection and the peak list, line fitting, integrals, and
multiplets all ask you to plot frequency-domain data first. Peaks and integrals
you already have are kept, not discarded, and come back with the FFT step.

## Available steps

- **Apodization** (window function)
- **Zero filling**
- **FFT**
- **Phase correction**
- **Baseline correction**
- **Reference** (chemical-shift referencing)
- **Magnitude**
- **Smoothing** (moving average or Savitzky-Golay)
- **Normalize** (largest peak, total area, or a constant divisor)
- **Binning** (aggregate points into bins of a given ppm width)
- **Reverse** (mirror the intensities along the axis)
- **Invert** (multiply intensities by −1)

## Cleanup steps

The cleanup steps (smoothing, normalize, binning, reverse, invert) work on the
spectrum after the FFT, and are available for 1D spectra. Add them from the
*Add step* menu's **Cleanup** group and reorder them freely among the other
frequency-domain steps.

- **Smoothing** — moving average, or Savitzky-Golay least-squares
  polynomial smoothing with adjustable odd window and polynomial order.
- **Normalize** — scale so the tallest peak is 1, so the absolute
  integral is 1, or divide by a constant of your choice.
- **Binning** — merge neighboring points into bins of a fixed axis width,
  summing or averaging each bin; the axis is rebuilt from the bin centers.
- **Reverse** — mirror the intensities along the axis.
- **Invert** — flip the sign of every point.

## Group-delay correction

Some spectrometers record a digital filter delay at the start of the FID that
shows up as distorted first points. Digital group-delay correction removes it;
it is **Group-delay correction**, a per-dataset switch under **Advanced** in the
Processing card's ⋮ menu, applied before the pipeline. It governs 1D and 2D
data alike: switch it off on a 2D dataset and the direct dimension is left
uncorrected too.

For Bruker data, a nonnegative `GRPDLY` supplies the delay, including zero.
If it is missing or -1, PlotX uses supported `DSPFVS`/`DECIM` settings to determine
the delay. Otherwise the delay remains unknown and raw data initially appears
as an FID. To view an uncorrected spectrum, turn off **Group-delay correction**
under **Advanced** and enable FFT. Review the result for filter-related distortion;
disabling correction does not establish the delay required by CRAFT.

## Apodization

Click the **Apodize** step to open its settings. All of them are shown at once,
under an **Apodization** heading:

- **Window** — *None*, *Cosine bell*, *Exponential*, or *Gaussian*.
- **LB** — line broadening in Hz, offered for an exponential or Gaussian window.
  It accepts −10000 to 10000, and the negative half is deliberate: under a
  Gaussian window a positive LB narrows lines — the Lorentz-to-Gauss resolution
  enhancement — and a negative one broadens them further.
- **GB** — Gaussian broadening in Hz, offered only for a Gaussian window. It
  must be **greater than 0**, up to 10000. At zero the Gaussian term vanishes
  and what is left grows without limit instead of decaying, so it is not a
  window at all.

Switching a step to an exponential or Gaussian window starts any broadening it
did not already carry at 1 Hz.

Dragging **LB** or **GB** previews continuously but leaves a single undo step
for the whole drag, so one <kbd>Ctrl</kbd>+<kbd>Z</kbd> takes back the gesture
rather than one frame of it.

### Changed values and reset

A value that differs from the one the standard pipeline for this dataset puts in
the step is marked with a dot. Hover the dot to see the default; use the reset
button beside it to go back.

A step you added yourself has no such default — nothing in the standard pipeline
corresponds to it — so it carries no marker and no reset button. Asking for a
reset through [Automation](/guides/automation/) reports that step as skipped
rather than turning a window you chose into *None*.

### While auto-recompute is paused

**Pause auto-recompute**, in the Processing card's ⋮ menu under **Advanced**,
governs these rows as well. Change **Window**, **LB** or **GB**
while it is on and the recipe records the change without recomputing: the panel
shows **Changes pending** with an **Apply** button, and nothing is recalculated
until you press it.

### Finding these settings

- <kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>Cmd</kbd> on macOS) searches settings as
  well as commands and data. `LB`, `line broadening`, `apodization window` and
  `gaussian broadening` all reach these rows; activating one opens Processing,
  expands the first step that carries the setting and highlights it. See
  [Command palette](/reference/command-palette/).
- **Apodization settings** in the **Processing** group of the Ribbon's
  **Process** tab goes to the same place.

## Phase correction

Automatic phase correction is enabled by default; you can switch methods or
adjust φ0 / φ1 manually with live preview.

Open the **Phase** step and its four rows sit together: **Mode**, **φ0**, **φ1**
and **Pivot**. φ0 and φ1 are in degrees. The pivot is a fraction of the axis
from 0 to 1, with the ppm position it currently lands on shown beside it. While
the step is open a pivot handle is drawn on the spectrum, and dragging it there
places the pivot in ppm.

φ0, φ1 and the pivot are editable only while **Mode** is *Manual*; under an
automatic method each row says which switch to flip first.

Through [Automation](/guides/automation/) these values keep their own units:
phase angles in radians, the pivot as a fraction.

For 2D data, the selected automatic method estimates a correction from the trace
containing the strongest real or imaginary signal and applies the same correction
across the series, preserving relative row signs. If it fails, review the error
and choose another method or adjust the phase manually.

For JEOL COSY, both F2 and F1 support phase correction and Reference. An extracted
row follows F2; a column follows F1. Both retain real and imaginary values for
further phase correction.

## Baseline correction

Baseline correction is off by default. Enable the step when your spectrum
needs it.

## Reference

A Reference step shifts the whole chemical-shift axis so the point currently at
**Source** reads **Target**: pin your standard (TMS, DSS, a solvent line) to
its known position. Add it from **Add step → Frequency domain → Reference**.

You can type the source position, or pick it on the spectrum: open the step and
click **Pick position on spectrum**. Hovering the plot previews the position —
the pick snaps to the nearest peak apex within a small window around the
cursor, so zooming in first lets you land on a weak line precisely. Hold
<kbd>Shift</kbd> to skip the snap and read the exact cursor position instead.
Click to fill **Source**, then type the shift that peak should have into
**Target**. <kbd>Esc</kbd>, collapsing the step, or switching datasets cancels
an unused pick.

Peak marks are calibrated with the spectrum, so editing a Reference step moves
existing marks along with the axis.

## Reusing a pipeline

A pipeline (including the group-delay setting) can be saved as a portable
`.plotxproc` recipe file or as a named template and reapplied to other
datasets — see [Recipes and templates](/guides/templates/). To apply one
action to many datasets at once, or to run a whole import → process → export
workflow, see [Automation](/guides/automation/).

To reduce the original complex FID directly to fitted resonance parameters,
see [CRAFT for 1D NMR](/guides/craft/).
