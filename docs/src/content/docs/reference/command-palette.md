---
title: Command palette
description: Search commands, settings, and data from the keyboard.
---

The command palette gives keyboard access to commands — everything from opening
files to switching tools — and to settings and data, without hunting through
menus and panels.

## Opening and closing

Press `Ctrl` + `K` or `Ctrl` + `Shift` + `P` (`Cmd` on macOS) to open the
palette. The search box is focused automatically. Press the shortcut again,
press `Esc`, or click outside the palette to close it.

You can also choose **Search commands** at the right of the Ribbon task tabs, or
**Help → Command Palette…** on Windows and Linux.

## Searching and running

Type to filter the list. Matching is case-insensitive; separate words with
spaces and a row matches only when every word hits.

- `↑` / `↓` move the selection, skipping unavailable rows.
- `Enter` or a mouse click activates the selected row and closes the palette.

Each row shows the name on the left and, in gray on the right, a hint: the
keyboard shortcut for a command, the panel for a setting, the kind or the page
for data.

Settings match on more than the label you see: their alternative names, and the
id they carry in [workflows](/guides/automation/), whole or word by word.
`contour threshold`, `sigma` and `series.contour.count` all reach the contour
rows.

Activating a setting does not change anything. It opens the panel the setting
lives in, expands its section, scrolls to the row and highlights it briefly, so
you can see the current value before editing it.

A setting that lives in Preferences instead opens the Preferences window at the
page that holds it; no individual row is highlighted there.

A setting that belongs to one processing step also opens Processing on the
canvas and expands the first step that actually carries it — a step whose
window has no **GB**, for instance, is passed over rather than opened onto a
row that is not there.

## Availability

Commands that don't apply in the current context are grayed out — for
example, export commands without an active canvas, or align and distribute
without enough selected objects.

Settings are grayed out the same way when nothing in the current context can
receive them — no plot selected, a selected series that draws something other
than a contour, or a dataset with no apodization step. They stay in the list so
you can still find them by name; hover one to see why it is unavailable.

A setting that applies to the selection but cannot be changed there is grayed
out too. A locked plot reads *Unlock this plot to change its settings; it can
still be read while locked.*

## What's included

Three kinds of row share one search: commands, settings, and data.

Commands:

- Open, import, and save; new canvas from a template.
- Export (SVG, PDF, PNG, JPEG, TIFF) and copy image.
- Undo, redo, select all, and grouping.
- Side bar and view toggles, and Preferences.
- View, data, processing, analysis, fit, and peak commands shown in the task Ribbon.
- Arrange: grid, align, distribute, z-order, and *Tidy up frames*.
- Applying themes and stacking data.
- Switching to any tool.
- *Contour settings*, *Line settings*, *Figure typography settings*,
  *Apodization settings*, *Raise lowest level* and *Lower lowest level*.

Settings:

- The **Contour** rows of the Object inspector — lowest level, anchor, levels,
  level ratio, negative contours, colours, and line width. See
  [Contour levels](/guides/contour-levels/).
- **Stroke width**, the width of a line series, in the Object inspector's
  **Line** section. `line width`, `stroke width`, `trace thickness` and `line
  thickness` all reach it.
- **Tick-label size** in the Object inspector's **Figure typography** section.
  `font size`, `tick size`, `points` and `figure typography` reach it. See
  [Layout and export](/guides/layout-and-export/).
- **Window**, **LB** and **GB** on an apodization step in Processing.
  `apodization`, `window function`, `exponential`, `gaussian`, `LB`, `line
  broadening`, `GB` and `gaussian broadening` reach them. See
  [Processing](/guides/processing/).
- **Raster resolution**, the pixel density bitmap exports start from, on the
  **Export** page of Preferences. `export DPI`, `bitmap DPI`, `bitmap
  resolution` and `raster resolution` reach it. It stays available whatever is
  selected, changing it cannot be undone, and it applies to every project. See
  [Preferences](/reference/preferences/).

Data:

- Every dataset, page, and object on a page. Activating one opens it and
  selects it.

Parameterized operations that need a target picked on the canvas — such as a
specific integral or phase adjustment — are not in the palette; switch to the
corresponding tool instead. One-off inputs to an operation stay in that
operation's dialog: the DPI you type into a single export is not searchable,
while **Raster resolution**, the default it starts from, is.
