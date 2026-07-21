# Contributing to PlotX

All contributions are welcome: code, documentation, translations, bug
reports, ideas. If you are unsure whether something fits, open an issue and
ask.

The one hard rule: never post confidential or proprietary data — in code,
issues, or pull requests. When data is needed to reproduce a problem, a
minimal anonymized sample works best.

## Development

PlotX builds with stable Rust. Some configurations pull in very heavy
dependencies, so the wrong command can turn a quick edit/compile loop into a
long wait — use the narrowest one that answers your question:

| Purpose | Command |
| --- | --- |
| Fast type-check loop | `cargo check -p plotx` |
| Day-to-day development: build and run the app | `cargo run -p plotx` |
| Test the crate you changed | `cargo test -p plotx-core` |
| Optimized build for performance work | `cargo build --release -p plotx` |
| Shipping configuration, adds the DataFusion table backend | `cargo release-build` |
| The same checks as CI, before a pull request | `cargo pr-check` |

Two things that are easy to trip over:

- Development builds keep the scientific processing crates optimized so they
  stay usable under a debugger, but the UI and renderer are not. Judge frame
  rate or rendering performance only with an optimized build.
- The app and CLI default to an in-memory reference table executor.
  Shipping builds enable the DataFusion backend via `cargo release-build`;
  the About window shows which engine is active.

`cargo pr-check` requires `protoc` and `cargo-deny`
(`cargo install --locked cargo-deny`); nothing else does. CI runs exactly the
same steps, split into parallel jobs (`cargo xtask pr-check quick|lint|test`).
`cargo licenses` (requires `cargo-about`) regenerates
`dist/THIRD-PARTY-LICENSES.html`, the report of bundled third-party licenses.

The user manual is an Astro/Starlight site in `docs/` (`npm run build` to
validate). UI screenshots come from the built-in harness: point `PLOTX_SHOT`
at an output directory and run the app; `crates/app/src/shot.rs` documents
the details.

## Contributor license agreement

PlotX is distributed under both an open-source license and commercial
licenses, so contributions are covered by a short
[CLA](CONTRIBUTOR-LICENSE-AGREEMENT.md): you keep your copyright and permit
your contribution to be distributed under both models. A bot walks you
through acceptance on your first pull request — there is nothing to do in
advance, and one acceptance covers later contributions.

Make sure the rights are yours to grant (for example, that an employer does
not own the work), and mention the source and license of any third-party
material in the pull request.
