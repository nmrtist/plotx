---
title: Interface glossary
description: The names this manual uses for the parts of the PlotX window.
---

This manual refers to the parts of the PlotX window by consistent names. If a
page says "open the Object inspector" and you are not sure where to look,
this glossary is the map. The [quick tour](/getting-started/quick-tour/)
introduces the same regions in walkthrough form.

## Window regions

- **Primary Side Bar** — the left panel. Its **Canvas** mode lists plots,
  pages, and saved board views; its **Data** mode shows every dataset and the
  results derived from it.
- **Canvas** / **the board** — the central area: an infinite board holding
  your plots, arranged on grid-snapped pages. "Page" is one framed area of
  the board that exports as one figure.
- **Secondary Side Bar** — the right panel, holding contextual tools for the
  selected data. The **Processing panel**, **Analysis panel**, and **Dataset
  tools** named throughout this manual are tool groups shown here.
- **Ribbon** — the command strip under the title bar, organized into task
  tabs (**Data**, **Process**, **Analyze**, **Figure**, **Arrange**,
  **View**). It is a shortcut surface: everything on it is also in the menus
  or the command palette.
- **Context line** — the line below the Ribbon naming the active canvas,
  object, dataset, task, and tool.
- **Status bar** — the bottom strip, showing hints, progress, and selection
  details.

## Recurring elements

- **Task card** — a floating card at the upper-right of the canvas that walks
  through a multi-step task (Regions, Curve Fit, Statistics). Drag its
  lower-right handle to resize it.
- **Object inspector** — the properties panel for the selected board object:
  chart type, styling, and geometry.
- **Data sheet** — the spreadsheet view of a data table, opened by
  double-clicking the table.
- **Command palette** — the searchable command list on
  <kbd>Ctrl</kbd>+<kbd>K</kbd>; see
  [Command palette](/reference/command-palette/).
- **Size chip** — the label above a page's top-left corner showing its
  dimensions and matched journal preset.

## Data terms

- **Dataset** — anything importable or derived that holds data: a spectrum, a
  recording, or a table.
- **Derived data** — results that came from another dataset (slices,
  projections, region tables, fit tables); the data browser lists them under
  their source.
- **Pseudo-2D** — a stack of 1D spectra acquired while one parameter varies
  (gradient strength for DOSY, delay for T1/T2), as opposed to a **true 2D**
  spectrum such as COSY or HSQC.
- **Pipeline** — the ordered list of processing steps applied to a dataset's
  raw data.
- **Recipe / template** — a saved pipeline, as a shareable file
  (`.plotxproc`) or under a name in your settings; see
  [Recipes and templates](/guides/templates/).
