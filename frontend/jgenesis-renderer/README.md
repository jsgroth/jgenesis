# jgenesis-renderer

Code for the `wgpu`-based renderer, which is split into a separate crate so that it can be used in `jgenesis-web` without pulling in SDL2 dependencies (which do not support WASM).