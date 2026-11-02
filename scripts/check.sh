#!/usr/bin/env sh
# rustfmt on Newen's own files: `cargo fmt --all` would also reformat limn, a path dependency.
set -eu
cd "$(dirname "$0")/.."
find apps tools crates -name '*.rs' 2>/dev/null | xargs rustfmt --check --edition 2021
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
