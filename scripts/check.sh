#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --release
