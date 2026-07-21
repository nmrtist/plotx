---
title: Annotations
description: Text labels, panel labels, shapes, arrows, and grouping on the board.
---

Figures rarely ship without a label or an arrow. PlotX's annotation objects
live on the board next to your plots, move and export with them, and are
fully undoable.

## Text and panel labels

- **Text** (`T`) — a free text label or caption. Set the font size, color,
  alignment, and bold weight in the Object inspector.
- **Panel label** — the same object with defaults tuned for figure panel
  letters: bold, 8 pt, ready for journal-sized layouts. Available from the
  Ribbon and the command palette.

While editing a label, `Enter` commits, `Shift` + `Enter` inserts a newline,
and `Esc` cancels.

## Shapes

- **Rectangle** (`R`) and **Ellipse** (`O`) — outline a feature; each has a
  stroke color, stroke width, and an optional fill.
- **Line** (`L`) and **Arrow** — a straight connector drawn between the two
  corners you drag; the arrow adds a head at the far end.

Drag on the canvas to create a shape, then move or resize it like any frame.

## Style defaults

After styling an object, you can set its look as the default for new objects
of that kind ("set as default for new …"), so a series of labels comes out
consistent without restyling each one. These defaults last for the session;
axis and title text sizes are a document-level setting instead — see
[Figure Typography](/guides/layout-and-export/#plot-styling-and-typography).

## Selecting, grouping, and locking

- `Ctrl` + `A` selects all objects on the page; `Esc` steps the selection
  back out one level.
- `Ctrl` + `G` groups the selected objects so they select and move together;
  `Ctrl` + `Shift` + `G` ungroups.
- Objects can be locked against accidental edits, or hidden, from their
  context menu.
- `Delete` or `Backspace` removes the selected annotation objects.

Alignment, distribution, and z-ordering for annotations work exactly as for
plot frames — see [the board](/guides/layout-and-export/).
