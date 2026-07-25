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

## Availability

Commands that don't apply in the current context are grayed out — for
example, export commands without an active canvas, or align and distribute
without enough selected objects.

Settings are grayed out the same way when nothing in the current selection can
receive them — no plot selected, or a selected series that draws something other
than a contour. They stay in the list so you can still find them by name; hover
one to see why it is unavailable.

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
- *Contour settings*, *Raise lowest level* and *Lower lowest level*.

Settings:

- The contour rows of the Object inspector — lowest level, anchor, levels, level
  ratio, negative contours, colours, and line width. See
  [Contour levels](/guides/contour-levels/).

Data:

- Every dataset, page, and object on a page. Activating one opens it and
  selects it.

Parameterized operations that need a target picked on the canvas — such as a
specific integral or phase adjustment — are not in the palette; switch to the
corresponding tool instead. One-off inputs to an operation, such as the export
resolution of a single export, are part of that operation's dialog rather than
searchable settings.
