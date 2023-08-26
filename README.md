# jgenesis

WIP multi-console Sega emulator. Currently mostly supports the Sega Master System, the Game Gear, and the Sega Genesis.

Major TODOs:
* Implement a unified frontend with GPU rendering and display configuration
* Implement a few remaining Genesis VDP features (shadow/highlight bit, sprite overflow & collision flags)
* Implement a few remaining YM2612 features (CSM and SSG-EG, they're obscure but some games did use them)
  * Volume levels also sound off in some games
* Halt the 68000 for the appropriate amount of time whenever a memory-to-VRAM DMA runs; not doing this causes graphical glitches in some games
* Support PAL for Genesis
* Support 6-button Genesis controllers
* Support the SMS optional YM2413 FM sound chip
* Support for specific Genesis games that do weird things with cartridge hardware (e.g. Phantasy Star 4 and Super Street Fighter 2)
* Support player 2 inputs
* Support the SMS reset button
* Support persistent save files for Genesis games with persistent cartridge RAM
