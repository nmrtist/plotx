# Repository instructions for AI agents

These instructions apply to the entire repository.

## Working principles

- Preserve user changes and keep edits focused on the requested task.
- Do not add adjacent features, abstractions, or cleanup unless they are needed
  for the requested change.
- Do not commit generated directories such as `target/`, `dist/`,
  `docs/dist/`, or `docs/node_modules/`.
- Keep public documentation and user-visible behavior in sync.
- Never add credentials or confidential or proprietary data.

## Product name

- The brand name is **PlotX** (capital P and X). Write it that way in every
  human-readable string: UI text, window titles, About, docs prose, README, and
  legal files. English sentences that begin with the name still use PlotX.
- Machine identifiers stay lowercase `plotx`: crate names (`plotx-core`), the
  binary and `-p plotx`, the repository and documentation URLs, the
  `.plotx`/`.plotxproc` extensions, env vars (`PLOTX_SHOT`), and the app bundle
  id. Never rewrite these to the brand form, and never brand-case a code block.
- The Rust type `PlotxApp` keeps its spelling; Rust treats the name as one word.
- Rule of thumb: if a human reads it as the product name, write PlotX; if a
  machine parses it, keep plotx.

## Code quality

- Prioritize correctness and clarity. Optimize only when requirements,
  profiling, or known numerical workloads justify it.
- Comments should explain non-obvious reasons, invariants, numerical choices,
  or safety constraints rather than restating the code.
- Never silently discard a fallible result. Propagate it, handle it explicitly,
  or deliberately log it when continuing is safe.
- Do not panic on external input or recoverable runtime failures. `unwrap()` and
  indexing are acceptable only where an invariant is local and evident, or in
  tests where failure is the intended outcome.
- Errors from background work must reach application state and produce useful
  user-visible feedback when the operation was initiated by the user.

## Rust workspace

- Stable resources and components use typed IDs. Collection indices are
  one-shot lookup positions only; they must not cross action, job, frame, or
  persistence boundaries.
- Respect crate boundaries: parsing in `plotx-io`, scientific algorithms in
  `plotx-analysis`, spectral transforms in `plotx-processing`, presentation
  models in `plotx-figure`, rendering in `plotx-render`, application state in
  `plotx-core`, DataFusion execution in `plotx-datafusion`, Substrait interop in
  `plotx-substrait`, and desktop UI in the `plotx` application crate.
- `plotx-data` stays engine-free: never add datafusion, datafusion-substrait or
  sqlparser to it. Backend adapters reach its internals through the
  `#[doc(hidden)] pub` seams; keep that set minimal instead of widening the API.
- The app and CLI default to the reference table executor. DataFusion is an
  opt-in `datafusion` feature, and releases are built with `cargo release-build`
  so the flag lives in one place. Keep both configurations compiling.
- Keep Rust source files below the repository's 800-line limit; prefer cohesive
  modules over large files. Extract tests to a sibling `*_tests.rs` referenced
  with `#[path]` when a file approaches it.
- Run `cargo pr-check` before completing code changes. It checks formatting,
  source sizes, dependency licenses and advisories, a default-configuration
  build of both frontends, Clippy with warnings denied, and the test suite in
  both backend configurations. It requires `cargo-deny`.

## Documentation

- The user manual lives in `docs/`, uses Astro/Starlight, and is published at
  <https://docs.plotx.nmrtist.space/>.
- English pages are canonical. Keep matching Simplified Chinese pages aligned
  when changing translated content.
- Validate documentation changes with `npm run build` from `docs/`.

## Rules hygiene

- Keep this file high-signal. Add a rule only when the behavior is non-obvious,
  repeatedly relevant, and specific enough to act on.
- Prefer documenting traps and invariants over architecture that can be
  discovered by reading the code.
- Put crate-specific rules near the relevant crate when they do not apply to
  the whole repository.
- Update an existing rule when possible; avoid adding rules for one-off
  observations.
