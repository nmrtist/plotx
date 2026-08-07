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
Select the plot, then use **Data** in the Object inspector to show or hide
individual sweeps. Use **Choose trace…** on a plot series to replace it with a
different stimulus from the same recording. **Add series…** adds every sweep
from the selected compatible recording. Use **Show all**, **Hide all**, the row
checkboxes, or remove buttons to reduce the stack to the voltages or currents
you want to compare. **Stack selected data** likewise starts with all sweeps
from every selected recording. These changes affect only the plot. In Dataset
tools, use **Patch clamp** to choose the recorded channel and the sweeps
included in region measurements, window statistics, IV tables, and data
exports. The optional zero-phase Gaussian low-pass is enabled at 1 kHz by
default. It affects charts and analysis consistently; raw samples remain
unchanged and the setting is saved in the project.

The legend is hidden for a newly imported recording. To identify sweeps on the
plot, set **Visibility** to **Show** under **Legend & scales**. A sweep with an
ABF DAC waveform is labeled with its command value and unit. Otherwise, PlotX
uses a confirmed voltage-step or current-step template when available, then
falls back to `Sweep n`. The command unit does not depend on the recorded
response channel. In a multi-epoch protocol, PlotX uses the command epoch that
varies between sweeps, so a fixed prepulse does not replace the test-pulse
value. **Legend size** and **Legend text color** under **Figure
typography** style legends throughout the document. With **Select** active,
drag the legend to a clear part of the plot; double-click it to restore
automatic placement.

## Regions and window statistics

Select the recording, then choose **Analyze** → **Draw Regions**. Drag across
the trace to mark one or more time windows. Each window is measured in every
selected sweep. Choose Height, Area, Max, Min, or Mean, then select
**View extracted curves**. PlotX selects and zooms to a scatter plot with one
color-matched series per region. The synchronized data table stays under the
recording in the Data browser without adding a second board frame. Use **Fit
curves**, **View data**, or **Back to regions** in the Curve Fit task card.
The data opens read-only; choose **Save Snapshot** in the Regions task card, or
**Save editable snapshot** in the data sheet, to create an independent table.
The synchronized table keeps the sweep membership captured when you create it;
changing the current analysis selection does not rewrite existing table rows.

For peak, average, and peak-time values, open **Patch clamp**. PlotX uses the
selected region, or the first region in the list when none is selected. Choose
Positive, Negative, or Absolute under **Peak mode**, then select **Create
statistics table**. The button is disabled until you draw a region. If the
window does not overlap a sweep or contains a non-finite sample, PlotX reports
an error instead of inserting zero. Inspect the result in Data Sheet or export
it with **Export Data…**.

**Show regions on figure and export** is enabled by default. With it enabled,
figure exports include each region's colored band, boundary, and label.

For the recording itself, **Export Data…** writes every selected sweep from the
current channel after the active filter. Time is the first column and each
sweep is a following column; shorter sweeps leave empty cells at the end.

## Stimulus and IV

**From ABF** means the command came from the file's DAC/epoch sections. If the
file does not contain a waveform, PlotX may suggest a Voltage Step, Current
Step, or Ramp from the protocol name. Suggested values are placeholders: edit
them and explicitly confirm the template before IV analysis is enabled.

**Create IV table** uses the same selected region and combines the stimulus
value with the peak and average response. Voltage stimuli require a current
response; current stimuli require a voltage response. A unit mismatch is
reported and calculation is stopped. Ramp protocols do not support IV
analysis: the stimulus varies continuously within a sweep, so there is no
single stimulus value to plot against. In the data browser the table stays
listed under the recording it came from, and its stimulus source remains part
of the saved dataset.

## Recording metadata

Cell ID, experiment, label, seal resistance, leak current, capacitance, and
series resistance are editable in Dataset tools and persist in `.plotx`.
