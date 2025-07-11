# jgenesis

Cross-platform multi-console emulator supporting a number of 8-bit and 16-bit gaming consoles.

Linux, Windows, and web (WASM) are officially supported. Other platforms may work but have not been tested.

## Features

* Emulation for the following consoles:
  * Sega Genesis / Mega Drive
  * Sega CD / Mega CD
  * Sega 32X / Mega 32X
    * This core is currently not well-optimized - full-speed 32X emulation requires a CPU with decent single-core performance
  * Sega Master System / Mark III
  * Game Gear
  * Nintendo Entertainment System (NES) / Famicom
  * Super Nintendo Entertainment System (SNES) / Super Famicom
  * Game Boy / Game Boy Color
* GPU-based renderer with integer prescaling and optional linear interpolation
* Configurable pixel aspect ratio for each console with several different options: accurate to original hardware/TVs, square pixels, and stretched to fill the window
* Support for the Sega Master System FM sound unit expansion
* Support for the Sega Genesis SVP chip, used in _Virtua Racing_
* Support for the most common NES mappers, plus a number of less common mappers
* Support for most SNES coprocessors (e.g. Super FX, SA-1, DSP-1, CX4, S-DD1, SPC7110)
* Support for both 3-button and 6-button Genesis controllers
* Support for keyboard controls and DirectInput gamepad controls
* Save states, fast forward, and rewind
* Some simple horizontal blur and naive anti-dither shaders for blending dithered pixel patterns, which were extremely common on these consoles due to limited color palettes and lack of hardware-supported transparency
* Optional 2x CPU overclocking for Sega Master System and Game Gear emulation
* Optional 2-4x GSU overclocking for SNES Super FX games
* Can run the Titan Overdrive and Titan Overdrive 2 demos for the Mega Drive

TODOs:
* Support multiple Sega CD BIOS versions in GUI and automatically use the correct one based on disc region

## Dev Builds

The GitHub Actions workflow compiles release builds on every commit, which will stay available for 90 days afterwards:
https://github.com/jsgroth/jgenesis/actions

Note that these builds are not always well-tested and may have bugs.

## Build & Run

See [BUILD.md](BUILD.md)

## Screenshots

