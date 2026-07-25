---
title: Contour levels
description: Set the lowest contour level, the ladder above it, and the colours of a 2D contour plot.
---

A contour plot draws a ladder of levels: a lowest level, then each next level a
fixed ratio above the one before. PlotX edits that ladder for any 2D scalar
data — a phase-sensitive NMR plane, a magnitude plane, an AFM height map — from
one place.

Select the plot on the page. The **Contour** section appears in the Object
inspector at the top of the Secondary Side Bar, and only when something in the
selection draws a contour. Its header counts the series you are about to edit —
*3 series* — so a change you meant for one plot never lands on three unnoticed.
Select several plots and the section edits them together; select nothing and it
follows the page's active plot.

## Lowest level

**Lowest level** is the only setting shown until you open **Advanced**, because
it decides what you see: everything below it is not drawn at all. Raise it to
suppress noise, lower it to reveal weak cross-peaks.

What the number means depends on the anchor (below). Under the default anchor
for a phase-sensitive NMR plane it is a multiple of the noise floor, so `5`
means "start at five times the noise". Because a multiple on its own does not
tell you where the level actually falls, the row names its unit and resolves the
level beside it: `5` `× noise floor` `= 1.2e4`.

Noise and background are estimated away from the interface, so until the
estimate arrives the resolved level reads `measuring…`. Data with no measurable
spread — a perfectly flat or perfectly regular grid — reads `no spread
measured`: the multiple anchors nothing, and the levels fall back to a ladder
derived from the data's own peak so the plot is not left blank.

### The noise floor

An estimator measures thermal noise. Data with a large dynamic range also
carries the sampling artefacts of its own strongest feature — in a 2D spectrum,
the t₁ noise ridges and residual solvent ridges that run through the tallest
peaks — and those are far above the thermal floor. A level set below them draws
the artefacts rather than the peaks: hundreds of thousands of contour crossings
that hide the spectrum and can exceed what one plot is able to draw at all.

So the σ anchor never resolves below **0.01 % of the data's peak**. Whichever is
larger, the estimated σ or that floor, is what the multiple is measured against.
On ordinary data σ is much larger and nothing changes. On data whose peak is
more than 10,000 times its noise the floor takes over, and the readout names it
rather than continuing to name σ: the row resolves to
`= 1.652e5 (0.01% of peak)`, and the plot corner spells out the whole sentence,
`5 × 0.01% of peak = 1.652e5 — σ is below this floor`.

Hover the resolved level for the longer explanation. The floor is a floor, not a
ceiling on what you can ask for: choose the **Absolute level** anchor to set any
level you like, including one inside the noise.

If you set an **Absolute level** the data never reaches, that half draws nothing
and the status bar reports both numbers — for example, *The positive contour
threshold 20000 is above this field's positive peak 1800, so no positive
contours are drawn. Lower the threshold below 1800.* A slipped decimal point is
visible at a glance that way.

### From the plot

Press `+` and `-` to raise and lower the lowest level of the selected plot — or
of the page's active plot when nothing is selected — without leaving the canvas.
On keyboards where `+` needs Shift, `=` raises as well. Each press moves the
level by one rung of that plot's own **Level ratio**, so one press adds or
removes roughly one contour ring whatever the intensity scale: the same gesture
works on a spectrum whose peak is 100 and one whose peak is a billion.

Whenever the keys apply, the current lowest level is shown in the top-right
corner of the plot, resolved the same way as in the panel — `5 × σ = 1.2e4`. A
plot whose contour series do not all sit at the same level says so instead of
showing one of them. Stepping is an
ordinary edit: it can be undone, and a step past the highest value the current
anchor allows is refused, with the reason in the status bar.

## Anchor and ladder

Open **Advanced** for the rest of the ladder.

