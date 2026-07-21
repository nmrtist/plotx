---
title: Installation
description: How to install and first launch PlotX on Windows, macOS, and Linux.
---

PlotX is a native desktop application for Windows, macOS, and Linux.

## Download and install

Prebuilt packages are published on the
[GitHub releases page](https://github.com/nmrtist/plotx/releases).
Download the archive for your platform, unpack it anywhere you like, and run
the `plotx` executable. No installer or administrator rights are required.

## First launch

Because the packages are downloaded from the internet, your operating system
may warn before the first run:

- **Windows** — if SmartScreen reports an unrecognized app, choose
  **More info → Run anyway**.
- **macOS** — if Gatekeeper blocks the app, right-click it, choose **Open**,
  and confirm; macOS remembers the choice.

On first launch PlotX shows a welcome screen. Drag a data file onto the window
or use **File → Open File…** to get started — the
[quick tour](/getting-started/quick-tour/) walks through the interface.

## Where PlotX keeps its settings

Preferences, saved processing templates, and custom fit models live in a small
per-user configuration folder:

| Platform | Location |
| --- | --- |
| Windows | `%APPDATA%\plotx\config` |
| macOS | `~/Library/Application Support/plotx` |
| Linux | `~/.config/plotx` |

Your data and projects are never stored there — they stay wherever you save
them. Deleting the folder resets PlotX to its defaults.

## Updating and uninstalling

PlotX checks for new versions and updates itself in the background — see
[Updates](/reference/updates/). To uninstall, delete the application folder
and, if you want a clean slate, the configuration folder above.

## Building from source

Developers can build PlotX from a checkout; see the
[repository README](https://github.com/nmrtist/plotx#build-from-source) for
instructions. As a user of the released packages you never need to do this.
