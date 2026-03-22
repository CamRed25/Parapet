# Parapet — Security Guide

> **Scope:** All contributors (human and AI agents) working on `parapet/`.
> **Authority:** RULE_OF_LAW.md §2 (priority 13)
> **Last Updated:** 2026-03-21

---

## 1. Purpose

This standard defines security practices for building, maintaining, and releasing Parapet. It covers dependency vulnerability management, `unsafe` code hygiene, responsible disclosure, and the actions required when a security issue is discovered.

`RULE_OF_LAW §4.6` states "Security advisory: Fix immediately regardless of breaking changes." This document defines *how* to discover, triage, and respond.

---

## 2. Dependency Vulnerability Scanning

### 2.1 `cargo audit`

Run `cargo audit` before every release and after updating any dependency:

```bash
cargo audit
```

`cargo audit` checks `Cargo.lock` against the [RustSec Advisory Database](https://rustsec.org/). Any advisory at severity **medium** or higher is a blocker for release.

**Install:**
```bash
cargo install cargo-audit
```

### 2.2 Frequency

| Trigger | Action |
|---------|--------|
| Before any release tag | Run `cargo audit` — must be clean |
| After `cargo update` | Run `cargo audit` |
| Weekly (optional, CI-aided) | Automated advisory check |
| RustSec advisory published for a dependency | Patch or replace within 7 days for high/critical; 30 days for medium |

### 2.3 Pinned Dependency Versions

Workspace `Cargo.toml` uses `~X.Y` (tilde) constraints, not `=X.Y.Z` pins. This allows patch-level updates to land without a PR. When `cargo audit` flags a version, bump the lower bound in the workspace `Cargo.toml` so the advisory-affected version is excluded.

---

## 3. `unsafe` Code Hygiene

### 3.1 Every `unsafe` Block Must Have `// SAFETY:`

Per `CODING_STANDARDS §6`, every `unsafe` block must be preceded by a `// SAFETY:` comment explaining:
1. Why the unsafe operation is valid
2. What invariants are relied upon
3. Which function or library contract guarantees those invariants

Missing `// SAFETY:` comments are a **CRITICAL** violation and a blocker for CI.

### 3.2 Unsafe Audit Frequency

All `unsafe` blocks must be audited whenever:
- The function or data structure containing the block is modified
- A dependency providing the invariant is updated (e.g. `glib`, `gtk`, `libc`)
- A new unsafe block is introduced (reviewer must independently verify)

```bash
# Find all unsafe blocks in production code
grep -rn "unsafe" crates/ --include="*.rs" | grep -v "#\[cfg(test)\]"
```

### 3.3 No Unbounded FFI

Raw pointer arithmetic and unchecked slice operations must be wrapped in a dedicated module (e.g. `ffi.rs`) with its own module-level safety invariant documentation. Do not scatter raw FFI operations across unrelated modules.

---

## 4. User Input and External Data

### 4.1 TOML Config

Config is read from `~/.config/parapet/config.toml`. Malformed TOML returns a `ParapetError::Config` and the bar exits gracefully with an error message. Never `unwrap()` on config values at parse time.

### 4.2 Theme Paths

Theme filenames are user-supplied via the `--theme` CLI flag and `bar.theme` config key. `resolve_theme_path()` in `css.rs` enforces a path-traversal guard: the resolved path must be within `~/.config/parapet/themes/`. Never pass user-supplied paths directly to file operations without this guard.

**Do not weaken this guard** — it prevents `--theme ../../.ssh/id_rsa`-style reads.

### 4.3 Shell Commands (`spawn_shell`)

Per-widget `on_click`, `on_scroll_up`, and `on_scroll_down` fields execute arbitrary shell commands via `spawn_shell()`. This is explicitly a user-configurable feature (not automatic). However:
- Never pass widget data (CPU%, temperature, etc.) into a shell command string without sanitization
- Widget-data interpolation into shell commands is **not currently supported** and **must not be added** without an explicit security review

### 4.4 HTTP Requests (Weather Widget)

The weather widget makes outbound HTTP requests to Open-Meteo (`api.open-meteo.com`). Allowed:
- `latitude` and `longitude` from user config are URL-encoded by `ureq` before use
- Open-Meteo is a fixed upstream — do not make this endpoint user-configurable

Do not add general-purpose HTTP request widgets or config-driven URL fields without a security review.

### 4.5 D-Bus (Media Widget)

The media widget queries the D-Bus session bus via `zbus`. Session-bus calls are limited to the user's own session. Do not add system-bus (`connection.new_system()`) calls.

---

## 5. Security Advisory Response

### 5.1 Severity Definitions

| Severity | Definition | Response Time |
|----------|------------|---------------|
| **Critical** | Remote code execution, privilege escalation, data exfiltration | Fix within 24 hours; emergency patch release |
| **High** | Privilege boundary violation, path traversal, shell injection | Fix within 7 days; patch release |
| **Medium** | Denial of service, information disclosure | Fix within 30 days; next regular release |
| **Low** | Non-exploitable or theoretical | Fix in next regular release |

### 5.2 Response Process

1. Identify the affected component (crate, feature, config field)
2. Check whether the vulnerability is reachable in Parapet's execution model
3. If reachable: create a fix branch, update `CHANGELOG.md` with a `### Security` section, bump the patch version, tag, and release
4. If not reachable: document why in `DOCS/conflict.md` under a "Security Non-Issue" heading
5. For dependency CVEs that cannot be patched immediately: add a `[patch.crates-io]` override in the workspace `Cargo.toml` pointing to a patched fork, and open an issue tracking the upstream fix

### 5.3 CHANGELOG Entry for Security Fixes

```markdown
### Security

- **[HIGH]** Fixed path-traversal in `resolve_theme_path()` where filenames
  containing `../` sequences could escape the themes directory.
  ([CVE-XXXX-XXXX](https://...))
```

---

## 6. What Is Out of Scope

- Sandboxing or privilege separation: Parapet runs as a normal user process; no setuid, no capabilities
- Network listener hardening: Parapet has no network server
- Cryptography: no cryptographic operations are performed; no keys, tokens, or passwords are stored

---

## 7. CI Integration

Add to the CI pipeline (`.github/workflows/`):

```yaml
- name: Security audit
  run: cargo audit --deny warnings
```

This fails the CI build on any advisory at medium severity or higher.
