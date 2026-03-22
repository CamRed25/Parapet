# Parapet — Audit Report

**Date:** 2026-03-21
**Auditor:** Auditor Agent
**Standards revision:** 2026-03-21 (RULE_OF_LAW, AGENT_GUIDE, SECURITY created/updated this session)
**Scope:** Full codebase audit — `crates/`, `standards/`, `.github/`, `.claude/`, `DOCS/`

---

## Critical Violations

No critical violations found.

*Critical threshold: GTK/display imports in `parapet_core`, `unsafe` block without `// SAFETY:`, silent `Result` drop with `let _ =` in production without documented invariant, data-loss risk.*

The two `unsafe` blocks found in `crates/parapet_bar/src/widgets/launcher.rs` both carry correct `// SAFETY:` comments — compliant.

---

## High Violations

> All items below were **fixed during this audit**. No outstanding high violations remain.

| # | Location | Violation | Status |
|---|----------|-----------|--------|
| H-1 | `standards/ARCHITECTURE.md §4.2` | `WidgetData` code snippet: `Cpu` missing `temp_celsius: Option<f32>`, `Battery.charge_pct` typed as `f32` instead of `Option<f32>`, `Disk` missing `all_disks: Vec<DiskEntry>`. Stale from early dev. | ✅ Fixed |
| H-2 | `standards/ARCHITECTURE.md §5` | Module table listed `config.rs` in `parapet_bar` — file does not exist. Hot-reload lives in `main.rs`. | ✅ Fixed |
| H-3 | `standards/ARCHITECTURE.md §6.1` | Startup sequence described a vague 8-step flow; actual `main.rs` has 11 numbered stages. | ✅ Fixed |
| H-4 | `standards/WIDGET_API.md §4` | `Cpu` variant had malformed formatting: `temp_celsius` doc comment was on the same line as the preceding struct field. | ✅ Fixed |
| H-5 | `standards/BUILD_GUIDE.md §2.1` | GTK feature flag `"v3_22"` — actual workspace `Cargo.toml` uses `"v3_24"`. Missing 5 workspace dependencies: `gio`, `fuzzy-matcher`, `libc`, `ctrlc`, `tempfile`. | ✅ Fixed |
| H-6 | `standards/BUILD_GUIDE.md §2.2` | `parapet_core` Cargo.toml template missing 4 dependencies (`ureq`, `zbus`, `schemars`, `serde_json`); `tempfile = "3"` wrong format (must be `tempfile.workspace = true`). | ✅ Fixed |
| H-7 | `standards/UI_GUIDE.md §3.1` | Code example used `gtk::WindowType::Popup`; actual `bar.rs` uses `gtk::WindowType::Toplevel`. `Toplevel + _NET_WM_WINDOW_TYPE_DOCK` is intentional for correct EWMH strut association. | ✅ Fixed |
| H-8 | `standards/BAR_DESIGN.md §2.1` | WindowType property table entry: `Popup` → `Toplevel`. | ✅ Fixed |

---

## Medium Violations

> All items below were **fixed during this audit** unless marked `[USER ACTION]`.

| # | Location | Violation | Status |
|---|----------|-----------|--------|
| M-1 | `standards/RULE_OF_LAW.md §2` | `RELEASE_GUIDE.md` exists in `standards/` but was absent from the hierarchy table. | ✅ Fixed (added as priority 11) |
| M-2 | `.github/copilot-instructions.md` | Module table covered only 5 of 11 `parapet_core` widgets (stale from early development). Missing: `disk`, `volume`, `brightness`, `weather`, `media`, and updated descriptions for `cpu` (now has temperature). | ✅ Fixed (expanded to 15 entries) |
| M-3 | `.github/agents/rust-reviewer_agent.md` | Severity Classification section referenced `RULE_OF_LAW §8.1` — section does not exist. | ✅ Fixed (→ `§5, §3.2`) |
| M-4 | `.github/agents/planner_agent.md` | "Before Planning" section referenced `RULE_OF_LAW §7.1` — section does not exist. | ✅ Fixed (→ `§3.4`) |
| M-5 | `.claude/commands/audit.md` | All 20 path occurrences used `/home/cam/Documents/Status/` (old, wrong path). Output directory was `/home/cam/Documents/Status/audits/`. | ✅ Fixed (→ `/home/cam/Documents/Desktop/Status/DOCS/audits/`) |
| M-6 | `crates/parapet_bar/src/main.rs` | Step comments misnumbered: duplicate `// ── 2.` for both CLI args and Config; then jumped to `// ── 5.` for GTK init (skipping 3 and 4). | ✅ Fixed (renumbered 2–11 sequentially) |
| M-7 | `crates/parapet_bar/src/main.rs` (signal handler) | `let _ = sig_tx.send(())` discarded a `Result` without a comment. CODING_STANDARDS §3.5 prohibits silent discards. | ✅ Fixed (justification comment added; see DOCS/conflict.md) |
| M-8 | `DOCS/futures.md` | Completed entry for `VolumeWidget` claimed `pactl get-sink-info` replaced the two-call approach (Part 1). Code still uses two separate `pactl` calls in `read_volume_info()`. | ✅ Fixed (entry corrected; two-call approach acknowledged as present but no longer per-poll) |
| M-9 | `DOCS/plan.md` | `Status: Completed` (VolumeWidget optimization, 2026-03-18). Per copilot-instructions.md plan lifecycle, completed plans must be deleted before a new plan is written. File is *not* deleted by the Auditor — this is user or Planner agent action. | `[USER ACTION]` Delete `DOCS/plan.md` before writing any new plan. |
| M-10 | Repo root | `CHANGELOG.md` does not exist. `RELEASE_GUIDE §2` lists it in the pre-release checklist; `RELEASE_GUIDE §4.3` uses it for GitHub Releases. No changelog means the first release cannot follow the established process. | `[USER ACTION]` Create `CHANGELOG.md` using the format in `RELEASE_GUIDE §5`. |
| M-11 | `DOCS/` | `DOCS/conflict.md` did not exist. RULE_OF_LAW §3.2 requires creating it when a standard is silent on a topic. | ✅ Fixed (created with 2 entries) |

