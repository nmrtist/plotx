## Summary

<!-- Explain the user-visible motivation and the focused change. -->

## Validation

<!-- List the checks you ran, including cargo pr-check when applicable. -->

## UI display logic

<!-- Delete this section if the change does not touch commands, the Ribbon,
     or panels. Full rules: crates/app/AGENTS.md. -->

- [ ] New or changed actions go through the command catalog (`CommandId` +
  `describe`) and are searchable in the command palette.
- [ ] Hiding happens only at Ribbon-group level (dataset kind or width
  budget); transient states disable with a `disabled_reason` that says how
  to unblock.
- [ ] No layout changes from background events; panels are never auto-closed.
- [ ] New panels or empty regions show an empty state with a next step.
- [ ] Docs updated in `docs/` (English and zh-CN) for user-visible behavior.
