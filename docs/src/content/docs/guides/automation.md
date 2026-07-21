---
title: Automation
description: Apply one action to many datasets or figures, or run a saved workflow.
---

When the same operation has to happen to a whole series of experiments,
automation replaces the click-per-dataset routine with one reviewed batch.

Open **File → Automation…** (also on the command palette). The window has two
tabs:

## Current Project

Works on what is already open. Search for datasets or figures and check the
ones you want — or press **Current selection** to pull in your current
selection — then pick a tool and press **Preflight** to see which targets it
will affect and whether any are incompatible. **Confirm and execute** applies
it, and the whole batch collapses into a single **Undo automation** step.

## External Inputs

Runs a saved workflow that starts from files on disk — for example: import
every experiment in a folder, apply a processing recipe, and export each
figure. Press **Open workflow…** to load it, **Validate** to check it, then
**Confirm and run workflow**. Progress is reported step by step and a long run
can be cancelled. Every completed run is recorded in the project.

Workflow files are plain JSON and can also be run without the desktop app —
see [the command line](/reference/cli/), which executes the same workflows
headlessly. [File formats](/reference/file-formats/) describes the workflow
and run-record files themselves.
