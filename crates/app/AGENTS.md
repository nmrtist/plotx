# UI display-logic rules (app crate)

These rules govern the Ribbon, the sidebars, and every surface that shows or
executes commands. They exist so that new features keep one consistent
show/hide/disable behavior regardless of who adds them.

## Command catalog

- Every action exposed in the Ribbon, menus, or shortcuts must be registered as
  a `CommandId` and described by `commands::describe`, so the Ribbon, menus,
  command palette, and shortcuts share one enabled/placement decision. Do not
  add ad-hoc business-action buttons that bypass the catalog. Panel-local
  widget interactions (rename fields, tree-row toggles) are exempt.

## Hide vs disable

- Hiding is allowed only at Ribbon-group level, and only for two reasons: the
  active dataset kind makes the whole group inapplicable, or the width budget
  moves the group into the "More" overflow menu. Never hide an individual
  button within a group because of state.
- Anything transient (no selection, missing analysis range, a running job)
  disables the command instead, with a `disabled_reason` from the `requires()`
  gate chain. Reasons start with a verb and state how to unblock, e.g.
  "Draw an analysis range before running Peak Fit." — never a bare
  "Not available".

## Layout stability

- Layout changes (panels opening/closing/collapsing, Ribbon groups appearing/
  disappearing) may only be triggered by an explicit user action: clicking,
  selecting, resizing the window. Background events (job completion, data
  arrival, timers) may update the status bar, feedback banner, or rows inside
  a tree — never the layout.
- Panels never auto-hide. Code may auto-open a panel as the direct consequence
  of a user action (e.g. opening a tool group), never auto-close one.
- An empty panel shows a one-line empty state ending with the next step
  (see `primary_sidebar.rs` for the house style), never blank space.

## New-command decision tree

1. Register a `CommandId`; gate it in `describe()`; it must be searchable in
   the command palette.
2. Global command (no dataset dependency)? Fixed Ribbon placement, always
   visible; disable with a reason when unavailable.
3. Only meaningful for one dataset kind? Put it in that kind's group; the
   whole group shows/hides with the kind.
4. Transiently unavailable? Keep it visible and disable it with the first
   unmet requirement.
5. Low-frequency or advanced? Menus and palette only — still via the catalog.
