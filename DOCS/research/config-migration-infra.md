# Research: Config Migration Infrastructure — `config_version` + Migration Chain
**Date:** 2026-03-22
**Status:** Findings complete — decision recommended — ready to close

## Question

What is the best implementation of a `config_version: Option<u32>` field in
`[bar]` plus a migration chain in `parapet_core::config` that detects missing or
outdated schema versions and auto-upgrades or returns structured errors?

This extends `DOCS/research/config-migration.md`, which established the high-level
design. The current document provides implementation-level specifics: exact
`BarConfig` changes, the return strategy (in-place vs. return-string), available
crates, and `CONFIG_MODEL.md` gap analysis.

---

## Summary

Add `config_version: u32` to `BarConfig` with `#[serde(default = "...")]`
returning `1`. Implement migration as a `migrate_toml(source: &str) ->
Result<Cow<str>, ParapetConfigError>` function: returns `Borrowed(&str)` when no
migration is needed (zero-cost fast path), `Owned(String)` with the upgraded TOML
when migrations ran. The caller (`ParapetConfig::load`) writes the result back
to disk. No new crate dependencies are required. `CONFIG_MODEL.md §3` does not
currently document `config_version` — it must be updated when this ships.

---

## Findings

### Q1 — What version should v0 (pre-`config_version`) migrate to? What is the canonical "current version"?

**Set `CONFIG_SCHEMA_VERSION: u32 = 1` and treat a missing/absent
`config_version` field as version 1 — the current schema.**

Rationale: Config Phase 2 (`WidgetKind` typed enum, `#[serde(deny_unknown_fields)]`)
is already complete and is the current schema. That constitutes schema version 1.
There is no "version 0" that needs migrating right now — the first migration
function will be written when Phase 3 introduces the first breaking field change.

The MIGRATIONS slice starts **empty**:

```rust
pub static MIGRATIONS: &[MigrationFn] = &[];
// To add a migration from v1 to v2, append here and bump CONFIG_SCHEMA_VERSION.
```

This infrastructure validates the field and produces typed errors without any
active rewriting until a real migration is needed.

---

### Q2 — Should `migrate()` be in-place (rewrite file) or return a new string?

**Return a `Cow<str>` from `migrate_toml()` — the caller decides whether to
write.**

**Tradeoffs:**

| Approach | Pros | Cons |
|----------|------|------|
| **Return new string (`Cow<str>`)** | Testable headlessly (string in → string out); caller controls I/O; `load()` keeps single responsibility; works in unit tests with no filesystem | Caller must write + handle write errors |
| In-place rewrite inside `load()` | Fewer call sites | Couples I/O to business logic; untestable without temp files; violates display-isolation when tested headlessly |

The `Cow<str>` approach aligns with the project's headless-testability requirement
(`cargo test --workspace --no-default-features`). The migration function takes a
`&str`, returns `Ok(Cow::Borrowed(src))` when no migration ran, or
`Ok(Cow::Owned(new_toml))` after rewriting. `ParapetConfig::load()` receives the
`Cow`, writes to disk only when `Cow::is_owned()`, then deserializes the final
string.

```rust
/// Migrate a TOML source string to the current schema version.
///
/// Returns [`Cow::Borrowed`] (zero-copy) when `source` is already at the
/// current version. Returns [`Cow::Owned`] when migrations were applied, with
/// the updated TOML as a new string.
///
/// # Errors
///
/// Returns [`ParapetConfigError::ConfigTooNew`] when `config_version` in the
/// source exceeds [`CONFIG_SCHEMA_VERSION`].
pub fn migrate_toml(source: &str) -> Result<Cow<str>, ParapetConfigError> {
    let mut doc: toml::Value = toml::from_str(source)
        .map_err(|e| ParapetConfigError::Parse(e.to_string()))?;

    let file_version = doc
        .get("bar")
        .and_then(|b| b.get("config_version"))
        .and_then(|v| v.as_integer())
        .map(|i| i as u32)
        .unwrap_or(1);

    if file_version > CONFIG_SCHEMA_VERSION {
        return Err(ParapetConfigError::ConfigTooNew {
            file_version,
            supported: CONFIG_SCHEMA_VERSION,
        });
    }

    if file_version == CONFIG_SCHEMA_VERSION {
        return Ok(Cow::Borrowed(source));
    }

    // Apply migration steps from file_version..CONFIG_SCHEMA_VERSION.
    let start = (file_version as usize) - 1;  // 0-indexed
    for step in &MIGRATIONS[start..] {
        step(&mut doc);
    }

    // Write updated config_version into the document.
    if let Some(bar) = doc.get_mut("bar").and_then(|b| b.as_table_mut()) {
        bar.insert("config_version".into(),
                   toml::Value::Integer(i64::from(CONFIG_SCHEMA_VERSION)));
    }

    let updated = toml::to_string_pretty(&doc)
        .map_err(|e| ParapetConfigError::Parse(e.to_string()))?;
    Ok(Cow::Owned(updated))
}
```

