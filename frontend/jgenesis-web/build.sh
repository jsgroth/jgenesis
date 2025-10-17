#!/usr/bin/env bash

set -euo pipefail
set +x

toolchain="${JGENESIS_TOOLCHAIN:-nightly}"

rustup run $toolchain wasm-pack build --target web . "$@" -- -Z build-std=panic_abort,std
