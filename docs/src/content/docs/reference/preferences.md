---
title: Preferences
description: Every setting in the Preferences window, by category.
---

Open Preferences with `Ctrl` + `,` (`Cmd` + `,` on macOS) or from the menus.
Changes apply immediately and are saved automatically; **Reset to Defaults**
restores everything except your recent-files list.

## General

- **Object snapping** — snap plots and shapes to guides while dragging (also
  toggleable from the toolbar).
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

## Export

- **Embed view snapshots** — save each plot's on-screen view into the
  `.plotx` file.
- **Raster resolution** — the default pixel density (72–1200 dpi) for bitmap
  exports.

## Recent

The files, folders, and projects you opened recently — the same list as
**File → Open Recent** and the welcome screen — with a **Clear recent files**
button.