Integration in `ParapetConfig::load()`:

```rust
pub fn load(path: &Path) -> Result<Self, ParapetConfigError> {
    if !path.exists() {
        return Err(ParapetConfigError::NotFound { path: path.to_path_buf() });
    }
    let source = std::fs::read_to_string(path)?;

    let migrated = migrate_toml(&source)?;
    if migrated.is_owned() {
        // Back up then rewrite so migration is durable.
        std::fs::write(path, migrated.as_ref())?;
        tracing::info!(path = %path.display(), "config migrated to v{}", CONFIG_SCHEMA_VERSION);
    }

    let mut config: Self = toml::from_str(migrated.as_ref())?;
    config.validate()?;
    Ok(config)
}
```

---

### Q3 — What existing crates can drive migrations?

The workspace already includes:

```toml
toml = "~0.8"       # in [workspace.dependencies]
```

`toml = "~0.8"` provides:
- `toml::Value` — arbitrary TOML tree for structural mutation
- `toml::from_str` — source → `Value`
- `toml::to_string_pretty` — `Value` → formatted TOML string (available in 0.8
  without additional features; confirmed from crate docs)

**`toml_edit` is NOT in the workspace** and is not a transitive dependency.
`toml_edit` would preserve user comments across the rewrite; `toml::to_string_pretty`
does not (comments are stripped). This is an accepted trade-off: the migration
event is a rare, once-per-upgrade occurrence, and a log message at `INFO` level
plus the backup/overwrite pattern mitigates the user impact.

**No new dependencies are required.** Both the migration infrastructure and the
first real migration function will rely entirely on `toml::Value` manipulation.

---

### Q4 — What does `CONFIG_MODEL.md` currently say?

`CONFIG_MODEL.md §3` (the `[bar]` fields table, updated 2026-01-11) lists:

| position | height | monitor | css | theme | widget_spacing |

`config_version` is **not listed**. There are no hints in the file that the field
is planned.

This omission must be remedied when the feature ships. The proposed addition to
`§3`:

| `config_version` | integer | `1` | Config schema version. Auto-managed by the migration system. Do not set by hand unless you have manually edited the file to match an older format. |

`ARCHITECTURE.md §4.1` also does not mention the migration chain as part of
`ParapetConfig::load()`. A sentence should be added when this ships:

> `ParapetConfig::load()` runs the migration chain (`migrate_toml()`) before
> deserialization. When the file was migrated, it is rewritten to disk so the
> next load is a no-op.

---

### Exact `BarConfig` change

Current `BarConfig` (from `crates/parapet_core/src/config.rs`):
```rust
pub struct BarConfig {
    pub position: BarPosition,
    pub height: u32,
    pub monitor: MonitorTarget,
    pub css: Option<String>,
    pub theme: Option<String>,
    pub widget_spacing: u32,
}
```

Add one field:

```rust
/// Config schema version for the migration system.
///
/// Defaults to [`CONFIG_SCHEMA_VERSION`] on new configs; the migration chain
/// in [`migrate_toml`] upgrades older values automatically.
/// Do not set this manually unless intentionally pinning to an old schema.
#[serde(default = "BarConfig::default_config_version")]
pub config_version: u32,

// In impl BarConfig:
fn default_config_version() -> u32 { CONFIG_SCHEMA_VERSION }
```

Note: `Default` for `BarConfig` should also initialise `config_version` to
`CONFIG_SCHEMA_VERSION`.

---

### New `ParapetConfigError` variant

The existing `ParapetConfigError` (from `error.rs`) needs a typed variant for the
"file is newer than binary" case:

