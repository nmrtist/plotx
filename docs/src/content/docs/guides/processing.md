---
title: Processing
description: The ordered processing pipeline — apodization, FFT, phase, and baseline correction.
---

Processing in PlotX is an **ordered list of steps** applied to the raw data.
Steps can be added, removed, edited, and reordered (via each row's
*Move up* / *Move down* menu) at any time; the result is recomputed and
previewed live. Large 2D spectra recompute without blocking the app — you
can keep zooming, panning, and editing while the updated spectrum appears
moments later.

## A typical 1D spectrum

A newly imported 1D dataset already carries the standard pipeline —
apodization, zero filling, FFT, phase correction, and baseline correction, in
that order — with automatic phasing enabled. In most cases the spectrum on
screen is immediately usable, and a session touches at most three things:

1. **Phase** — if the automatic result is off, open the Phase correction step
   and adjust φ0 / φ1 manually with live preview, or switch the automatic
   method.
2. **Baseline** — baseline correction is off by default; enable the step when
   the baseline rolls or offsets.
3. **Reference** — add a Reference step to pin a known peak to its
   chemical-shift position.

2D datasets get a cosine-bell apodization enabled by default. Datasets that
arrive already transformed (frequency-domain data) get a pipeline without the
time-domain steps.

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
it is a per-dataset switch next to the step list, applied before the pipeline.
It governs 1D and 2D data alike: switch it off on a 2D dataset and the direct
dimension is left uncorrected too.

## Apodization

Click the **Apodize** row in the step list to open its settings. All of them are
shown at once, under an **Apodization** heading:

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

**Pause auto-recompute**, in the ⋮ menu at the top of the Processing panel under
**Advanced**, governs these rows as well. Change **Window**, **LB** or **GB**
while it is on and the recipe records the change without recomputing: the panel
shows **Changes pending** with an **Apply** button, and nothing is recalculated
until you press it.

### Finding these settings

- <kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>Cmd</kbd> on macOS) searches settings as
  well as commands and data. `LB`, `line broadening`, `apodization window` and
  `gaussian broadening` all reach these rows; activating one opens the
  Processing panel, expands the first step that carries the setting and
  highlights it. See [Command palette](/reference/command-palette/).
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

## Baseline correction

Baseline correction is off by default. Enable the step when your spectrum
needs it.

## Reusing a pipeline

A pipeline (including the group-delay setting) can be saved as a portable
`.plotxproc` recipe file or as a named template and reapplied to other
datasets — see [Recipes and templates](/guides/templates/). To apply one
action to many datasets at once, or to run a whole import → process → export
workflow, see [Automation](/guides/automation/).