- **Anchor** — what the lowest level is measured against. The choices offered
  depend on what the data can supply:
  - *Multiple of the noise floor* — needs data whose noise level can be
    estimated, such as a phase-sensitive NMR plane. This is the default there.
    See [The noise floor](#the-noise-floor) for what the floor is.
  - *Background + multiple of spread* — needs data with a measurable background
    level and spread, such as an AFM height map, where the background is not
    centred on zero.
  - *Fraction of value range* — needs single-signed data, such as a magnitude
    plane. It is not offered for data that has both positive and negative
    values, where a fraction of the range is not a meaningful threshold.
  - *Absolute level* — a raw intensity, always available.

  The anchor also sets what **Lowest level** accepts: a multiple of the noise
  floor or of the spread runs up to 10,000, a fraction of the range is between 0
  and 1, and an absolute level is any intensity above 0.
- **Levels** — how many levels the ladder has, from 1 to 256.
- **Level ratio** — the factor between one level and the next. It must be
  greater than 1, and at most 10.

**Negative contours**, **Positive colour**, **Negative colour** and **Line
width** (0.05 to 10) are in the same section.

The ladder stops early when the next level would be above the data's peak, so a
plot can show fewer levels than **Levels** asks for. That is not an error: the
remaining levels would draw nothing.

A ladder can also stop early at the bottom. A level that falls inside the noise
crosses most of the grid, and there is a limit to how much line one plot can
draw. Past that limit PlotX drops the remaining levels whole — never cutting a
contour off part-way along its own path — and drops the same levels from both
signs, so what you see is a complete ladder with a higher floor. The status bar
says how many went and what to set instead, for example: *The lowest 14 contour
levels were not drawn: at 5.052e4 and below, this field crosses more of the grid
than one plot can render. Raise the lowest level to 6.820e4 or above to see
every level the panel lists.*

Changing the anchor, the lowest level, the count or the ratio applies to both
signs — the negative half mirrors the positive ladder and applies its own sign.

## While it is being drawn

Contour geometry, and the noise or background estimate a ladder is anchored to,
are computed away from the interface so a large plane never freezes the
application. Until they land the plot is empty, and the status bar says which
step is running — *Measuring this field's noise scale…*, then *Building contour
geometry…* — so a plot that is merely slow is never mistaken for one that has
failed. Large planes take a moment; the plot fills in by itself. The work is
shared: two plots drawn from the same data at the same levels wait on one
computation, not two.

## No single value

The anchor, the lowest level, the count and the ratio are shared: by the
positive and negative halves of a ladder, and by every series you have selected.
When they do not all agree, the control shows an em dash and the word *mixed*
instead of a number or a choice, so no one value is presented as the setting.
The resolved level beside the control disappears with it: there is no single
lowest level to resolve. Hover *mixed* to see which case it is — one series
whose two halves differ, or several series that differ from each other.

Setting the row applies your value to every selected series and to both halves
of each ladder, which is what makes them agree again.

## Negative contours

**Negative contours** is offered only for data that has both signs, such as a
phase-sensitive NMR plane. Single-signed data — a magnitude plane, a height map
— has no negative half to draw.

Positive and negative contours have separate colours, so the sign information of
a phase-sensitive spectrum survives in the figure.

## Changed values and reset

A setting that differs from what PlotX would choose for this data is marked with
a dot. Hover the dot to see the default; use the reset button next to it to go
back. Reset re-derives the default from the current data rather than restoring
an older value, so it stays correct after reprocessing. A row with no single
value carries the same marker.

Resetting the lowest level re-derives it under the anchor currently in force, not
under the one PlotX would have chosen: reset it while anchored to a fraction of
the range and you get a fraction, not a noise multiple.

**Reset contour** rebuilds the whole contour — anchor, ladder and colours — from
the defaults for this data. It touches only the series drawn as contours;
anything else in the same plot, such as a heatmap underneath, is left as it is
and reported as skipped in the status bar.

## Finding a setting

Four routes reach a contour setting, and they all end at the same control:

- <kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>Cmd</kbd> on macOS) searches settings as
  well as commands and data. Typing `contour threshold`, `sigma` or `levels`
  finds the matching row, opens the panel it lives in, expands its section and
  highlights it. See [Command palette](/reference/command-palette/).
- **Contour** in the **Style** group of the Ribbon's **Figure** tab jumps to the
  same place.
- Right-click the plot and choose **Contour settings…**.
- `+` and `-` change the lowest level directly on the plot.
