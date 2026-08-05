---
title: Organizing data
description: Browse datasets and their derived results, and save your work as a project.
---

## The data browser

Switch the Primary Side Bar to **Data** to see a searchable tree of everything
in the project. Imported datasets are roots. Slices, projections,
spectrum-arithmetic results, region tables, peak-fit tables, and multiplet
tables appear under the dataset they came from, in **Derived data**.
**Analysis** expands to individual peaks, integrals, regions, peak fits,
multiplets, and fitted table columns.

A link icon marks a result with more than one source — for example a table
built from two spectra. It is the same dataset shown under each source, not a
copy: selection, highlighting, renaming, and opening the data sheet stay
synchronized. Hover the icon to see every source, or use **Reveal sources**
from its context menu.

Search is case-insensitive and keeps the complete ancestor path visible;
clearing it restores your previous expanded and collapsed branches.

## Selecting and opening

Click a dataset or reference to select it. `Shift`-click selects the continuous
range from the previous lead item; `Ctrl`-click (`Cmd` on macOS) adds or removes
one item, and combining both modifiers adds a range without clearing the current
selection. The same extended-selection model applies to the Canvas and Layers
lists. A multi-selection is how you stack several spectra in one plot or apply a
processing template to many datasets at once.

When one of these lists is active, use `↑` / `↓` or `Home` / `End` to move the
selection, hold `Shift` to extend it, and press `Space` to add or remove the lead
item. `Ctrl` + `A` selects all datasets; `Ctrl` + `Shift` + `A` clears them.
Double-click a dataset to open its data sheet. Click an analysis result to
focus its dataset; double-click it to jump to the plot and the corresponding
analysis tool.

Projects saved with much older versions of PlotX may show some derived results
as top-level datasets; they remain fully usable.

## Board views

In **Canvas** mode, the lower part of the side bar holds named **Board
views** — saved framings of the board you can return to with one click. They
are hidden while browsing data.

## Saving projects

`Ctrl` + `S` opens the project save options. A `.plotx` project file stores the
whole session — imported data, processing, analysis results, and layout — so
it reopens exactly where you left off and can be shared as a single file. See
[File formats](/reference/file-formats/) for what the file contains and how
versions are handled.

Saving runs in the background. If you continue editing while it runs, the
completed file contains the revision captured when saving began and PlotX
keeps the project marked as unsaved for the newer edits.

PlotX also writes an internal crash-recovery checkpoint after edits settle,
with a maximum one-minute recovery interval during continuous work. A
checkpoint is written only when the document has changed since the previous
one; it is cleared after a successful up-to-date save or a clean exit.

**Preferences → General → Project backup copies** keeps a chosen number of
complete previous saves as hidden files beside the project, so an accidental
overwrite is recoverable.

**File → New Project**, **Close Project**, opening another project, and
quitting all use the same **Save / Discard / Cancel** check when the current
project has unsaved changes.
