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

Drag over empty board space to marquee-select page and sheet frames. Hold
`Shift` to add the enclosed frames or `Ctrl` (`Cmd` on macOS) to toggle them.
Dragging the header of any selected frame moves the whole selection; the move
undoes as one step. In the Canvas list, `Shift`-click selects a continuous range
and `Ctrl`-click adds or removes one canvas. `Ctrl` + `A` selects every frame or
canvas according to the area you last used.

## Scientific summary and notes

PlotX shows an automatic summary of the plotted data below each page on the
board. On a multi-panel page, shared information appears once and the remaining
details follow the panel labels.

Open **Canvas settings** to add a page note, or select a Panel and edit its
**Note**. Notes appear after the summary and are not included in exports. Use
**Show summary below page** to show or hide this whole board-only block.

## Add external images

To place each image on its own fitted page, choose **Add Images…** from the File
menu, Ribbon, or command palette. You can select multiple PNG, JPEG, TIFF, WebP,
or BMP files. You can also drop files onto the board; each image gets a new page
even if you drop it over an existing one.

To add an image inside an existing Panel, select or enter an unlocked Panel,
then drop the file inside the green outline. To paste an image or a copied list
of image files, use **Paste Image** or press `Ctrl`/`Cmd` + `V`. On Windows, you
can copy files from File Explorer or copy image pixels from an application such
as Paint. Pasting in a text field continues to paste text normally.

When PlotX creates a page for an image, it uses the image's physical size when
DPI information is available and assumes 96 DPI otherwise. The page is at most
89 mm wide, and its height follows the image's aspect ratio. The image fills the
page without distortion. Importing several images is one undoable action; a
failed or cancelled import leaves the project unchanged.

For formats that need special handling, use these commands:

- **Add Animated Image First Frame…** imports the first frame of an animated
  PNG or WebP.
- **Add Images Without Metadata…** removes metadata by converting the image to
  PNG before adding it.
- **Add All TIFF Pages…** adds every readable page of a multi-page TIFF. The
  standard **Add Images…** command adds only its first page.

PlotX asks for confirmation before importing a very large image. Select an
image and use **Image** in the Object inspector to crop or rotate it, adjust its
opacity, or choose how it is interpolated. These changes do not alter the
embedded source image.

The board uses a bounded proxy for responsive editing, but SVG, PDF, EMF,
PNG, JPEG, TIFF, and **Copy Figure** sample the embedded source. Crop,
quarter-turn rotation, opacity, fit, interpolation, Panel clipping, and layer
order therefore match the authored page without depending on the source file's
original path.

If an embedded image is missing or damaged, the project still opens and keeps
the image item in place as a labelled placeholder. Use **Replace Image…** in
the Object inspector to repair it. Normal save and publication export stop
until it is replaced; the Export dialog can explicitly allow labelled
placeholders when a review copy is still useful.

## Work with figure panels

Use the Panel commands in the Arrange menu or command palette to build a
multi-panel figure. You can create an empty Panel, turn selected objects into a
Panel, or duplicate, merge, split, dissolve, and delete Panels. Expand a Panel
in Layers to see the objects it contains.

Click anywhere inside a Panel to select it, then drag to move it or use a corner
handle to resize it together with its contents. `Shift`-click selects multiple
Panels. To work with an object inside a Panel, double-click the Panel or press
`Enter`. Use the breadcrumb above the page or press `Esc` to return to the page.
You can also `Ctrl`/`Cmd`-click an object to select it without entering first.

Inside a Panel, objects snap to the Panel edge and to one another. To move an
object into another unlocked Panel, drag it over the Panel and release when the
target is highlighted. You can make the same move in Layers by dragging the
object's row onto the Panel's row. A Stack or Grid Panel rearranges its contents
after the move. Hidden objects remain selectable in Layers; locked objects can
be selected but not edited.

