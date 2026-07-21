---
title: Electrophysiology
description: Import ABF2 recordings, inspect sweeps, measure responses, and build IV tables.
---

PlotX imports ABF 2.x recordings as native electrophysiology datasets. It
supports int16 and float32 samples, one or more recorded channels, fixed or
variable-length sweeps, ADC scaling, channel names and units, and DAC epoch
waveforms. ABF1 and compressed ABF2 data are not currently supported.

## Sweeps and filtering

The default chart overlays every sweep from the selected channel against time.
Use **Patch clamp** in Dataset tools to select or clear individual sweeps and
choose the recorded channel. The optional zero-phase Gaussian low-pass is
enabled at 1 kHz by default. It affects charts and analysis consistently; raw
samples remain unchanged and the setting is saved in the project.

## Window statistics

Enter the start and end time in seconds and choose Positive, Negative, or
Absolute peak mode. **Create statistics table** creates a normal PlotX data
table containing signed peak, average, and peak time for every selected sweep.
An empty window or non-finite sample produces an error instead of a fabricated
zero. Use the normal Data Sheet and **Export Data…** to inspect or export results.

For the recording itself, **Export Data…** writes every selected sweep from the
current channel after the active filter. Time is the first column and each
sweep is a following column; shorter sweeps leave empty cells at the end.

## Stimulus and IV

**From ABF** means the command came from the file's DAC/epoch sections. If the
file does not contain a waveform, PlotX may suggest a Voltage Step, Current
Step, or Ramp from the protocol name. Suggested values are placeholders: edit
them and explicitly confirm the template before IV analysis is enabled.

**Create IV table** combines the stimulus value with peak and average response.
Voltage stimuli require a current response; current stimuli require a voltage
response. A unit mismatch is reported and calculation is stopped. Ramp protocols
do not support IV analysis: the stimulus varies continuously within a sweep, so
there is no single stimulus value to plot against. In the data browser the
table stays listed under the recording it came from, and its stimulus source
remains part of the saved dataset.

## Recording metadata

Cell ID, experiment, label, seal resistance, leak current, capacitance, and
series resistance are editable in Dataset tools and persist in `.plotx`.
