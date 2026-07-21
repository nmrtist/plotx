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

## Phase correction

Automatic phase correction is enabled by default; you can switch methods or
adjust φ0 / φ1 manually with live preview.

## Baseline correction

Baseline correction is off by default. Enable the step when your spectrum
needs it.

## Reusing a pipeline

A pipeline (including the group-delay setting) can be saved as a portable
`.plotxproc` recipe file or as a named template and reapplied to other
datasets — see [Recipes and templates](/guides/templates/). To apply one
action to many datasets at once, or to run a whole import → process → export
workflow, see [Automation](/guides/automation/).
