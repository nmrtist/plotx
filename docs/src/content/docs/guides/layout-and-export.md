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

## Stacked and multi-dataset plots

A single plot frame can display several 1D datasets — superimposed, or
stacked with adjustable vertical spacing and 3D shear. 2D datasets combine
as a color overlay.

## Plot styling and typography

PlotX styles plots for print automatically: clean bottom-and-left axes with
outward ticks, tick precision that follows the data range, tick density that
automatically thins as a panel narrows, and NMR isotope numbers set as
superscripts. New dataset pages start at the 89 × 60 mm single-column size, so
a plot spanning the page already shows text at its printed journal size;
assemble multi-panel figures on a wider page later, keeping each panel at its
natural size.

What you control directly:

- **Figure Typography…** on the Figure Ribbon tab sets the axis text sizes
  (tick labels, axis titles, and the figure title) for every plot at once, in
  absolute points — a document-level style, so resizing a panel never changes
  its type size.
- **Canvas themes** carry matching sizes — the Presentation Dark theme, for
  example, enlarges the axis text for slides.

When the figure is ready, see [Exporting](/guides/exporting/).