![Screenshot from 2023-08-27 22-47-13](https://github.com/jsgroth/jgenesis/assets/1137683/d2ec2bc6-de7d-4ff1-98c5-10a0c4db7391)

![Screenshot from 2023-08-27 22-53-09](https://github.com/jsgroth/jgenesis/assets/1137683/05a7c309-0706-4627-9b45-313f259cc494)

![Screenshot from 2023-09-27 19-36-19](https://github.com/jsgroth/jgenesis/assets/1137683/2684be78-c2db-4af3-81dc-4325eb25f440)

![Screenshot from 2023-09-29 17-12-35](https://github.com/jsgroth/jgenesis/assets/1137683/69ab2eb5-1a5f-42e3-abac-c660b5c359e7)

![Screenshot from 2023-11-06 21-42-49](https://github.com/jsgroth/jgenesis/assets/1137683/437bd22f-f1ec-43a2-9340-62c042d489de)

![Screenshot from 2023-08-27 22-45-06](https://github.com/jsgroth/jgenesis/assets/1137683/7d1567ce-39ba-4645-9aff-3c6d6e0afb80)

![Screenshot from 2023-08-27 22-45-32](https://github.com/jsgroth/jgenesis/assets/1137683/90d96e18-57a8-4327-8d9d-385f55a718b3)

![Screenshot from 2023-09-18 15-44-28](https://github.com/jsgroth/jgenesis/assets/1137683/d70b708c-c1dc-4a9e-adda-11d2b1b8fa00)

## Sources

### Sega Master System / Game Gear
* Z80 User Manual: https://map.grauw.nl/resources/cpu/z80.pdf
* The Undocumented Z80 Documented: http://www.myquest.nl/z80undocumented/z80-documented-v0.91.pdf
* Sega Master System architecture: https://www.copetti.org/writings/consoles/master-system/
* Sega Master System / Game Gear documentation: https://www.smspower.org/Development/Documents
* YM2413 application manual: https://www.smspower.org/maxim/Documents/YM2413ApplicationManual
* Reverse engineering of the YM2413: https://github.com/andete/ym2413

### Sega Genesis / Mega Drive
* M68000 Family Programmer's Reference Manual: https://www.nxp.com/docs/en/reference-manual/M68000PRM.pdf
* Motorola 68000 Opcodes: http://goldencrystal.free.fr/M68kOpcodes.pdf
* Mega Drive / Genesis architecture: https://www.copetti.org/writings/consoles/mega-drive-genesis/
* Mega Drive official documentation: https://segaretro.org/Mega_Drive_official_documentation
* Sega Genesis hardware notes by Charles MacDonald: https://gendev.spritesmind.net/mirrors/cmd/gen-hw.txt
* Aggregating Community Research: https://gendev.spritesmind.net/forum/viewtopic.php?f=2&t=2227
* Mega Drive video timings: https://gendev.spritesmind.net/forum/viewtopic.php?f=22&t=519
* Genesis ROM header reference: https://plutiedev.com/rom-header
* Genesis - Going beyond 4MB: https://plutiedev.com/beyond-4mb
* SEGA Mega Drive / Genesis hardware notes by Kabuto: https://plutiedev.com/mirror/kabuto-hardware-notes
* Huge thread discussing and detailing the YM2612: https://gendev.spritesmind.net/forum/viewtopic.php?f=24&t=386
* Genesis EEPROM games: https://gendev.spritesmind.net/forum/viewtopic.php?f=25&t=206
* SVP documentation by notaz, as well as earlier documentation work by Tasco Deluxe: https://notaz.gp2x.de/docs/svpdoc.txt

### Sega CD / Mega CD
* Mega CD official documentation: https://segaretro.org/Mega-CD_official_documentation
* ECMA-130 standard: https://www.ecma-international.org/publications-and-standards/standards/ecma-130/
* Thread discussing details of Mega CD emulation: https://gendev.spritesmind.net/forum/viewtopic.php?t=3020

### 32X
* 32X official documentation: https://segaretro.org/32X_official_documentation
* SH-1/SH-2 programming manual: https://antime.kapsi.fi/sega/files/h12p0.pdf
* SH7604 hardware manual: https://segaretro.org/images/2/2c/SH7604_Hardware_Manual.pdf

### NES
* 6502 Instruction Set: https://www.masswerk.at/6502/6502_instruction_set.html
* 6502 Hardware Manual: https://web.archive.org/web/20120227142944if_/http://archive.6502.org:80/datasheets/synertek_hardware_manual.pdf
* Documentation for the NMOS 65xx/85xx Instruction Set: https://www.nesdev.org/6502_cpu.txt
* Nintendo Entertainment System (NES) architecture: https://www.copetti.org/writings/consoles/nes/
* NESDev NES reference guide: https://www.nesdev.org/wiki/NES_reference_guide

### SNES
* A 65816 Primer: https://softpixel.com/~cwright/sianse/docs/65816NFO.HTM
* Super Nintendo architecture: https://www.copetti.org/writings/consoles/super-nintendo/
* fullsnes - nocash SNES hardware specifications: https://problemkaputt.de/fullsnes.htm
* Anomie's SNES documents: https://www.romhacking.net/?page=documents&category=&platform=&game=&author=&perpage=20&level=&title=anomie&docsearch=Go
* SFC Development Wiki: https://wiki.superfamicom.org/

### Game Boy / Game Boy Color
* Game Boy CPU (SM83) Instruction Set: https://gbdev.io/gb-opcodes/optables/
* Pan Docs: https://gbdev.io/pandocs/
* Game Boy Complete Technical Reference: https://gekkio.fi/files/gb-docs/gbctr.pdf
* Gameboy Sound Hardware: https://gbdev.gg8.se/wiki/articles/Gameboy_sound_hardware
* The Cycle-Accurate Game Boy Docs: https://github.com/geaz/emu-gameboy/blob/master/docs/The%20Cycle-Accurate%20Game%20Boy%20Docs.pdf
  * This contains some outdated/incorrect information but is still useful
