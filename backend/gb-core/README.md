# gb-core

Emulation core for the Game Boy and Game Boy Color.

These systems contain the following components:

* Sharp SM83 CPU clocked at 4.194304 MHz (GB) or 8.388608 MHz (GBC in double speed mode)
  * SM83 is kind of like a Z80-lite
* PPU (picture processing unit)
  * Renders at 160x144 directly onto the Game Boy's LCD screen
  * Supports 4 different colors (GB) or 32,768 different colors (GBC)
  * GB supports a single 4-color background palette and 2 different 3-color sprite palettes
  * GBC supports 8 different 4-color background palettes and 8 different 3-color sprite palettes
  * Supports a scrollable background layer as well as a window layer that can replace the background in a configurable region of the screen
  * Supports up to 40 sprites per frame and up to 10 sprites per scanline, with sprites being 8x8 pixels
  * Supports HBlank and scanline interrupts and has a CPU-readable scanline counter, unlike the NES
* APU (audio processing unit)
  * Contains 4 audio channels: 2 pulse wave generators, a pseudo-random noise generator, and a custom wave channel that plays from dedicated waveform RAM (16 bytes)
  * Supports monoaural sound through the built-in speaker and stereo sound through headphones
* 8KB of working RAM (GB) or 32KB (GBC)
* 8KB of VRAM (GB) or 16KB (GBC)
* 160 bytes of OAM (object attribute memory)
* 127 bytes of "HRAM", a small section of working RAM on a separate bus from the rest of RAM (often used during OAM DMAs)