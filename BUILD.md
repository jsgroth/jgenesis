# Dependencies

## Rust

This project requires the latest stable version of the [Rust toolchain](https://doc.rust-lang.org/book/ch01-01-installation.html) to build.

## SDL3

This project uses [SDL3](https://www.libsdl.org/) for windowing, audio playback, and reading keyboard/gamepad/mouse inputs.

By default, SDL3 is fetched and built from source while building this project, and it is dynamically linked at runtime.
This behavior is customizable using [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html):

| Feature                  | Description                      | Default |
|--------------------------|----------------------------------|---------|
| `sdl3-build-from-source` | Fetch and build SDL3 from source | Yes     |
| `sdl3-static-link`       | Statically link SDL3             | No      |

Building SDL3 from source requires a C build toolchain. See also:
* [SDL3 Linux README](https://github.com/libsdl-org/SDL/blob/main/docs/README-linux.md)
* [SDL3 Windows README](https://github.com/libsdl-org/SDL/blob/main/docs/README-windows.md)

Example command to disable building SDL3 from source:

```shell
cargo build --no-default-features
```

Example command to build while statically linking SDL3:

```shell
cargo build --features sdl3-static-link
```

### DirectX Shader Compiler (Windows DX12 backend only)

The DirectX 12 wgpu backend is currently configured in such a way that it requires DLLs for Microsoft's DirectX shader compiler. The latest release is available here: <https://github.com/microsoft/DirectXShaderCompiler/releases>

`dxcompiler.dll` and `dxil.dll` must be present in the current working directory for the DirectX 12 backend to work.

## Build & Run

Build and run GUI:

```shell
cargo run --release --bin jgenesis-gui
```

Build and run CLI:

```shell
cargo run --release --bin jgenesis-cli -- -f /path/to/rom/file
```

For one-time builds, the `release-lto` profile enables fat LTOs and a few other build settings that improve runtime performance and decrease binary size at the cost of much longer compile times:

```shell
cargo build --profile release-lto -p jgenesis-gui
```

```shell
cargo build --profile release-lto -p jgenesis-cli
```

...After which the binaries will be in `target/release-lto/`.

If you are building for usage solely on your own machine, you can additionally set the compiler flag `-C target-cpu=native` to tell the compiler that it can use any CPU instruction that your computer's CPU supports, which may slightly improve performance:

```shell
RUSTFLAGS="-C target-cpu=native" cargo build --profile release-lto
```

`-C target-cpu=native` is not recommended for shared or distributed builds because the binaries may contain instructions that are only supported on recent CPUs, e.g. AVX-512 instructions. For shared/distributed builds it is better to use a specific CPU target such as `-C target-cpu=x86-64-v3` (allows the compiler to use AVX2, FMA, LZCNT, etc).

On Linux, the following command will build AppImage packages (requires [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)):

```shell
cargo packager --profile release-lto -f appimage
```