---

## Low Violations

| # | Location | Violation | Status |
|---|----------|-----------|--------|
| L-1 | `crates/parapet_core/src/widgets/volume.rs` (subscribe_loop) | `unreachable!()` used in production code. The invariant (`stdout` is always `Some` when `Stdio::piped()` is set) is valid per the stdlib contract and is documented. Not a practical risk. | ✅ Documented in `DOCS/conflict.md` |
| L-2 | `standards/AGENT_GUIDE.md`, `standards/SECURITY.md` | No standards document governed agent file maintenance or security practices. Both gaps produced real problems (stale agent files, no `cargo audit` requirement). | ✅ Created both standards (see §Standards Updates Made) |
| L-3 | `standards/` | `CODING_STANDARDS §3.5` does not mention signal-handler `send()` discards as an accepted `let _ =` exception. | ✅ Documented in `DOCS/conflict.md`; follow-up: update §3.5 explicitly |
| L-4 | `standards/` | `CODING_STANDARDS §3.3` does not mention `unreachable!()` with documented invariant as an accepted pattern. | ✅ Documented in `DOCS/conflict.md`; follow-up: update §3.3 explicitly |
| L-5 | `doa/` | Directory does not exist. RULE_OF_LAW §4.1 defines it as the archive for removed code. No code has been removed yet, but creating the directory proactively avoids governance ambiguity. | `[USER ACTION]` `mkdir doa/` and add a `.gitkeep` |

---

## Standards Updates Made

| File | Change | Rationale |
|------|--------|-----------|
| `standards/ARCHITECTURE.md` | §4.2 `WidgetData` snippet updated (3 fields corrected); §5 table: removed non-existent `config.rs`; §6.1 startup sequence rewritten to 11-step | Stale documentation — code was correct |
| `standards/WIDGET_API.md` | §4 `Cpu` variant: fixed malformed field formatting | Typo — doc comment inline with code |
| `standards/BUILD_GUIDE.md` | §2.1 GTK feature `v3_22` → `v3_24`; 5 workspace deps added; §2.2 parapet_core template: 4 deps added, `tempfile` format corrected | Wrong GTK feature would cause failed builds following the guide |
| `standards/UI_GUIDE.md` | §3.1 `WindowType::Popup` → `Toplevel` (with rationale) | Code correct; standard wrong — could confuse future contributors |
| `standards/BAR_DESIGN.md` | §2.1 WindowType table: `Popup` → `Toplevel` | Same as above |
| `standards/RULE_OF_LAW.md` | §2 hierarchy: added RELEASE_GUIDE (11), AGENT_GUIDE (12), SECURITY (13) | RELEASE_GUIDE existed but wasn't in the table; AGENT_GUIDE and SECURITY are new |
| `standards/AGENT_GUIDE.md` | **Created** (new standard, priority 12) | No governance for agent file maintenance; files had drifted severely |
| `standards/SECURITY.md` | **Created** (new standard, priority 13) | No documented security practices despite RULE_OF_LAW §4.6 requiring urgent CVE response |
| `.github/copilot-instructions.md` | Module table: 8 → 15 entries; all 11 widgets + corrected descriptions | 5 widgets were completely absent from Copilot context |
| `.github/agents/rust-reviewer_agent.md` | `RULE_OF_LAW §8.1` → `§5, §3.2` | §8.1 does not exist |
| `.github/agents/planner_agent.md` | `RULE_OF_LAW §7.1` → `§3.4` | §7.1 does not exist |
| `.claude/commands/audit.md` | 20 path occurrences: `/home/cam/Documents/Status/` → `/home/cam/Documents/Desktop/Status/DOCS/`; Severity Reference section reference updated | All paths were wrong (old root); report output pointed to non-existent directory |

---

## Code Changes Made

