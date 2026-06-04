# zj-prompt-jumper task runner.
#
# Run `just --list` to see all recipes.

# The plugin is loaded by zellij directly from the cargo build output,
# referenced by the `prompt-jumper` alias in ~/.config/zellij/config.kdl.
plugin_url := "file:" + justfile_directory() + "/target/wasm32-wasip1/release/zj-prompt-jumper.wasm"

# Default to the dev loop: build + reload in the running Zellij session.
default: reload

# Build the plugin in release mode for Zellij's wasm target.
build:
    cargo build --release --target wasm32-wasip1

# Build, then hot-reload the running plugin (and close the spurious side pane).
reload: build
    #!/usr/bin/env bash
    # `start-or-reload-plugin` matches running instances by (URL, configuration).
    # The plugin is headless (loaded via `load_plugins`), but the CLI still
    # spawns a temporary pane to attach; capture and close it.
    set -euo pipefail
    pid=$(zellij action start-or-reload-plugin {{plugin_url}})
    if [[ "$pid" =~ ^plugin_[0-9]+$ ]]; then
        zellij action close-pane --pane-id "$pid" 2>/dev/null || true
    fi

# Run the full test suite.
test:
    cargo test

# Lint with clippy on both the wasm build and the test build.
lint:
    cargo clippy --target wasm32-wasip1 -- -D warnings
    cargo clippy --tests -- -D warnings

# Format check (CI-friendly).
fmt-check:
    cargo fmt --all -- --check

# Apply rustfmt.
fmt:
    cargo fmt --all

# Remove build artifacts.
clean:
    cargo clean
