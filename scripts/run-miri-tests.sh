#!/usr/bin/env bash

set -euo pipefail

# Windowed sinc interpolator tests - reads from raw pointers using x86_64 intrinsics
cargo +nightly miri test -p dsp

# Boxed 2D array tests - allocates memory using unsafe code
cargo +nightly miri test -p jgenesis-common

# 32X bus tests - uses raw pointers to avoid lifetime params on the 32X SH-2 bus struct
cargo +nightly miri test -p s32x-core
