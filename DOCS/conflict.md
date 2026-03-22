# Parapet — Standards Conflicts and Gaps

Temporary entries for situations where a standard is silent or ambiguous.
Move to `## Resolved` with a date once the relevant standard is updated.

---

*(No open entries.)*

---

## Resolved

### Signal-handler `let _ = Result` is not a standards violation

- **Date:** 2026-03-21 | **Resolved:** 2026-03-21
- **Standards involved:** CODING_STANDARDS §3.5
- **Situation:** `CODING_STANDARDS §3.5` prohibited all `let _ =` on `Result`. `main.rs` uses `let _ = sig_tx.send(())` in the `ctrlc` signal handler; failure means shutdown is already in progress.
- **Resolution applied:** Added an explanatory comment at the callsite. `CODING_STANDARDS §3.5` updated to explicitly list signal-handler `send()` as an accepted exception with required comment.
- **Closed by:** `CODING_STANDARDS §3.5` update on 2026-03-21.

---

### `unreachable!()` in `subscribe_loop` is not a panic risk

- **Date:** 2026-03-21 | **Resolved:** 2026-03-21
- **Standards involved:** CODING_STANDARDS §3.3
- **Situation:** `CODING_STANDARDS §3.3` was silent on `unreachable!()`. `volume.rs::subscribe_loop` uses `unreachable!()` with a documented stdlib invariant (`stdout` is always `Some` when `Stdio::piped()` is set).
- **Resolution applied:** `CODING_STANDARDS §3.3` updated to explicitly permit `unreachable!()` with a `// SAFETY:` or `// INVARIANT:` comment documenting the compile-time or API-contract invariant.
- **Closed by:** `CODING_STANDARDS §3.3` update on 2026-03-21.
