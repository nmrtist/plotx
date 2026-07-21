# plotx-cli

`plotx-cli` is PlotX's non-interactive frontend. It uses the same core loaders,
tool registry, workflow executor, renderer, authority checks and run-manifest
types as the desktop application.

```text
plotx-cli inspect <input> [--json]
plotx-cli process <input> --scheme <file> --output <path> [--format svg|pdf|png|tiff|jpeg]
plotx-cli batch --workflow <workflow.json> --manifest <manifest.json>
```

`batch` executes a `plotx.workflow.v1` directed acyclic graph. Nodes name stable
tool IDs such as `data.import`, `processing.apply_scheme` and `figure.export`;
they can bind workflow inputs or preceding node outputs to parameters and target
selectors. Relative paths are resolved from the workflow file. Arbitrary loops,
scripts and code execution are not part of v1.

The resulting `plotx.run-manifest.v1` records the canonical workflow and hash,
caller, PlotX/tool versions, revisions, frozen targets and selection reasons,
per-node results, warnings, errors, verification and timing. It is written
atomically and emitted unchanged on stdout.

Exit codes are `0` for success/help, `2` for usage or workflow validation, `3`
for input/workflow reads, `4` for processing, `5` for figure creation, `6` for
output writes, `7` for an executed workflow with failures, and `70` for internal
result serialization.
