---
title: The board and page layout
description: Arranging figures on the infinite board and sizing pages for journals.
---

## The board

Plots live on an infinite board organized into pages. Dragging a frame snaps
it to the page grid, margins, and the edges of neighboring frames; snapping
can be toggled off from the toolbar. The arrange menu in the toolbar offers
alignment (with two or more frames selected), horizontal / vertical
distribution (three or more), z-ordering, and a *Tidy up frames* command.

## Canvas size

The active page shows a size chip above its top-left corner — the current
dimensions plus the matched preset (for example *89 × 60 mm · Nature ·
Single column*). Click it, use **Canvas Size & Settings…** on the Figure
Ribbon tab, or run any "Canvas Size" entry in the command palette to change
the size.

The preset list is searchable and grouped: journal figure widths (Nature,
Science, Cell Press, ACS, Elsevier, PNAS, and IEEE, taken from the
publishers' artwork guidelines), paper sizes with a portrait/landscape
toggle, presentation slides, recently used entries, and your own saved
custom sizes. Journal presets fix the page *width* — the height stays
content-driven up to the journal's maximum figure depth, and a warning
appears when the page grows past that depth.

By default a size change never moves or resizes your content; if objects
end up outside the page, the chip row shows an overflow warning that scales
the content down to fit in one undoable click (font sizes keep their
physical pt values). Turn on **Scale content when applying sizes** to have
presets scale objects together with the page instead.

Two helpers automate the rest:

- **Auto height** keeps the width fixed and lets the page height follow the
  content's depth, clamped to the journal's maximum.
- When the layout grid asks for two or more panel columns on a
  single-column page, a dismissible hint offers the same journal's full
  width in one click — it never resizes the page on its own.

The export dialog pre-selects the matching journal preset, so a page
authored at a column width exports at that width by default.

## Spacing between panels

Panels of a multi-panel figure read best when their data areas are evenly
spaced — but each plot reserves a different amount of room for its tick
labels and axis titles, so equal frame gaps rarely look equal.
**Canvas Size & Settings…** sets both the spacing you want and how it is
measured.

**Minimum spacing** is the gap itself, in the canvas unit; the **Tight**
(2 mm), **Normal** (5 mm), and **Spacious** (10 mm) presets fill in common
values.

**Spacing basis** decides what that gap is measured between:

- **Visual** (the default) measures between the data areas of neighboring
  plots, counting the tick labels and axis titles that sit between them, so a
  panel with a long y title is given the room it needs. The value is a
  minimum: the gap you see can end up wider, never narrower, and frames never
  overlap.
- **Frame** measures between the plot frames and ignores axis text. Frames
  then sit exactly the requested distance apart, and the visible space between
  data areas varies from pair to pair.

The basis applies wherever PlotX places plots for you — **Apply grid**, and
dragging a plot onto a page that already holds one.

Dragging a plot onto another page moves it there and re-tiles the destination.
During the drag the plot travels with the pointer, keeping the point you grabbed
under the cursor, and the destination page draws where every plot will sit once
you release. On a page that already holds two or more plots, the whole page
re-tiles into an even grid and the arriving plot takes the cell you are pointing
at.

If the move leaves the source page empty, PlotX deletes that page as part of the
drop, so the move and the deletion undo together. Hold `Alt` as you release to
keep the empty page instead; the status bar shows which way `Alt` will flip the
current drop. To keep empty source pages by default, turn on **Keep source canvas
when tiling its last object** in Preferences → General — `Alt` then removes them
for that one drop.

With the Select tool active, each non-zero page margin is drawn as a dashed
line across the page, showing the content area you are laying out into; a
margin of zero draws no line. Turning on the layout grid adds the cell
outlines, and snapping guides appear in a contrasting color while you drag.

## Simplify inner axes

In a grid of panels that share the same axes, repeating the tick numbers and
axis titles on every panel wastes space. **Simplify inner axes** keeps the
x-axis text only on the bottom plot of each column and the y-axis text only on
the leftmost plot of each row. Axis lines and tick marks stay on every panel.

There are two ways in:

- Tick **Simplify inner axes** beside **Apply grid** in Canvas settings to
  arrange and simplify as one undoable step. The frames are then measured
  against the simplified axes, so the panels grow into the space the hidden
  text used to take.
- Run **Simplify Inner Axes** — on the Arrange Ribbon tab, in the canvas
  right-click Arrange menu, or from the command palette — to simplify plots
  that are already in place. It needs at least two plots aligned in a grid;
  otherwise the status bar says what to fix.

To bring text back on one panel, select it and use **Axes** in the Object
inspector: the **X text** and **Y text** rows toggle **Tick labels** and
**Title** individually, and **Automatic** returns that axis to showing both.

## Stacked and multi-dataset plots

A single plot frame can display several 1D datasets — superimposed, or
stacked with adjustable vertical spacing and 3D shear. 2D datasets combine
as a color overlay.

## Plot styling and typography

PlotX styles plots for print automatically: clean bottom-and-left axes with
outward ticks, tick precision that follows the displayed range, tick density
that thins as a panel narrows, and NMR isotope numbers set as superscripts.
New dataset pages start at the 89 × 60 mm single-column size, so a plot
spanning the page already shows text at its printed journal size; assemble
multi-panel figures on a wider page later, keeping each panel at its natural
size.

What you control directly:

- Select one plot and use **Axes** in the Object inspector to override its X
  and Y titles or numeric ranges, or to hide either axis's tick labels and
  title. Leave a title blank, or keep a range on
  **Auto**, to use the value derived from the data. A manual range becomes that
  axis's full range: zooming and panning stay inside it, and a double-click on
  the plot returns to it. Charts without visible axes offer no axis settings,
  and categorical axes have no range controls.
- **Figure Typography…** on the Figure Ribbon tab sets the axis text sizes
  (tick labels, axis titles, and the figure title) for every plot at once, in
  absolute points — a document-level style, so resizing a panel never changes
  its type size.
- **Canvas themes** carry matching sizes — the Presentation Dark theme, for
  example, enlarges the axis text for slides.

When the figure is ready, see [Exporting](/guides/exporting/).
