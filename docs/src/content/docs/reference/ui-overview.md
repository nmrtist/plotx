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
  your plots and ordinary data sheets. "Page" is one framed area of the board
  that exports as one figure. Drag a page or table-sheet header to place it;
  near another frame, it snaps to that frame's edges and the standard gap.
  Hold <kbd>Alt</kbd> while dragging to bypass snapping.
- **Secondary Side Bar** — the right panel, holding the Object inspector and
  contextual analysis tools for the selection. It scrolls as one column, so a
  long inspector never pushes the tools below it out of reach. Processing is
  not here; it lives on the canvas.
- **Task dock** — the card at the upper right of the canvas that holds the
  multi-step tasks: Processing, Regions, Curve Fit, and Statistics. When two or
  more are open it grows tabs — **Process**, **Regions**, **Fit**, **Stats** —
  and shows one page at a time. Its Processing page lists one pipeline for a 1D
  or pseudo-2D dataset and two, **F2 (direct)** then **F1 (indirect)**, for a
  true 2D spectrum. See [Processing](/guides/processing/).
- **Ribbon** — the command strip organized into task tabs (**Data**,
  **Process**, **Analyze**, **Figure**, **Arrange**, **View**). On macOS its
  task row also holds the native window controls and project name. It is a
  shortcut surface: everything on it is also in the menus or command palette.
- **Status bar** — the bottom strip, showing hints, progress, and selection
  details.

## Recurring elements

- **Figure panel (Panel)** — a labelled section of a figure that can contain
  plots, images, text, and shapes. Panels cannot be placed inside other Panels.
  A **group** moves and arranges objects together but does not add a panel label.

- **Task page** — one task's page in the task dock. Drag its lower edge to
  change its height. Switching tabs keeps a page's settings; closing it with
  its ✕ is what discards them, and selecting a tab makes that page's dataset
  active.
- **Object inspector** — the properties panel for the selected board object:
  chart type, styling, geometry, and the display settings of what it draws, such
  as [contour levels](/guides/contour-levels/). Everyday settings are shown
  directly; the rest are folded into **Advanced**. A row of small buttons at the
  top — **Layout**, **Data**, **Type**, and whichever style sections apply —
  scrolls straight to that section. Its heading names what you have selected: an
  object and its dataset, or, for several objects at once, how many objects and
  how many datasets they draw from. Its **Contour** and **Line** sections appear
  only when the selection draws one; **Figure typography** belongs to the
  document and is always shown.
- **Setting row** — one setting as it appears in the Object inspector, Canvas
  settings, a processing step, or Preferences. Every row behaves the same
  way wherever it is shown. A dot marks a value that differs from its default —
  hover it to see that default — and the reset button beside it goes back.
  <kbd>Ctrl</kbd>+<kbd>K</kbd> searches these settings by name: activating a
  result opens the panel that holds the row, expands what it is folded into,
  scrolls to it, and highlights it. A row that cannot be edited in the current
  state stays visible but greyed; hover it for the switch to change first.
  When your selection covers several objects, one edit applies to all of them.
  If they do not already agree, the row reads **mixed** and shows a dash in
  place of a value rather than presenting one object's as the answer; hover it
  to see how many disagree and what setting a value now would do.
- **Settings group** — a named set of related settings with one home. The Ribbon
  carries a button per group that opens that home rather than repeating the
  controls: **Contour settings**, **Line settings** and **Figure typography
  settings** in **Figure → Style**, **Apodization settings** in
  **Process → Processing**. The canvas right-click menu lists the same groups
  that currently apply, as *Contour settings…* and so on, and
  <kbd>Ctrl</kbd>+<kbd>K</kbd> finds the individual settings inside them.
  Application preferences belong to no object, so the canvas menu does not list
  them: reach them with **Preferences…** in **View → Display**, from the
  command palette, or with <kbd>Ctrl</kbd>+<kbd>,</kbd>.
- **Data sheet** — the spreadsheet view of a data table, opened by
  double-clicking the table. A synchronized region table remains in the Data
  browser and opens read-only; its extracted-curves plot is the only board
  frame created for it. Use **Save editable snapshot** when you need values
  that can diverge from the source regions.
- **Command palette** — the searchable list of commands, settings, and data on
  <kbd>Ctrl</kbd>+<kbd>K</kbd>; see
  [Command palette](/reference/command-palette/).
- **Size chip** — the label above a page's top-left corner showing its
  dimensions and matched journal preset.
- **Step row** — one step of the active dataset's processing pipeline. Click it
  to edit its parameters; move it with **Move earlier** / **Move later** in its
  ⋯ menu or with <kbd>Alt</kbd>+<kbd>↑</kbd>/<kbd>↓</kbd>. Every step in the
  list is a step that runs, FFT included; an imported spectrum has no FFT row
  because its recipe has no FFT. See [Processing](/guides/processing/).

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
