# Frames justfile
# Install just: cargo install just
# Usage: just check | just check-headless | just install-hooks

# Run full verification suite (requires display for frames_bar GTK tests)
check:
    cargo build --workspace
    cargo clippy --workspace -- -D warnings
    cargo fmt --all -- --check
    cargo test --workspace

# Run headless-safe verification suite (no display required — safe for CI)
check-headless:
    cargo build --workspace
    cargo clippy --workspace -- -D warnings
    cargo fmt --all -- --check
    cargo test --workspace --no-default-features

# Install the pre-commit hook (run once per clone)
install-hooks:
    #!/usr/bin/env sh
    HOOK=.git/hooks/pre-commit
    echo '#!/usr/bin/env sh' > "$HOOK"
    echo 'just check-headless' >> "$HOOK"
    chmod +x "$HOOK"
    echo "Pre-commit hook installed at $HOOK"
