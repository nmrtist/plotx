---
title: Keyboard shortcuts
description: Keyboard and mouse shortcuts.
---

`Ctrl` means `Cmd` on macOS. Shortcuts are ignored while a text field has
keyboard focus.

## Tools

Single keys, no modifiers:

| Key | Tool |
| --- | --- |
| `V` | Select |
| `Z` | Zoom (rubber-band box zoom) |
| `I` | Integrate (1D band or true-2D rectangle, according to the dataset) |
| `P` | Peaks |
| `S` | Slice |
| `D` | Peak Fit |
| `T` | Text |
| `R` | Rectangle |
| `O` | Ellipse |
| `L` | Line |

## Navigation

Pan and zoom are ambient — available in every tool, acting on the plot under
the cursor, or on the board when the cursor is over empty space.

| Input | Action |
| --- | --- |
| Scroll wheel / pinch | Zoom the plot under the cursor (both axes) |
| `Shift` + scroll | Zoom the x axis only |
| `Alt` + scroll | Zoom the y axis only |
| Scroll over an axis strip | Zoom that axis only |
| `Ctrl` + scroll / pinch | Zoom the board instead of the plot |
| Middle-drag or `Space` + drag | Pan the plot (the board when over empty space or holding `Ctrl`) |
| Drag on an axis strip | Select a range on that axis to zoom into |
| Double-click a plot | Reset both axes to full range |
| Double-click an axis strip | Reset that axis only |
| `F` | Zoom the board to fit the selected frames (everything when nothing is selected) |
| `Enter` | Zoom the board to the selected page or sheet |

## Selection and editing

| Input | Action |
| --- | --- |
| `Ctrl` + `S` | Open project save options |
| `Ctrl` + `Z` | Undo |
| `Ctrl` + `Shift` + `Z` or `Ctrl` + `Y` | Redo |
| `Ctrl` + `A` | Select all objects on the page |
| `Ctrl` + `G` | Group the selected objects |
| `Ctrl` + `Shift` + `G` | Ungroup |
| `Delete` or `Backspace` | Delete the selected annotation objects; in the Peaks and Integrate tools, delete the selected peak or region |
| `F2` | Rename the selected dataset or canvas |
| `Esc` | Cancel the active drag; otherwise step the selection back one level until nothing is selected |
| `Ctrl` + `C` | Copy the selected frame (or the active canvas) to the clipboard as bitmap + vector |
| `Ctrl` + `Shift` + `V` | Paste a delimited table (comma, tab, or semicolon) from the clipboard as a new data table |
| `Ctrl` + `,` | Open Preferences |
| `Ctrl` + `K` or `Ctrl` + `Shift` + `P` | Open the [command palette](/reference/command-palette/) |

While editing a board note: `Enter` commits, `Shift` + `Enter` inserts a
newline, `Esc` cancels.

## UI scale

PlotX picks a legible UI scale for each display automatically (**UI scale**
under Preferences → Appearance); these shortcuts adjust it in 10% steps, and
the adjustment is remembered per display.

| Input | Action |
| --- | --- |
| `Ctrl` + `+` / `Ctrl` + `-` | Increase / decrease the UI scale on the current display |
| `Ctrl` + `0` | Reset the UI scale to automatic |

## Present mode

| Input | Action |
| --- | --- |
| `→` / `↓` / `Space` / `PageDown` | Next page |
| `←` / `↑` / `PageUp` | Previous page |
| `Home` / `End` | First / last page |
| `Esc` | Exit present mode |