| File | Change | Rationale |
|------|--------|-----------|
| `crates/parapet_bar/src/main.rs` | Step comments renumbered (2–11 sequentially; previously had duplicate "2." and jumped from 2 to 5) | Numbering mismatch with module-level doc comment |
| `crates/parapet_bar/src/main.rs` | Signal handler `let _ = sig_tx.send(())`: added justification comment | CODING_STANDARDS §3.5 requires explanation for discarded Results |
| `DOCS/futures.md` | VolumeWidget Part 1 claim corrected in both Technical Debt stub and Completed entry | Code audit confirmed two-call approach remains; futures.md was inaccurate |
| `DOCS/conflict.md` | **Created** with two entries: signal handler `let _ =` pattern, and `unreachable!()` invariant | RULE_OF_LAW §3.2 requires documenting standards gaps |

---

## conflict.md Entries Added

Two entries created in `DOCS/conflict.md`:

1. **Signal-handler `let _ = Result` is not a standards violation** — `CODING_STANDARDS §3.5` is silent on the ctrlc signal handler pattern. Resolution: a discarded `send()` error in a signal handler is accepted when the error case (channel disconnected) means shutdown is already in progress. Follow-up: explicitly list this exception in §3.5.

2. **`unreachable!()` in `subscribe_loop` is not a panic risk** — `CODING_STANDARDS §3.3` does not address `unreachable!()`. Resolution: `unreachable!()` with a documented compile-time or API-contract invariant is treated as equivalent to `.expect()` with invariant explanation. Follow-up: add to §3.3.

---

## futures.md Updates

| Item | Action |
|------|--------|
| VolumeWidget Technical Debt stub | Updated to say "Part 2 completed; Part 1 (single `pactl get-sink-info`) not implemented; two-call approach remains in helper but no longer per-poll" |
| VolumeWidget Completed entry | Removed inaccurate Part 1 claim; accurately describes Part 2 (event-driven subscribe thread) with note about Part 1 status |

No items were moved between sections (no work items completed during this audit cycle).

---

## Standards Health

### Coverage Gaps Closed This Session
- Agent file governance: `AGENT_GUIDE.md` created
- Security practices: `SECURITY.md` created
- `RELEASE_GUIDE.md` now indexed in `RULE_OF_LAW §2` hierarchy

### Remaining Gaps in Standards
These gaps are documented in `DOCS/conflict.md` but the standards themselves have not yet been updated:

1. `CODING_STANDARDS §3.5` does not list signal-handler `send()` discard as an accepted `let _ =` exception
2. `CODING_STANDARDS §3.3` does not explicitly permit `unreachable!()` with documented invariant

### Cross-Reference Health
All `[StandardName.md §N]` references checked in `.github/agents/` files — 2 broken references repaired (rust-reviewer_agent.md, planner_agent.md). All other references verified as valid.

### Module Table Currency
`ARCHITECTURE.md §4` and `ARCHITECTURE.md §5` tables now match the actual source tree after removing the non-existent `config.rs` entry from the `parapet_bar` table.

`WIDGET_API.md §4` `WidgetData` variants checked against `crates/parapet_core/src/widget.rs` — all 11 widget variants present and correctly described.

`CONFIG_MODEL.md` checked against `crates/parapet_core/src/config.rs` — all `ParapetConfig` and `WidgetKind` enum variants are present. No discrepancies found.

---

## Summary

**15 violations found.** All were corrected except 3 that require user action (delete `DOCS/plan.md`, create `CHANGELOG.md`, create `doa/` directory). No critical violations were present.

**2 new standards written:** `AGENT_GUIDE.md` (priority 12) and `SECURITY.md` (priority 13). Both address real governance gaps that produced actual violations in this audit.

**Root cause of most violations:** Documentation written during early development was never synchronized with code changes. The module tables, `WidgetData` code snippet, startup sequence, and dependency lists all drifted as the feature set expanded from 5 to 11 widgets and additional infrastructure was added. The `AGENT_GUIDE.md` standard now establishes explicit governance obligations to prevent this class of drift.

**No code correctness issues.** All Rust code examined for error handling, `unsafe`, and architectural violations is compliant. The `let _ =` and `unreachable!()` edge cases are well-justified and have now been documented.

### User Actions Required

All user action items completed on 2026-03-21:

| Priority | Action | Status |
|----------|--------|--------|
| 1 | `DOCS/plan.md` archived to `doa/plan_2026-03-18_volume-optimization.md` | ✅ Done |
| 2 | `CHANGELOG.md` created at repo root with `[0.1.0-alpha]` entry | ✅ Done |
| 3 | `doa/` directory created with `.gitkeep` | ✅ Done |
| 4 | `CODING_STANDARDS §3.3` updated to permit `unreachable!()` with documented invariant | ✅ Done |
| 5 | `CODING_STANDARDS §3.5` updated with two accepted `let _ =` exceptions | ✅ Done |

Both `DOCS/conflict.md` entries moved to `## Resolved` after their follow-up standards were updated.
