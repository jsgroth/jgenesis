# jgenesis

Cross-platform multi-console Sega emulator that supports the Sega Genesis / Mega Drive, the Sega Master System / Mark III, and the Game Gear.

Major TODOs:
* Implement a few remaining YM2612 features (CSM and SSG-EG, they're obscure but some games did use them)
* Support the SMS optional YM2413 FM sound chip

Minor TODOs:
* Emulate the Genesis VDP FIFO, in particular the fact that the CPU stalls if it performs VRAM writes too rapidly during active display. A few games depend on this to function correctly (e.g. _The Chaos Engine_, _Double Clutch_, _Sol-Deace_), and a few other games have graphical glitches if it's not emulated (e.g. the EA logo flickering for a single frame)
* Support 24C64 EEPROM chips (used only in _Frank Thomas Big Hurt Baseball_ and _College Slam_)

## Dependencies

### Rust

This project requires the latest stable version of the [Rust toolchain](https://doc.rust-lang.org/book/ch01-01-installation.html) to build.

### SDL2

This project requires [SDL2](https://www.libsdl.org/) core headers to build.

Linux (Debian-based):
```
sudo apt install libsdl2-dev
```

macOS:
```
brew install sdl2
```

### GTK3 (Linux GUI only)

On Linux only, the GUI requires [GTK3](https://www.gtk.org/) headers to build.

Linux (Debian-based):
```
sudo apt install libgtk-3-dev
```

## Build & Run

CLI:
```
cargo run --release --bin jgenesis-cli -- -f <path_to_rom_file>
```

To view all CLI args:
```
cargo run --release --bin jgenesis-cli -- -h
```

GUI:
```
cargo run --release --bin jgenesis-gui
```

To build with maximum optimizations (better runtime performance + smaller binary size at the cost of long compile time):
```
RUSTFLAGS="-Ctarget-cpu=native" cargo build --profile release-lto
```
...After which the executables will be in `target/release-lto/`.

## Screenshots

![Screenshot from 2023-08-27 22-45-06](https://github.com/jsgroth/jgenesis/assets/1137683/7d1567ce-39ba-4645-9aff-3c6d6e0afb80)

![Screenshot from 2023-08-27 22-45-32](https://github.com/jsgroth/jgenesis/assets/1137683/90d96e18-57a8-4327-8d9d-385f55a718b3)

![Screenshot from 2023-08-27 22-47-13](https://github.com/jsgroth/jgenesis/assets/1137683/d2ec2bc6-de7d-4ff1-98c5-10a0c4db7391)

![Screenshot from 2023-08-27 22-53-09](https://github.com/jsgroth/jgenesis/assets/1137683/05a7c309-0706-4627-9b45-313f259cc494)
