# smsgg-core

Sega Master System / Game Gear / SG-1000 emulation core.

The Sega Master System contains the following components:
* Z80 CPU clocked at 3.58 MHz
* VDP (video display processor) that is derived from the Texas Instruments TMS9918 but sports an additional advanced graphics mode (which nearly every game on the console uses)
  * Renders at 256x192
  * Supports 6-bit RGB color with up to 32 colors onscreen simultaneously
  * Contains a single background layer
  * Supports up to 64 sprites per frame and up to 8 sprites per scanline, with sprites being 8x8 pixels
* SN76489, a PSG (programmable sound generator)
  * Contains 3 square wave generators and a noise generator
* 8KB of working RAM
* 16KB of VRAM
* 32 bytes of CRAM (color RAM for storing palettes)
* Support for an optional FM sound unit expansion, which adds a YM2413 FM synthesis chip

The Game Gear is nearly identical to the Master System hardware-wise, with a few key differences:
* The VDP still renders 256x192 frames, but only the center 160x144 pixels are displayed
* The VDP color format is changed from 6-bit RGB to 12-bit RGB, and color RAM is doubled in size to accommodate this
* The Start/Pause button flips a bit in a register instead of generating an NMI
* There is a new stereo sound control register that enables hard panning each of the 4 audio channels

SG-1000 is the Master System's predecessor, and it differs in the following ways:
* 1KB of working RAM instead of 8KB
* Video processor is an actual TMS9918A, not Sega's customized version used in SMS/GG (and a later version in Genesis)
* SN76489's noise channel has slightly different behavior in white noise mode, since it's a stock SN76489 rather than a Sega-customized chip
* Cartridge memory mapping is much simpler; no games used memory-banking mappers

This crate contains code for:
* SMS/GG VDP
* SMS/GG PSG
* SMS/GG memory map
* SMS FM sound unit expansion
* SMS/GG main loop
