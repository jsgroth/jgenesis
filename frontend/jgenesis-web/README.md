# jgenesis-web

Minimal web frontend that compiles to WASM and runs in the browser.

## Dependencies

### Rust Nightly

The web frontend's audio implementation requires sharing memory between the main thread and the
audio worklet thread, which is not supported in stable Rust when compiling to WASM. Even in nightly,
this requires rebuilding the standard library with atomics support.

Assuming you already have the basic Rust toolchain installed:
```
rustup toolchain add nightly --component rust-src
```

### wasm-pack

wasm-pack is used to build and package the WASM/JS files. Here it's used mainly just as a convenience
wrapper around wasm-bindgen-cli and wasm-opt.

To install:
```
cargo install wasm-pack
```

## Build

The following incantation builds the required WASM/JS files into the `pkg` directory:

```
RUSTFLAGS='--cfg getrandom_backend="wasm_js" -C target-feature=+atomics,+bulk-memory,+mutable-globals' \
rustup run nightly \
wasm-pack build --target web . -- -Z build-std=panic_abort,std
```

The provided `build.sh` script runs this command for you:
```
./build.sh
```

For development, the `--dev` flag can be used to disable LTOs and wasm-opt, which gives
significantly shorter compile times at the cost of larger file size and worse performance:
```
./build.sh --dev
```

## Run

Copy `index.html`, the `js` directory, and the `pkg` directory into the webserver of your choice.

The webserver must set the following HTTP headers on every request or the application may not work
properly in some browsers:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

The provided `webserver.py` runs a local webserver that automatically sets these headers on every request.

```
# Binds to localhost:8080 by default
./webserver.py
```
```
# Binds to a specific address:port
./webserver.py localhost:9000
```
