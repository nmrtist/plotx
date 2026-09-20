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

## Inspect and process data

```sh
plotx-cli inspect <input> [--json] [--sampling-declaration <file.json>]
plotx-cli craft <input> --output <result.json> [--region <start-ppm:end-ppm>]... [--expected-ratio <value>]...
plotx-cli process <input> --scheme <recipe.plotxproc> --output <path> [--format svg|pdf|png|tiff|jpeg] [--sampling-declaration <file.json>]
```

`inspect` detects, loads, and describes one supported dataset; `--json` emits a
stable machine-readable report for scripting. For ABF2 recordings it also
reports the ABF version, channel names and units, sample rate, sweep count, and
protocol name.
For XPS it reports measurement, region and point counts, the region names, and
how many regions have a binding-energy axis or remain kinetic-only.

For NMR, `inspect` describes the source data without processing it. Bruker
experiment directories prefer raw data; select a processed file explicitly to
inspect that spectrum. If a directory is ambiguous, specify a file or processing
directory. For NUS data, the reported shape includes unsampled grid points; it
is not the number of acquired observations. A combination of time, frequency,
or parameter axes is reported as `domain: "mixed"`.

Check reported warnings before processing. Missing calibration or digital-filter
delay remains unknown; see [NMR format limitations](/reference/file-formats/#nmr-data-and-projects).
Processing failures identify the failing step and return a nonzero exit code.

`process` is the convenience path for a single import, one
[processing recipe](/guides/templates/), and one figure export. When
`--format` is omitted, the format is inferred from the output file's
extension.

`craft` runs CRAFT on a one-dimensional complex FID. A raw acquisition
directory is treated as one input, even when it contains processed-data
subdirectories. A batch directory analyzes only its immediate child directories
that PlotX recognizes as raw acquisitions; a directory with none is rejected.
Repeat `--region <start-ppm:end-ppm>` to fit selected ppm intervals. Omit it to
use the full acquired bandwidth. The output is a `plotx.craft.batch.v1` JSON
report containing each input's fitted components, per-region coherent amplitudes,
an amplitude ratio when exactly two regions are supplied, diagnostics, and
quality checks. Wide selections are still reported under the regions you gave.

CRAFT requires known spectral width, observe frequency, chemical-shift reference,
and digital-filter delay. Missing information produces a failed entry in the report.
The ppm reference frequency is retained separately from the observe frequency;
the report's `chemical_shift_reference.reference_frequency_mhz` defines Hz-to-ppm
conversion. FFT cross-check magnitudes depend on the modeling interval,
exponential window, and zero filling. Use them to assess the fit; use the reported
coherent amplitudes for region amplitude ratios.

When exactly two regions are supplied, repeat `--expected-ratio` once per input
to compare the measured ratio with a reference value. The report records the
relative error and passes the check when it is within 5%. `all_succeeded` tells
you whether every calculation completed. `all_quality_checks_passed` is stricter:
it is false when an input or fit has a warning, produces no components for a
selected region, or reaches a diagnostic limit. Treat a false quality result as
an indication that the data needs scientific review, even when the command
completed.

## Sampling declarations

`inspect` and `process` accept `--sampling-declaration sampling.json` for a
supported 2D Bruker NUS or JEOL acquisition with only part of the indirect grid sampled. The JSON file is limited
to 8 MiB. Supply every field explicitly; for example:

```json
{
  "assertion_id": "my-sampling-table-1",
  "source": "user-provided sampling table from experiment notes",
  "grid_shape": [4],
  "coordinates": [[4], [2]],
  "index_base": "one",
  "component_counts": [2]
}
```

Set the fields from the original acquisition records:

| Field | Value to supply |
| --- | --- |
| `assertion_id` | An identifier for this sampling declaration. |
| `source` | Where the table came from, such as an acquisition log. |
| `grid_shape` | The full indirect grid size, including unsampled points, as a one-element array. |
| `coordinates` | One indirect index per observation, each in its own array, in acquisition order. |
| `index_base` | `"zero"` for indices starting at 0, or `"one"` for indices starting at 1. |
| `component_counts` | The number of component records (lanes) per observation, as a one-element array. |

The example describes a four-point grid with two lanes per observation, sampled
at one-based indices 4 then 2. Each coordinate row represents all lanes for that
observation. Preserve acquisition order and repeated observations. Repeats can
be imported, but currently prevent NUS reconstruction.

PlotX checks the declaration against the acquisition. Missing grid or calibration
information, incorrect lane or observation counts, and conflicts with existing
sampling lists cause an error. This option does not support Varian data,
processed spectra, or acquisitions outside the supported 2D layouts.

For example, save the declaration as `sampling.json`, then inspect the acquisition:

```sh
plotx-cli inspect experiment/ser --sampling-declaration sampling.json --json
```

The workflow tool `data.import` accepts the same JSON object in its optional
`sampling_declaration` parameter. The declaration must be valid for every path
in that import node; use separate nodes for different tables. Saving the project
retains the declaration without changing the vendor files.

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
