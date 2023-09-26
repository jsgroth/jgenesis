# genesis-core

Sega Genesis / Mega Drive emulation core.

The Genesis contains the following components, some of which were reused from the Master System:
* 68000 CPU clocked at 7.67 MHz
* Z80 CPU clocked at 3.58 MHz, meant primarily for audio processing
* VDP (video display processor) that is based on the Master System VDP but adds yet another more advanced graphics mode
  * Can render in either 256x224 or 320x224; PAL consoles additionally support 256x240 and 320x240
  * Supports 9-bit RGB color with up to 64 colors onscreen simultaneously
  * Supports shadowing and highlighting to create additional colors beyond the 512 colors of 9-bit RGB
  * Supports two background layers with per-pixel priority
  * Supports per-scanline and per-16px-column background scrolling
  * Supports up to 80 sprites per frame and up to 20 sprites per scanline, with sprites ranging in size from 8x8 to 32x32
* YM2612, an FM synthesis sound chip
  * Contains 6 fully configurable FM synthesis channels, each with 4 operators that can be arranged in 1 of 8 different configurations
  * One of the 6 channels can be swapped out for a raw DAC channel which directly outputs a PCM sample
  * Output frequency of 53.267 KHz
* SN76489, a PSG lifted directly from the Master System
* 64KB of working RAM for the 68000
* 8KB of working/audio RAM for the Z80
* 64KB of VRAM
* 128 bytes of CRAM (color RAM for storing palettes)
* 80 bytes of VSRAM (vertical scroll RAM for storing per-column V-scroll values)

This crate contains code for the following:
* Genesis VDP
* Genesis FM sound chip
* Genesis memory map
* Tying together all of the Genesis components