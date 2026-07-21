# PlotX typed-table scale benchmark

Run the release benchmark on the designated reference workstation:

```powershell
cargo bench -p plotx-datafusion --bench table_scale
$env:PLOTX_BENCH_ROWS='10000000'
$env:PLOTX_BENCH_MEMORY_MIB='512'
cargo bench -p plotx-datafusion --bench table_scale
```

The default run uses one million rows and 256 MiB. The second command is the
ten-million-row gate with the verified bounded 512 MiB tier. Record the printed
OS, available thread count, row count, memory tier, and elapsed time for filter,
aggregate, stable sort, and one-to-one join. On Windows the executable reports
its process peak resident set directly; other platforms may supply the same
measurement through their workstation harness. A passing run must spill rather
than OOM.

For a quick functional smoke run without release linking:

```powershell
$env:PLOTX_BENCH_ROWS='100000'
cargo run -p plotx-datafusion --example table_scale
```

## Reference workstation and recorded release baseline

Reference workstation: Intel Core Ultra 9 185H, 22 available logical threads,
approximately 64 GiB RAM, Windows NT 10.0 build 26200. The benchmark uses
directory-backed content blocks. The independently launched release executable
was 76,083,712 bytes on 2026-07-21. The complete stripped PlotX desktop release
binary built from the same tree was 110,838,272 bytes; this is the dependency
and binary-size baseline for future table-engine upgrades.

One million rows, 256 MiB execution budget:

| Operation | Elapsed | Process peak RSS |
| --- | ---: | ---: |
| Filter | 1.265 s | 48.1 MiB |
| Aggregate (1,000 groups) | 0.712 s | 48.1 MiB |
| Stable sort | 1.338 s | 95.5 MiB |
| One-to-one inner join | 2.226 s | 186.0 MiB |

Ten million rows, 512 MiB execution budget:

| Operation | Elapsed | Process peak RSS |
| --- | ---: | ---: |
| Filter | 10.809 s | 281.1 MiB |
| Aggregate (1,000 groups) | 6.438 s | 281.1 MiB |
| Stable sort | 12.318 s | 326.1 MiB |
| One-to-one inner join | 21.629 s | 333.2 MiB |

The two ten-million-row join inputs exceed the execution budget, so the fixed
two-partition SortMergeJoin uses external sorting. At the 256 MiB tier the
first three operations complete, while the join reports a controlled
resource-exhausted error rather than risking OOM; callers may retry it at the
bounded 512 MiB tier.
