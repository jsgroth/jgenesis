# Architecture

## Overview

The crates can be broken up roughly into 5 categories:
* Common libraries: `jgenesis-common`, `jgenesis-proc-macros`, `cdrom`
* CPU emulators: `z80-emu`, `m68000-emu`, `mos6502-emu`, `wdc65816-emu`, `spc700-emu`
* Emulation backend: `smsgg-core`, `genesis-core`, `segacd-core`, `nes-core`, `snes-core`, `snes-coprocessors`, `gb-core`
* Emulation frontend: `jgenesis-renderer`, `jgenesis-native-driver`, `jgenesis-native-config`, `jgenesis-cli`, `jgenesis-gui`, `jgenesis-web`
* CPU emulator test harnesses: `z80-test-runner`, `m68000-test-runner`, `mos6502-test-runner`, `wdc65816-test-runner`, `spc700-test-runner`

Repo structure:
* Common library crates are at the top level of the project
* `cpu/` contains the CPU emulator and test harness crates
* `backend/` contains the emulation backend crates
* `frontend/` contains the emulation frontend crates

The CPU emulators are designed to be usable with any implementation of their respective bus traits. The test harnesses provide a bus implementation that maps every address to RAM (which is what the tests expect), while the various consoles provide implementations that emulate the console's memory map.

For the most part, the backends interact with the frontends through trait implementations. The backends implement traits that enable frontend features including save states and rewind. The frontends provide trait implementations to the backends that enable the backends to display video frames, output audio samples, and persist any save files (e.g. for a cartridge with battery-backed SRAM). The frontends are also responsible for passing current emulated controller state to the backends (i.e. which buttons are currently pressed).

## Crates

### `jgenesis-common`

Contains traits that define the interface between the emulation backends and the emulation frontends, as well as some dependency-light common code that is used across many of the other crates (e.g. helper extension traits and audio resampling).

### `jgenesis-proc-macros`

Custom derive macros used across many of the other crates.

### `cdrom`

Contains code for reading CD-ROM images in CUE/BIN or CHD format.

### `z80-emu`

Instruction-based emulation core for the Zilog Z80 CPU, which is used in the Master System, the Game Gear, and the Genesis.

### `m68000-emu`

Instruction-based emulation core for the Motorola 68000 CPU, which is used in the Genesis and the Sega CD.

### `mos6502-emu`

Cycle-based emulation core for the MOS 6502 CPU. Supports both the stock 6502 and the NES 6502.

### `wdc65816-emu`

Cycle-based emulation core for the WDC 65C816 CPU (aka 65816), which is used in the SNES.

### `spc700-emu`

Cycle-based emulation core for the Sony SPC700 CPU, which is used in the SNES as a dedicated audio processor embedded inside the APU.

### `smsgg-core`

Emulation core for the Sega Master System and Game Gear. The core is shared because there are very few hardware differences between the two.

### `genesis-core`

Emulation core for the Sega Genesis / Mega Drive. Uses the PSG component from `smsgg-core`, since the Genesis reused the Master System PSG as a secondary sound chip.

### `segacd-core`

Emulation core for the Sega CD / Mega CD. Uses many components from `genesis-core`, as the Genesis side of the system is virtually unchanged except for the parts of the memory map that the standalone Genesis maps to the cartridge.

### `nes-core`

Emulation core for the Nintendo Entertainment System (NES) / Famicom.

### `snes-core`

Emulation core for the Super Nintendo Entertainment System (SNES) / Super Famicom. Depends on `snes-coprocessors` to emulate cartridges that contain coprocessors.

### `snes-coprocessors`

Emulation for coprocessors used in SNES cartridges. Coprocessor cartridge implementations expose methods such as reading a memory address, writing a memory address, and ticking the internal processor (for cartridges that contain a CPU or DSP).

### `gb-core`

Emulation core for the Game Boy and Game Boy Color.

### `jgenesis-renderer`

GPU-based implementation of the `Renderer` trait in `jgenesis-traits`, built on top of `wgpu`. Can be used with any window that implements the `raw-window-handle` traits. Exists in its own crate so that it can be used in both the native and web frontends.

### `jgenesis-native-driver`

Native emulation frontend that uses SDL2 for windowing, audio, and input.

### `jgenesis-native-config`

Contains the code representation of the main config file used by the CLI and GUI.

### `jgenesis-cli` / `jgenesis-gui`

CLI and GUI that both invoke `jgenesis-native-driver` to run the emulator. `jgenesis-gui` is built using `egui` and `eframe`.

### `jgenesis-web`

Web emulation frontend that compiles to WASM and runs in a web browser.

### `z80-test-runner`

Test harness to test `z80-emu` against Z80 test suites that were assembled for old PCs, such as ZEXDOC and ZEXALL.

### `m68000-test-runner`

Test harness to test `m68000-emu` against [TomHarte's 68000 test suite](https://github.com/TomHarte/ProcessorTests/tree/main/680x0/68000/v1).

### `mos6502-test-runner`

Test harness to test `mos6502-emu` against [TomHarte's NES 6502 test suite](https://github.com/TomHarte/ProcessorTests/tree/main/nes6502).

### `wdc65816-test-runner`

Test harness to test `wdc65816-emu` against [TomHarte's 65816 test suite](https://github.com/TomHarte/ProcessorTests/tree/main/65816).

### `spc700-test-runner`

Test harness to test `spc700-emu` against [JSON SPC700 test suites](https://github.com/TomHarte/ProcessorTests/tree/main/spc700).