Use the Panel inspector to control visibility, locking, clipping, labels, and
layout. **Vertical Stack**, **Horizontal Stack**, and **Grid** arrange the
contents using the selected gap, padding, and alignment. Automatic panel letters
appear only when a page has more than one visible Panel and follow reading order:
left to right, then top to bottom. A letter placed over an image automatically
switches between black and white for contrast. Manual and locked labels remain
visible on a page with one Panel.

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

To align line series along x, select the plot and choose **Align Traces…** in
the **Align** group on the **Analyze** tab. The command is available when the
plot contains at least two visible line series with the same x-axis unit. You
can open the same dialog with **Align traces…** under **Data** in the Object
inspector.

Choose a reference series, then align by **Trace start** (the first finite
plotted sample) or by a peak within an x window. For peak alignment,
**Positive** finds upward peaks, **Negative** finds downward peaks, and
**Magnitude** compares their prominence. Review the **Anchor**, **Delta**, and
**Result** columns before selecting **Apply**. Hidden series, incompatible
series, and series without a usable anchor are listed as **Skipped** and remain
unchanged.

Alignment preserves the plot's current zoom. To adjust one line manually, edit
its **X shift** under **Data**. Automatic and manual shifts are undoable plot
settings and do not affect source data or data exports.

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
  title. For a true 2D NMR spectrum, **F1/F2 equal scale (1:1)** uses the same
  data units per screen unit on both axes. PlotX sets its initial value when
  the spectrum is imported; you can change it afterward without changing the
  import preference. Leave a title blank, or keep a range on **Auto**, to use
  the value derived from the data. A manual range becomes that axis's full
  range: zooming and panning stay inside it, and a double-click on the plot
  returns to it. Charts without visible axes offer no axis settings, and
  categorical axes have no range controls.
- Use **Legend & scales** in the Object inspector for any plot. With
  **Visibility** on **Auto**, PlotX shows a categorical legend when the plot
  has two or more named entries and shows a continuous colour scale for a
  heatmap. **Show** also permits a one-entry legend; **Hide** removes the legend
  or colour scale from that plot. **Position** can keep it inside the plot or
  reserve space to its right or below it. **Layout** switches a categorical
  legend between a vertical list and a horizontal row; **Title** is available
  under the section's advanced controls.
- With **Select** active, click a legend or colour scale to open its
  **Legend & scales** settings. Drag a categorical legend to place it freely;
  the custom position survives resizing and is used by every export format.
  Double-click a legend or colour scale to restore its automatic placement.
- **Figure Typography…** on the Figure Ribbon tab sets the text sizes (tick
  labels, axis titles, the figure title, and legends) for every plot at once,
  in absolute points — a document-level style, so resizing a panel never
  changes its type size. These sizes accept 1 to 72 pt. Legends default to
  7 pt.
- **Figure typography** in the Object inspector holds the same tick-label size
  as **Tick-label size**, plus **Legend size** and **Legend text color**.
  Changing a value in either surface changes the one document-wide value.
  Because it belongs to the document rather than to a plot, the section is
  shown whatever is selected — including when nothing is.
- **Line** in the Object inspector sets the **Stroke width** of the selected
  plot's line series, in points. New line series are 0.5 pt; drag or type any
  value from 0.05 to 10 pt, or take one from **Presets** — *Fine* 0.50 pt,
  *Medium* 0.75 pt, *Bold* 1.25 pt. The section appears only when something in
  the selection is drawn as a line, and its header counts the series it is
  about to change. Select several plots and it edits them together; select
  nothing and it follows the page's active plot.

  When the selected series do not all carry the same width the control shows an
  em dash and the word *mixed* rather than presenting one of them as the
  setting; setting the row is what makes them agree. A selected series that is
  drawn as something else — a contour, say — is reported as skipped in the
  status bar, and the rest still take the value. A width that differs from the
  one PlotX would choose for this data is marked with a dot, and the reset
  button beside it re-derives that default.
- **Canvas themes** carry matching sizes — the Presentation Dark theme, for
  example, enlarges the axis text for slides.

When the figure is ready, see [Exporting](/guides/exporting/).
