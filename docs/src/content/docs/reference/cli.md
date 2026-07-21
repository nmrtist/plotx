---
title: Command line
description: Run imports, processing, exports, and saved workflows without opening the app.
---

`plotx-cli` runs PlotX operations from a terminal or a script, with no window
and no display server — useful for processing a night's worth of experiments
on a server, or wiring PlotX into a larger pipeline. It performs the same
operations as the in-app [Automation](/guides/automation/) window.

:::note[Availability]
The command-line tool is not included in the released packages yet. If you
need it, build it from a checkout — see the
[repository README](https://github.com/nmrtist/plotx#build-from-source) — or
use the in-app Automation window, which runs the same workflows.
:::

## Inspect and process single files

```sh
plotx-cli inspect <input> [--json]
plotx-cli process <input> --scheme <recipe.plotxproc> --output <path> [--format svg|pdf|png|tiff|jpeg]
```

`inspect` detects, loads, and describes one supported dataset; `--json` emits a
stable machine-readable report for scripting. For ABF2 recordings it also
reports the ABF version, channel names and units, sample rate, sweep count, and
protocol name.

`process` is the convenience path for a single import, one
[processing recipe](/guides/templates/), and one figure export. When
`--format` is omitted, the format is inferred from the output file's
extension.

## Run a workflow

```sh
plotx-cli batch --workflow <workflow.json> --manifest <run-manifest.json>
```

`batch` runs a workflow file — the same JSON the in-app Automation window
uses, so the easiest way to get one is to build and validate it there first,
then hand the file to the command line for unattended runs.

Every run writes a run-manifest record of what happened: the workflow and its
hash, the PlotX version, the targets each step ran on, and every step's
parameters, results, warnings, and errors. The same JSON is written to the
`--manifest` file and to standard output, so a script can both archive it and
react to it.

Safety properties worth knowing:

- Relative paths in the workflow are resolved against the workflow file.
- Unknown parameters, cycles, and references to missing steps are rejected
  before any step runs.
- An existing output file is left untouched unless that step sets
  `overwrite` to `true`.
- `failure_policy` decides what happens when a step fails: `strict` (the
  default) stops the run, while `continue_compatible` skips the failed step
  and carries on.

## Exit codes

For scripting, success is `0`; invalid usage or workflows use `2`, unreadable
workflow or input files use `3`, processing failures use `4`, figure
construction failures use `5`, output-write failures use `6`, and completed
runs that contained failures use `7`. Any other nonzero code indicates an
internal error worth reporting.

## Workflow file anatomy

A workflow (`plotx.workflow.v1`) is a JSON graph of steps with no loops. You
rarely write one from scratch — the app produces them — but the format is
plain and editable. A minimal import → process → export workflow:

```json
{
  "schema": "plotx.workflow.v1",
  "inputs": {
    "files": { "kind": "external_files", "paths": ["data/sample.dx"] }
  },
  "nodes": [
    {
      "id": "import",
      "tool_id": "data.import",
      "parameters": {},
      "targets": { "kind": "explicit", "ids": [] },
      "bindings": [
        { "parameter": "paths", "source": { "kind": "workflow_input", "name": "files" } }
      ]
    },
    {
      "id": "process",
      "tool_id": "processing.apply_scheme",
      "parameters": { "path": "routine.plotxproc", "compatible_only": true },
      "targets": { "kind": "node_output", "node": "import", "port": "resources" },
      "dependencies": ["import"]
    },
    {
      "id": "export",
      "tool_id": "figure.export",
      "parameters": { "directory": "results", "format": "svg", "overwrite": false },
      "targets": { "kind": "node_output", "node": "import", "port": "resources" },
      "dependencies": ["process"]
    }
  ],
  "failure_policy": "strict"
}
```

Each node names a `tool_id` (the operation to run), the `targets` it acts on,
and the `dependencies` that must finish first. Targets can be an explicit list,
a query, the files declared under `inputs`, or the outputs of an earlier node.
A `data.transform` node reshapes a data table with the same operations as the
sheet's column and **Combine** menus; set `plan` to the transform to apply,
`name` to the output table's name, and target the tables to transform.
