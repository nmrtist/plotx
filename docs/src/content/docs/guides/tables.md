---
title: Data tables
description: Working with spreadsheet-style data tables.
---

Tables are first-class datasets in PlotX. They hold imported tabular data,
values extracted by the region tools, and fit results.

## Editing

Double-click a table on the board to open it as an editable sheet. The sheet
is a spreadsheet-style grid: numbered rows, resizable columns, and a header
that shows each column's name with its unit and type underneath. Empty cells
render as a dimmed dash; `NaN` and infinities render dimmed. The grid paints
only its visible row window, so very large tables stay responsive.

Click a cell to select it and move with the arrow keys. Double-click, press
**Enter** or **F2**, or simply start typing to edit; **Enter** commits and
moves down, **Tab** commits and moves right, and **Esc** cancels. Invalid
input keeps the editor open with a red outline and an explanation on hover.
Click a row number to select the whole row; **Ctrl+C** copies the selected
cell or row. The status bar shows the table size and the current selection.

Every edit is undoable; undo and redo step back and forth through the table's
edit history rather than replacing the whole table each time. Text, Boolean,
date, and empty cells edit exactly the way numbers do.

## Transforming and refreshing tables

Right-click a column header to reshape the table around that column. The menu
offers **Rename**, **Sort ascending** and **Sort descending**, **Keep only this
column**, **Filter out empty rows**, **Duplicate as computed column**, and
**Count rows per value**. Floating-point columns add **Mark non-finite as
missing** and, when the column has a convertible unit, a **Convert unit**
submenu. When the table holds several columns of one type you can **Unpivot
matching columns** into name/value rows, and a text column can **Pivot using
these names** to turn its values into column headings. The **Combine** menu in
the sheet toolbar unions or joins the table with another table in the project.
Every transform produces a new table and leaves the original untouched.

Transforms run in the background: the sheet shows the elapsed time and a
**Cancel** button, and a cancelled or failed transform never replaces the
table. A table built from another table gains a **Refresh** action — it re-runs
the steps that produced the table and reapplies your later cell edits on top.
If the source has changed so that edited rows can no longer be matched, or a
column's type or unit no longer fits, Refresh stops and reports the conflict
instead of guessing which row is which.

These transforms can also run unattended as part of a batch job — see
[Automation](/guides/automation/) and [the command line](/reference/cli/).

## Chart types

Select a plot made from a table and pick a chart in the **Chart type** gallery
of the Object inspector. **Chart** on the Ribbon's **Figure** tab jumps there
directly: it selects the table's plot and opens the inspector.

Picking a chart is picking the question it answers: to follow values along an
axis, use **Line**; to compare quantities across categories, **Bar** or
**Grouped bars**; to show how one column's values are distributed,
**Histogram**; to compare distributions across several columns at a glance,
**Box** or **Violin**; to show parts of a whole, **Pie**; and to see an
entire matrix of values at once, **Heatmap** or **Surface 3D**.

- **Line** — one point series per column against the x axis, with fitted
  curves overlaid when present.
- **Bar** — one chosen column as filled bars on the numeric x axis.
- **Grouped bars** — every column side by side, one group per row; check
  **Stacked** to pile the columns instead (positive values stack upward,
  negative downward).
- **Histogram** — the value distribution of one chosen column. Bins follow the
  Freedman–Diaconis rule; uncheck **Auto bins** to set a fixed count.
- **Box** — one box per column: median, quartile box, 1.5 × IQR whiskers, and
  individual outlier points.
- **Violin** — one kernel-density silhouette per column with an inner quartile
  bar (columns need at least two distinct values; degenerate columns fall back
  to raw points).
- **Heatmap** — the whole table as a colormapped matrix with rows reading
  top-to-bottom; pick the colormap in the inspector.
- **Pie** — one chosen column as wedges, one per row, labeled with
  percentages. Negative and missing rows are excluded.
- **Surface 3D** — the table as a height field in a fixed orthographic view;
  adjust the azimuth/elevation angles and the colormap in the inspector.

Grouped bars, Box, Violin, Heatmap, and Surface 3D use all columns at once;
Bar, Histogram, and Pie read the single column selected next to the gallery.
Every chart renders identically on screen and in SVG/PDF/EMF export.

## Plotting from tables

Error bars come from a column holding the uncertainty of another column. Tables
produced by PlotX's analysis tools already carry these, and an imported table
carries whichever its file describes — a symmetric ±σ, separate lower and upper
errors, or a confidence interval with its level. To add error bars to your own
data, put a column named `<y>_sigma` immediately after a y column in a CSV or
TSV, holding each point's uncertainty (zero or positive); on import PlotX links
it to that y column as a symmetric standard deviation. Before plotting, PlotX
checks that a value column and its uncertainty share compatible units. Line,
Bar, and unstacked Grouped-bar charts draw these as vertical error bars and
widen the y-axis range so the bars stay fully visible.

To fit x-y values with a mathematical model, choose **Fit Curves** in the **Curve Fit** group of
the **Analyze** tab. A Curve Fit task card opens at the upper-right of the
canvas: choose a model, fit every curve or one selected curve, then choose
**Run Fit**. See [Curve fitting](/guides/curve-fitting/) for the built-in
models, custom models, shared (global) parameters, weighting and robust-loss
options, and fit diagnostics. The fitted curves and their results stay with the
table.
Use **Export Data…** and choose **Curve-fit parameters** to export parameters,
diagnostics, covariance, and point predictions/residuals.
A completed fit records exactly which rows it used, and which it excluded and
why, so later edits to the table cannot quietly change which points the result
was based on. The curve-fit export lists those included and excluded rows.

**Export Data…** can also export the complete table as CSV, TSV, XLSX, or
clipboard TSV. Empty cells stay empty, while `NaN`, `+Inf`, and `-Inf` are
written as those literal tokens because they are real values, not blanks. Each
of these formats carries the table's column types, units, and error-bar
relationships alongside the data — in the `.plotx-schema.json` companion for
CSV and TSV, on a hidden worksheet for XLSX, or in the clipboard payload for
Copy TSV — so a `<y>_sigma` column name is only needed when you hand-build a
file that has none.

**Peak Fit** is a separate tool: use it when a plotted trace contains
overlapping spectral peaks that need to be deconvolved — see
[Choosing an analysis tool](/guides/choosing-a-tool/).

To compare groups, test correlations, check normality, or run ANOVA on a
table's columns, choose **Statistics** in the **Analyze** tab. See
[Statistics](/guides/statistics/).
