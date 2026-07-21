# PlotX

[![CI](https://github.com/nmrtist/plotx/actions/workflows/ci.yml/badge.svg)](https://github.com/nmrtist/plotx/actions/workflows/ci.yml)

PlotX is a native desktop application for scientific data analysis and figure
preparation.

[User manual](https://docs.plotx.nmrtist.space/) · [Releases](https://github.com/nmrtist/plotx/releases) · [Contributing](CONTRIBUTING.md)

## Highlights

- **Bring scientific data together.** Current import support includes Axon
  ABF2 patch-clamp recordings, JEOL Delta and Bruker TopSpin experiments,
  JCAMP-DX spectra, archives, and delimited tables.
- **Process and analyze interactively.** Build ordered processing pipelines,
  then pick peaks, integrate regions, and fit data. NMR workflows also include
  DOSY and relaxation analysis, plus sweep statistics and IV analysis for
  electrophysiology recordings.
- **Prepare figures for submission.** Compose spectra, tables, and annotations
  on a multipage board; use journal-size and resolution presets, check font
  sizes and line widths before export, then write vector or raster files. On
  Windows, figures also copy to the clipboard as both bitmap and vector data.
- **Make work reproducible.** Save complete `.plotx` sessions and reuse
  `.plotxproc` processing recipes across datasets.

The [user manual](https://docs.plotx.nmrtist.space/) has the supported-format
matrix, the processing-step reference, and workflow guides.

## Install

Prebuilt packages are published on the
[Releases](https://github.com/nmrtist/plotx/releases) page; the
[installation guide](https://docs.plotx.nmrtist.space/getting-started/installation/)
covers setup and updates. If there is no package for your platform yet, build
from source below.

## Build from source

Building requires the current stable Rust toolchain.

```sh
git clone https://github.com/nmrtist/plotx.git
cd plotx
cargo run --release -p plotx
```

That is the fast development build. It uses the built-in reference table
executor, which keeps whole tables in memory and cannot spill, so large-table
work needs the shipping engine instead:

```sh
cargo release-build          # the desktop app with the DataFusion backend
```

Releases are produced with `cargo release-build`. The About window always
names the active table engine, so you can tell which kind of build you are
running.

The development workflow and pre-submission checks are described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Project status

PlotX is pre-1.0 and under active development. The `.plotx` project schema may
change between releases until 1.0.

## License

PlotX is dual-licensed:

- **Open source:** [GNU GPL v3.0 or later](LICENSE). If you distribute a
  combined work covered by the GPL, you must comply with its source-code and
  licensing requirements.
- **Commercial:** a separate commercial license is available for embedding
  PlotX in closed-source or proprietary products. See
  [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md).

Copyright and asset licensing are described in [COPYRIGHT.md](COPYRIGHT.md),
including treatment of the PlotX name and logos. External contributions are
accepted under the agreement and process in [CONTRIBUTING.md](CONTRIBUTING.md).

Using the PlotX application as an end user is always free and never requires a
commercial license.
