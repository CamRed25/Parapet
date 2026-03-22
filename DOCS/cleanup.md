# Code Archive Log

Records all code moved to `doa/` from the active build.
Per RULE_OF_LAW §4.1 — never delete code, move it.

---

## Active Removals

<!-- Template for entries:
### [Item Name]
- **Date:** YYYY-MM-DD
- **Source:** original/path/
- **Destination:** doa/modules/ (or doa/crates/, doa/misc/)
- **Reason:** [why it was removed from the active build]
-->

*No code archived yet.*\

### `parse_xprop_cardinal` and `parse_xprop_utf8_list`
- **Date:** 2026-03-22
- **Source:** `crates/parapet_bar/src/widgets/workspaces.rs`
- **Destination:** `doa/workspaces_xprop_helpers_2026-03-22.rs`
- **Reason:** `read_workspaces()` was rewritten to use `gdk::property_get()` on the root window, eliminating the `xprop -root` subprocess. These helpers parsed `xprop` text output and have no further reuse value. Plan: WorkspacesWidget Step 1 (2026-03-22).

### plan_2026-03-18_volume-optimization.md
- **Date:** 2026-03-21
- **Source:** `DOCS/plan.md`
- **Destination:** `doa/plan_2026-03-18_volume-optimization.md`
- **Reason:** Plan status was `Completed`; moved to doa/ per copilot-instructions.md plan lifecycle rules before the next plan is written.

---

## Resolved

<!-- Entries from Active Removals that have been permanently closed -->
