---
title: Quick tour
description: A five-minute walkthrough of the PlotX interface.
---

This page walks through the main parts of the PlotX window and the typical
flow from raw data to a finished figure.

## The window at a glance

- **Primary Side Bar** (left) — datasets and project structure.
- **Canvas** (center) — an infinite board holding your plots, arranged on
  grid-snapped pages.
- **Secondary Side Bar** (right) — contextual tools for the selected data:
  processing, peaks, regions, and more.

Hide either side bar from **View** or the **View** Ribbon to give the canvas
more room.

## Menus and task Ribbon

On Windows and Linux, the title bar holds the app logo, the **File**, **Edit**,
**View**, **Insert**, and **Help** menus, and the window controls in one row.
Drag its empty area to move the window, or double-click to maximize. On macOS
these commands use the system menu bar, including the standard PlotX
application and Window menus.

**File** keeps an **Open Recent** submenu with the files, folders, and projects
you opened or saved most recently; the same entries are listed on the welcome
screen while no data is loaded. **Help** contains **User Manual**, which opens
this documentation in your browser.

The Ribbon below it is a focused shortcut surface, not a second complete menu.
Choose **Data**, **Process**, **Analyze**, **Figure**, **Arrange**, or **View** to see
grouped frequent commands for that stage. Use **Collapse ribbon** to collapse it to the task tabs.
At narrower window widths, whole low-priority groups move into **More**; at the
minimum width the command area folds automatically instead of shrinking text or
buttons. The context line below the Ribbon names the active canvas, object,
dataset, task, and tool.

**Search commands** opens the command palette. Menu items, Ribbon buttons,
shortcuts, and palette rows share the same enabled and selected states.

The **Process** and **Analyze** tabs also switch the Secondary Side Bar to their
tool group, but they never reopen a side bar you have hidden.

## Data browser

The Primary Side Bar has two modes: **Canvas** lists your plots and pages, and
**Data** shows every dataset with the results derived from it. Click a dataset
to focus it; double-click to open its data sheet. See
[Organizing data](/guides/organizing-data/) for the full tour of the data tree,
multi-selection, and saved board views.

## A typical session

1. **Import** a dataset by dragging a file onto the window, using **File**, or
   choosing an import command on the **Data** Ribbon.
2. **Process** it in the processing panel — the pipeline applies steps in
   order and previews the result live.
3. **Analyze** with the peak and region tools; extracted values appear in
   linked tables.
4. **Arrange** plots on the board and **export** the figure.

[Your first figure](/getting-started/first-figure/) walks through these four
steps with a real dataset.

## Navigation

Pan and zoom are always available, in any tool:

- **Scroll wheel** — zoom the plot under the cursor.
- **Middle-drag** or **Space + drag** — pan.
- **Double-click** — auto-range the axes.