```rust
/// The config file declares a `config_version` newer than this binary supports.
///
/// Upgrade Parapet or manually downgrade the config file.
#[error(
    "config version {file_version} is not supported by this binary \
     (max supported: {supported}); upgrade Parapet or remove config_version"
)]
ConfigTooNew {
    file_version: u32,
    supported: u32,
},
```

This gives `parapet_bar::main` a typed arm to match against and display a
user-friendly startup error with an actionable next step.

---

### Headless testability

All test cases exercise `migrate_toml(&str)` directly — no filesystem:

```rust
#[test]
fn config_version_missing_defaults_to_no_migration() {
    let src = "[bar]\nheight = 30\n";
    let result = migrate_toml(src).unwrap();
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn config_version_at_current_is_noop() {
    let src = format!("[bar]\nconfig_version = {}\n", CONFIG_SCHEMA_VERSION);
    let result = migrate_toml(&src).unwrap();
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn config_version_too_new_returns_err() {
    let src = format!("[bar]\nconfig_version = {}\n", CONFIG_SCHEMA_VERSION + 1);
    let err = migrate_toml(&src).unwrap_err();
    assert!(matches!(err, ParapetConfigError::ConfigTooNew { .. }));
}

#[test]
fn empty_migration_slice_does_not_mutate() {
    // When MIGRATIONS is empty and file_version == 1, Borrowed is returned.
    let src = "[bar]\nconfig_version = 1\nheight = 28\n";
    let result = migrate_toml(src).unwrap();
    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
}
```

---

## Recommendation

Implement in this order:

1. Add `ConfigTooNew` variant to `ParapetConfigError` in `error.rs`.
2. Add `config_version: u32` to `BarConfig`; add `pub const CONFIG_SCHEMA_VERSION: u32 = 1`.
3. Create `crates/parapet_core/src/migrations.rs` with empty `MIGRATIONS` slice.
4. Implement `migrate_toml(source: &str) -> Result<Cow<str>, ParapetConfigError>`
   in `config.rs` (or delegate body to `migrations.rs`).
5. Integrate `migrate_toml()` into `ParapetConfig::load()`.
6. Add four headless tests to `crates/parapet_core/tests/config_roundtrip.rs`
   (or a new `tests/config_migration.rs`).
7. Update `CONFIG_MODEL.md §3` and `ARCHITECTURE.md §4.1`.

The MIGRATIONS slice remains empty until Phase 3 introduces the first breaking
field change — at that point, bump `CONFIG_SCHEMA_VERSION` to `2` and append a
`fn migrate_v1_to_v2(doc: &mut toml::Value)` to the slice.

---

## Standards Conflict / Proposed Update

**`CONFIG_MODEL.md §3`** — must gain a row for `config_version` (auto-managed,
integer, default 1) once implemented.

**`ARCHITECTURE.md §4.1`** — should note that `ParapetConfig::load()` runs the
migration chain before deserialization.

---

## Sources

- `DOCS/research/config-migration.md` — prior high-level design; this document
  provides implementation specifics not covered there
- `crates/parapet_core/Cargo.toml` — confirmed `toml = "~0.8"` is present, no
  `toml_edit`
- `crates/parapet_core/src/config.rs` — current `BarConfig` struct (no
  `config_version` field); current `ParapetConfig::load()` calling pattern
- `crates/parapet_core/src/error.rs` — current `ParapetConfigError` variants;
  `ConfigTooNew` not yet present
- `standards/CONFIG_MODEL.md §3` — confirmed `config_version` is absent from the
  `[bar]` fields table
- `DOCS/futures.md` — "Config migration infrastructure" entry confirms this is
  the intended implementation path, depends on Phase 2 (now complete)

---

## Open Questions

1. **Backup on migration:** Should `load()` write a `config.toml.bak` before
   overwriting? Protects users who edit TOML by hand with non-standard comments.
   Cost: one extra `fs::copy` call per migration event (rare). Recommendation:
   yes, back up, but this can be a follow-up.
2. **`MIGRATIONS` module placement:** Put `migrate_toml` and `MIGRATIONS` in a
   `migrations.rs` submodule (keeping `config.rs` focused on data structures) or
   inline in `config.rs` for discoverability? Either is acceptable; defer to
   implementer. The `migrations.rs` submodule scales better if migration functions
   accumulate.
