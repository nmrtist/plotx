---
title: Command palette
description: Search and run any command from the keyboard.
---

The command palette gives keyboard access to the application's commands —
everything from opening files to switching tools — without hunting through
menus.

## Opening and closing

Press `Ctrl` + `K` or `Ctrl` + `Shift` + `P` (`Cmd` on macOS) to open the
palette. The search box is focused automatically. Press the shortcut again,
press `Esc`, or click outside the palette to close it.

You can also choose **Search commands** at the right of the Ribbon task tabs, or
**Help → Command Palette…** on Windows and Linux.

## Searching and running

Type to filter the list. Matching is case-insensitive; separate words with
spaces and a command matches only when every word hits.

- `↑` / `↓` move the selection, skipping unavailable commands.
- `Enter` or a mouse click runs the selected command and closes the palette.

Each row shows the command name on the left and, in gray on the right, the
keyboard shortcut bound to that command, if it has one.

## Availability

Commands that don't apply in the current context are grayed out — for
example, export commands without an active canvas, or align and distribute
without enough selected objects.

## What's included

- Open, import, and save; new canvas from a template.
- Export (SVG, PDF, PNG, JPEG, TIFF) and copy image.
- Undo, redo, select all, and grouping.
- Side bar and view toggles, and Preferences.
- View, data, processing, analysis, fit, and peak commands shown in the task Ribbon.
- Arrange: grid, align, distribute, z-order, and *Tidy up frames*.
- Applying themes and stacking data.
- Switching to any tool.

Parameterized operations that need a target picked on the canvas — such as a
specific integral or phase adjustment — are not in the palette; switch to the
corresponding tool instead.
