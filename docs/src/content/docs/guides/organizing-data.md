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

Click a dataset or reference to focus it. Hold `Shift`, `Ctrl`, or `Cmd` while
clicking to extend the selection — a multi-selection is how you stack several
spectra in one plot or apply a processing template to many datasets at once.
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

**Preferences → General → Project backup copies** keeps a chosen number of
complete previous saves as hidden files beside the project, so an accidental
overwrite is recoverable.
