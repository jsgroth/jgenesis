# jgenesis

Cross-platform multi-console Sega emulator that supports the Sega Genesis / Mega Drive, the Sega CD / Mega CD, the Sega Master System, and the Game Gear.

## Features

* Emulation for the following consoles:
  * Sega Genesis / Mega Drive
  * Sega CD / Mega CD
    * The JP Model 1 BIOS does not currently work
  * Sega Master System / Mark III
  * Game Gear
* GPU-based renderer with integer prescaling and optional linear interpolation
* Configurable pixel aspect ratio for each console with several different options: accurate to original hardware/TVs, square pixels, and stretched to fill the window
* Support for the Sega Master System FM sound unit expansion
* Support for both 3-button and 6-button Genesis controllers
* Support for keyboard controls and DirectInput gamepad controls
* Save states, fast forward, and rewind
* Some simple horizontal blur and naive anti-dither shaders for blending dithered pixel patterns, which were extremely common on these consoles due to limited color palettes and lack of hardware-supported transparency
* Optional 2x CPU overclocking for Sega Master System and Game Gear emulation

Major TODOs:
* Implement a few remaining YM2612 features (CSM and SSG-EG, they're obscure but some games did use them)
* Fix remaining major Sega CD emulation bugs
  * JP V1.00 BIOS freezes after loading the music player menu, which in this BIOS version is required to boot any games

Minor TODOs:
* Emulate the Genesis VDP FIFO, in particular the fact that the CPU stalls if it writes to VRAM too rapidly during active display. A few games depend on this to function correctly (e.g. _The Chaos Engine_, _Double Clutch_, _Sol-Deace_), and a few other games have graphical glitches if it's not emulated (e.g. the EA logo flickering for a single frame, parts of the _Batman Returns_ intro running too fast)
* Support the Sega Virtua Processor (SVP) chip; used only by _Virtua Racing_
* Support 24C64 EEPROM chips (used only in _Frank Thomas Big Hurt Baseball_ and _College Slam_)
* Support the Sega Master System's additional graphics modes (Modes 0-3); only one officially released game used any of them, _F-16 Fighter_ (which uses Mode 2)
* Support the Sega CD's 128KB backup RAM cartridge; _Shining Force CD_ in particular benefits from this
* Support multiple Sega CD BIOS versions in GUI and automatically use the correct one based on disc region
* Support CHD files for Sega CD in addition to BIN/CUE

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

![Screenshot from 2023-09-27 19-36-19](https://github.com/jsgroth/jgenesis/assets/1137683/2684be78-c2db-4af3-81dc-4325eb25f440)

![Screenshot from 2023-09-29 17-12-35](https://github.com/jsgroth/jgenesis/assets/1137683/69ab2eb5-1a5f-42e3-abac-c660b5c359e7)

![Screenshot from 2023-09-18 15-44-28](https://github.com/jsgroth/jgenesis/assets/1137683/d70b708c-c1dc-4a9e-adda-11d2b1b8fa00)

## Sources

* Mega Drive official documentation: https://segaretro.org/Mega_Drive_official_documentation
* Mega Drive / Genesis architecture: https://www.copetti.org/writings/consoles/mega-drive-genesis/
* Sega Master System / Game Gear documentation: https://www.smspower.org/Development/Documents
* Sega Master System architecture: https://www.copetti.org/writings/consoles/master-system/
* Sega Genesis hardware notes by Charles MacDonald: https://gendev.spritesmind.net/mirrors/cmd/gen-hw.txt
* Mega Drive video timings: https://gendev.spritesmind.net/forum/viewtopic.php?f=22&t=519
* Genesis ROM header reference: https://plutiedev.com/rom-header
* Genesis - Going beyond 4MB: https://plutiedev.com/beyond-4mb
* Huge thread discussing and detailing the YM2612: https://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386
* Genesis EEPROM games: https://gendev.spritesmind.net/forum/viewtopic.php?f=25&t=206
* YM2413 application manual: https://www.smspower.org/maxim/Documents/YM2413ApplicationManual
* Reverse engineering of the YM2413: https://github.com/andete/ym2413
* Mega CD official documentation: https://segaretro.org/Mega-CD_official_documentation
* ECMA-130 standard: https://www.ecma-international.org/publications-and-standards/standards/ecma-130/
* Thread discussing details of Mega CD emulation: https://gendev.spritesmind.net/forum/viewtopic.php?t=3020
