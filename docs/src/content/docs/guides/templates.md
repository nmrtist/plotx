---
title: Recipes and templates
description: Save a processing pipeline once and reapply it to other datasets.
---

When a series of experiments needs identical processing, save the pipeline
once and reapply it — as a **recipe file** you can share, or as a **named
template** kept with your PlotX settings.

## Recipe files

A pipeline (including the group-delay setting) can be saved from the
Processing panel as a portable `.plotxproc` recipe file and applied to other
datasets, on this machine or a colleague's. Recipes are also the processing
building block for [automation workflows](/guides/automation/) and the
[command line](/reference/cli/).

## Named templates

Recipes you use often can be kept as **named templates** stored with the
application settings, so they are always at hand without a file dialog.

- **Save as template…** in the Processing panel saves the current
  dataset's pipeline under a name you choose. If the name is already
  taken, the dialog warns and the button becomes *Overwrite*.
- **Apply template…** lists the saved templates. Templates that don't
  match the current dataset (for example a two-axis template on a 1D
  spectrum) are marked incompatible with the reason shown, and cannot be
  applied. Applying a template is a single undoable step.
- **Apply to…** applies a template to many datasets at once: it opens a
  review dialog listing your selected datasets (or every dataset when
  nothing is multi-selected) with their compatibility, and applies the
  template to the compatible ones as **one** undoable step.
- **Delete** removes a template; click once to arm and again to confirm.

Both *Save processing template…* and *Apply processing template…* are
also available from the command palette
(<kbd>Ctrl</kbd>+<kbd>K</kbd>).
