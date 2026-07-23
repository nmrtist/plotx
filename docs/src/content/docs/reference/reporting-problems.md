---
title: Reporting problems
description: Find the diagnostic files to attach when reporting a PlotX crash or runtime problem.
---

## After a crash

When PlotX catches an internal error, it saves a plain-text crash report and
shows its full path. On the next launch, the recovery dialog or status bar
shows that path again. Attach the report to a
[GitHub issue](https://github.com/nmrtist/plotx/issues); it contains the PlotX
version, platform, panic location, backtrace, and the tail of the session log.

Crash reports are stored in the `crashes` subdirectory of PlotX's application
data directory. PlotX keeps the 10 most recent reports:

- Windows: `%LOCALAPPDATA%\plotx\data\crashes`
- macOS: `~/Library/Application Support/plotx/crashes`
- Linux: `$XDG_DATA_HOME/plotx/crashes`, or `~/.local/share/plotx/crashes`

## Logs for other problems

Each launch creates a log in the adjacent `logs` subdirectory. PlotX keeps the
five most recent session logs. For a problem that does not crash the
application, attach the log from the affected session and describe what you
were doing. Include a sample project or input file only when it does not
contain sensitive data.

PlotX does not upload reports or logs automatically.
