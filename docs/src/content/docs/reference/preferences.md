---
title: Preferences
description: Every setting in the Preferences window, by category.
---

Open Preferences with `Ctrl` + `,` (`Cmd` + `,` on macOS) or from the menus.
Changes apply immediately and are saved automatically; **Reset to Defaults**
restores everything except your recent-files list.

## General

- **Object snapping** — snap plots and shapes to page and object guides, and
  snap whole pages and table sheets to nearby frame edges and the standard gap
  between frames. You can also toggle this from the toolbar. Hold `Alt` to
  bypass snapping for one drag.
- **Equal scale for homonuclear 2D imports** — when both axes are frequency
  axes of the same nucleus, start an imported spectrum with equal F1/F2 scale
  if the wider range is no more than twice the narrower range. The imported
  plot keeps this initial setting even if you later change the preference.
  Change an individual plot with **F1/F2 equal scale (1:1)** under **Axes** in
  the Object inspector.
- **Keep source canvas when tiling its last object** — keep a page that a
  drag-to-tile move empties, instead of deleting it along with the drop. Off by
  default; hold `Alt` while releasing to reverse the choice for a single drop.
- **Project backup copies** — keep a chosen number of complete previous saves
  as hidden files beside each project. Each copy can be as large as the
  project; choose Off to disable.
- **Automatic updates** and **Update channel** — see
  [Updates](/reference/updates/). This section also shows the installed
  version, a **Check now** button, and **Restart now** once an update is
  ready.

## Appearance

- **Chrome theme** — light, dark, or follow the system appearance. This
  styles the application window; the look of your figures is set per canvas
  with canvas themes.
- **Canvas accent** — the color of selection outlines and handles, the layout
  grid, margin guides, and drag-to-tile previews. Pick a color, or use **Follow
  theme** to take it from the chrome theme. Snap guides keep a contrasting
  color of their own so they stay distinct, and figure content and exported
  colors are never affected.
- **UI scale** — the size of all interface text and controls, per display.
  Automatic picks a physically legible size from the display's reported pixel
  density; the manual choices and the `Ctrl` + `+` / `Ctrl` + `-` shortcuts
  override it for the current display only.
- **Graphics processor** — which GPU class PlotX requests at startup; takes
  effect after a restart. Change it only if you see rendering problems on a
  multi-GPU machine.

## Processing

- **Default ILT regularization (λ)** — the λ offered when you build an ILT DOSY
  map for a dataset with no earlier ILT result to take it from. Accepts
  `0.000001` to `1000`; the default is `0.01`. A λ typed into the **Experiment**
  card applies to that dataset and leaves this default unchanged. See
  [Pseudo-2D analysis](/guides/pseudo-2d/).

## Export

- **Embed view snapshots** — save each plot's on-screen view into the
  `.plotx` file.
- **Raster resolution** — the default pixel density (72–1200 dpi) for bitmap
  exports. A DPI typed into one export dialog applies to that export alone and
  leaves this default unchanged; searching `export DPI` in the
  [command palette](/reference/command-palette/) opens this page.

## Recent

The files, folders, and projects you opened recently — the same list as
**File → Open Recent** and the welcome screen — with a **Clear recent files**
button.
