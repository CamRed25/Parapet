// Archived 2026-03-22 from crates/parapet_bar/src/widgets/workspaces.rs
// Reason: replaced by inline gdk::property_get() calls in read_workspaces().
// See DOCS/cleanup.md and DOCS/plan.md (Plan: WorkspacesWidget Step 1).
//
// These helpers parsed text output from `xprop -root` subprocess calls.
// They have no further reuse value after the subprocess was eliminated.

/// Parse a `CARDINAL` integer from an `xprop -root` output line.
///
/// Expected format: `ATOM_NAME(CARDINAL) = N`
fn parse_xprop_cardinal(line: &str) -> Option<usize> {
    line.split_once("= ").and_then(|(_, v)| v.trim().parse::<usize>().ok())
}

/// Parse a `UTF8_STRING` list from an `xprop -root` output line.
///
/// Expected format: `ATOM_NAME(UTF8_STRING) = "val1", "val2"`
fn parse_xprop_utf8_list(line: &str) -> Vec<String> {
    let Some((_, values)) = line.split_once("= ") else { return Vec::new() };
    values
        .split(", ")
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
