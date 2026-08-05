---
title: Keyboard shortcuts
description: Keyboard and mouse shortcuts.
---

`Ctrl` means `Cmd` on macOS. Shortcuts are ignored while a text field has
keyboard focus.

## Tools

Single keys, no modifiers:

Press the same key again to leave a tool. Analysis tools return to **Zoom**;
drawing tools such as **Text** and **Rectangle** return to **Select**. Pressing
`V` or `Z` again leaves that neutral tool active. The cursor family is the
exception: repeated `C` presses advance to the next applicable cursor, and
`Esc` leaves the cursor.

| Key | Tool |
| --- | --- |
| `V` | Select |
| `Z` | Zoom (rubber-band box zoom) |
| `I` | Integrate (1D band or true-2D rectangle, according to the dataset) |
| `P` | Peaks |
| `C` | Cycle cursors: **Inspect**, **Delta**, then **Symmetry review** when the selected spectrum supports it |
| `S` | Slice |
| `D` | Peak Fit |
| `T` | Text |
| `R` | Rectangle |
| `O` | Ellipse |
| `L` | Line |

**Inspect** reports the coordinates and sampled intensity under the pointer;
click to pin it. **Delta** measures chemical-shift, frequency, and intensity
differences between two clicked positions. On an eligible homonuclear true-2D
spectrum, the third `C` press selects **Symmetry review**. On other spectra
that support cursors, `C` alternates between **Inspect** and **Delta**.

## Navigation

Pan and zoom are ambient — available in every tool, acting on the plot under
the cursor, or on the board when the cursor is over empty space.

| Input | Action |
| --- | --- |
| Scroll wheel over a plot body | Zoom the x axis of a 1D plot; both axes of a 2D plot |
| Two-finger swipe on a macOS trackpad | Pan the plot under the pointer; pan the board over empty space |
| Pinch over a plot | Zoom both axes, whatever the plot draws |
| `Alt` + scroll wheel over a plot body | Change what the plot shows: y intensity on a 1D plot, the lowest contour level, or a heatmap's colour range |
| `Alt` + drag over a plot body | Rubber-band a box to zoom into, in any tool |
| Scroll wheel over an axis strip | Zoom that axis only |
| `Ctrl` + scroll wheel / pinch | Zoom the board instead of the plot |
| `Ctrl` + two-finger swipe on a macOS trackpad | Pan the board instead of the plot |
| Middle-drag or `Space` + drag | Pan the plot (the board when over empty space or holding `Ctrl`) |
| Drag on an axis strip | Select a range on that axis to zoom into |
| Double-click a plot | Reset both axes to full range |
| Double-click an axis strip | Reset that axis only |
| `F` | Zoom the board to fit the selected frames (everything when nothing is selected) |
| `Enter` | Zoom the board to the selected page or sheet |

Hovering a plot body or an axis strip outlines the area the wheel will act on
and names the action in its top-left corner, including which setting `Alt` +
scroll wheel would change and on how many series. Where one plot draws two
things with display settings of their own — contours over a heatmap — `Alt` +
scroll wheel does nothing rather than guessing which you meant; change the
layer you want from the Object inspector.

## Selection and editing

| Input | Action |
| --- | --- |
| `Ctrl` + `N` | Start a new project |
| `Ctrl` + `W` | Close the current project |
| `Ctrl` + `S` | Open project save options |
| `Ctrl` + `Z` | Undo |
| `Ctrl` + `Shift` + `Z` or `Ctrl` + `Y` | Redo |
| `Ctrl` + `A` | Select every item in the current context: page objects, board frames, canvases, or datasets |
| `Ctrl` + `Shift` + `A` | Clear the selection in the current context |
| `Shift` + click | Select a continuous range in the Canvas, Layers, or Data list |
| `Ctrl` + click | Add or remove one item without clearing the rest |
| `Ctrl` + `Shift` + click | Add a continuous range to the existing list selection |
| `↑` / `↓`, `Home` / `End` | Move selection through the active Canvas, Layers, or Data list; hold `Shift` to extend |
| `Space` | Add or remove the lead item in the active list |
| `Ctrl` + `G` | Group the selected objects |
| `Ctrl` + `Shift` + `G` | Ungroup |
| `Delete` or `Backspace` | Delete the selected annotation objects; in Peaks or Integrate, delete the selected peak or region; in Symmetry review, delete the selected cross-peak mark |
| `+` (or `=`) / `-` | Raise / lower the lowest contour level of the selected plot |
| `F2` | Rename the selected dataset or canvas |
| `Esc` | Cancel the active drag; further presses clear the Analysis Range and selections one at a time, then leave the active tool |
| `Ctrl` + `C` | Copy the single selected page frame (or the active canvas) to the clipboard as bitmap + vector |
| `Ctrl` + `Shift` + `V` | Paste a delimited table (comma, tab, or semicolon) from the clipboard as a new data table |
| `Ctrl` + `,` | Open Preferences |
| `Ctrl` + `K` or `Ctrl` + `Shift` + `P` | Open the [command palette](/reference/command-palette/) |

`+` and `-` act on the plot you have selected, or on the active plot when
nothing is selected, and only when it draws contours. Each press moves the
lowest level by one rung of that plot's own level ratio, so one press adds or
removes roughly one contour ring whatever the intensity scale. The current
lowest level is shown in the top-right corner of the plot while the keys apply.
See [Contour levels](/guides/contour-levels/). Holding `Ctrl` changes the UI
scale instead.

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
