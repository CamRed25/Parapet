# Parapet — Agent Guide

> **Scope:** All AI agent customization files in `.github/agents/`, `.github/copilot-instructions.md`, and `.claude/commands/`.
> **Authority:** RULE_OF_LAW.md §2 (priority 12)
> **Last Updated:** 2026-03-21

---

## 1. Purpose

This standard governs the creation, maintenance, and review of agent customization files. These files configure how AI coding assistants (GitHub Copilot, Claude) interact with this codebase. Stale or inaccurate agent files lead to AI-generated code that violates project standards, references non-existent modules, or uses wrong paths.

**Why this matters:** During the March 2026 audit, agent files in `.github/agents/` were found to have: module tables covering only 5 of 11 widgets, references to non-existent `RULE_OF_LAW §7.1` and `§8.1` sections, and workspace paths pointing to the wrong directory. Agent files are documentation — they must be held to the same accuracy standard as any other documentation.

---

## 2. File Inventory

| Path | Role | Agent |
|------|------|-------|
| `.github/copilot-instructions.md` | Global Copilot context for all requests | GitHub Copilot |
| `.github/agents/auditor_agent.md` | Auditor slash command definition | Copilot custom agent |
| `.github/agents/planner_agent.md` | Planner agent workflow | Copilot custom agent |
| `.github/agents/rust-reviewer_agent.md` | Rust code review agent | Copilot custom agent |
| `.github/agents/pre-commit_agent.md` | Pre-commit check agent | Copilot custom agent |
| `.github/agents/widget_design_agent.md` | Widget design assistant | Copilot custom agent |
| `.github/agents/changelog_agent.md` | Changelog maintenance agent | Copilot custom agent |
| `.github/agents/research_agent.md` | Research and spike assistant | Copilot custom agent |
| `.github/agents/config_migration_agent.md` | Config migration assistant | Copilot custom agent |
| `.claude/commands/audit.md` | `/audit` slash command | Claude |

---

## 3. Mandatory Content Requirements

### 3.1 Module Tables

Any agent file that contains a module table refencing `parapet_core` or `parapet_bar` modules **must** be kept in sync with the actual source tree at:
- `crates/parapet_core/src/widgets/` — one row per widget module
- `crates/parapet_bar/src/widgets/` — one row per renderer

**Each module table entry must include:** module path, brief role description.

**Verification:** Run `ls crates/parapet_core/src/widgets/` and `ls crates/parapet_bar/src/widgets/` and confirm all modules appear in the table.

### 3.2 Standards Section References

Any `[StandardName.md §N]` reference in an agent file must:
1. Point to a section that actually exists in the named standard
2. Use the exact section number as rendered in that document's headings

**To verify:** Open the referenced standard and confirm the heading `## N.` or `### N.N` exists.

### 3.3 File Paths

All absolute paths in agent files must match the actual workspace root. The workspace root is `/home/cam/Documents/Desktop/Status`.

**Never use:**
- `/home/cam/Documents/Status/` (old path — moved to Desktop subfolder)
- Relative paths for output files
- Platform-specific separators

### 3.4 Handoff Fields

Agent files with a `handoffs:` YAML front-matter key must list valid agent names that correspond to files actually present in `.github/agents/`.

---

## 4. When to Update Agent Files

Agent files **must** be updated whenever:

| Trigger | Files to Update |
|---------|----------------|
| New widget module added to `parapet_core` | `.github/copilot-instructions.md` module table; any agent with a widget table |
| New module added to `parapet_bar` | `.github/copilot-instructions.md` module table |
| Standards document section renumbered | All agent files with references to that section |
| Workspace root path changes | All agent files with absolute paths |
| New agent file created | Update the inventory table in this document (§2) |
| New standard added to `standards/` | Update any agent that lists the standards hierarchy |

---

## 5. Creating a New Agent File

1. Place the file in `.github/agents/<role_name>_agent.md`
2. Begin the file with a YAML front-matter block:
```yaml
---
name: <role_name>
description: <one-sentence description>
tools: [<comma-separated tool names>]
---
```
3. Include a "Standards Authority" section listing which `standards/` documents the agent must read before acting.
4. Add the new file to the inventory table in §2 of this document.
5. Add any `handoffs:` references to this file in agents that may delegate to it.

---

## 6. Audit Checklist for Agent Files

Run this checklist during every audit cycle:

- [ ] Module tables list all widgets in `parapet_core/src/widgets/` and `parapet_bar/src/widgets/`
- [ ] All `StandardName.md §N` references resolve to real headings
- [ ] All absolute paths use `/home/cam/Documents/Desktop/Status/`
- [ ] `handoffs:` fields reference agents that exist
- [ ] No agent file was added without an entry in §2 of this document
- [ ] `.github/copilot-instructions.md` crate graph matches `Cargo.toml` workspace members
