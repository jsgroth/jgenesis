#!/usr/bin/env bash

set -euo pipefail

RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+atomics,+bulk-memory,+mutable-globals" \
rustup run nightly \
wasm-pack build --target web . "$@" -- -Z build-std=panic_abort,std
