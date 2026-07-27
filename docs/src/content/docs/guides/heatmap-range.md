---
title: Heatmap colour range
description: Control which values a scalar heatmap's colours span, without touching the data underneath.
---

A heatmap paints a 2D scalar field by mapping values to colours. Which values
the colours span is a display choice: tighten it and weak features stand out,
widen it and the full dynamic range fits without saturating.

Select the plot on the page. The **Heatmap** section appears in the Object
inspector at the top of the Secondary Side Bar, and only when something in the
selection draws a heatmap. Its header counts the series you are about to edit,
so a change you meant for one plot never lands on three unnoticed.

## Colour range

- **Colour range** is the distance between the lowest and highest coloured
  value. Reduce it for contrast, raise it to keep more of the range visible.
- **Range centre**, under **Advanced**, is the value halfway between those two
  limits. Move it to shift the colours up or down the scale without changing
  how wide the span is.

Until you set either one, the colours span the field's own smallest and largest
finite values. Setting a row stores an explicit range on that series; it never
normalizes, clips, or otherwise alters the data the plot was built from.

A changed row is marked with a dot. Hover the dot to see the value PlotX would
choose, and use the reset button beside it to go back. Resetting either row
returns the whole range to the field's finite minimum and maximum as they are
now, rather than restoring an older number.
**Reset heatmap** rebuilds the whole heatmap encoding from its defaults, and
touches only the series drawn as heatmaps: contours in the same plot are left
alone and reported as skipped in the status bar.

A field with no finite values has nothing to derive a scale from, and the rows
say so instead of showing a number.

## From the plot

Hover the plot body and hold `Alt` while you scroll to change **Colour range**
without leaving the canvas. Each wheel notch narrows or widens the span by
about 20% around its current centre, so the gesture behaves the same on a field
whose values run to 100 and one whose values run to a billion. Scroll up to
tighten the range and bring out weak features, down to widen it.

A hint in the top-left corner of the hovered plot names what the gesture will
change and how many series it will change before you commit to it. A pinch
always zooms the axes instead, so you can navigate the plot without disturbing
its colours.

If the same plot draws contours over the heatmap, both layers have a display
setting `Alt` + scroll could plausibly mean. PlotX does nothing rather than
picking one by drawing order; use the **Heatmap** or **Contour** section to
change the layer you meant.

## Finding a setting

- <kbd>Ctrl</kbd>+<kbd>K</kbd> (<kbd>Cmd</kbd> on macOS) searches settings as
  well as commands and data. Typing `colour scale`, `contrast` or `heatmap`
  finds the row, opens the panel it lives in, expands its section and
  highlights it. See [Command palette](/reference/command-palette/).
- **Heatmap** in the **Style** group of the Ribbon's **Figure** tab jumps to
  the same place.
- Right-click the plot and choose **Heatmap settings…**.
- `Alt` + scroll over the plot body changes **Colour range** directly.
