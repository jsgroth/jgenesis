# snes-core

Emulation core for the Super Nintendo Entertainment System (SNES) / Super Famicom.

This system contains the following components:
* 65C816 CPU clocked at 3.58 MHz
  * The SNES augments this CPU with some additional components, including multiplication and division units
* Technically two PPUs (picture processing units), though they function as a single unit
  * Can render in 256x224, 256x239, 512x224, 512x239, 512x448, or 512x478
    * 512px resolutions were not commonly used due to memory usage and the quality of 90s TV setups
    * 512x448 and 512x478 only possible using interlaced rendering modes
  * Supports 15-bit RGB color (32,768 possible colors) with up to 256 different colors onscreen simultaneously
  * Supports 4 different background layers, though most BG modes only use 2 or 3
    * Each BG layer can be 4-color, 16-color, or 256-color depending on BG mode
  * Supports 8 different background modes
    * One mode supports background rotation and scaling via affine transformations (Mode 7)
    * Two modes support 512px high-resolution backgrounds (Modes 5 & 6)
  * Supports 128 sprites onscreen simultaneously and up to 32 sprites per line, with sprites ranging in size from 8x8 to 64x64
  * Supports layer blending via "color math", commonly used for transparency and lighting effects
* APU (audio processing unit)
  * Contains an embedded SPC700 CPU clocked at 1.024 MHz, used to drive the DSP
  * The embedded DSP is an 8-channel ADPCM playback device with per-channel envelopes, a noise generator, pitch modulation, and an echo filter
  * Emits an audio signal at 32 KHz
* 128KB of working RAM
* 64KB of VRAM
* 512+32 bytes of OAM (object/sprite attribute memory)
* 512 bytes of CGRAM (color palette memory)
* 64KB of audio RAM shared between the SPC700 and the audio DSP

The SNES may have had a fairly awful CPU compared to its most significant competition, but it made up for it with its extremely capable graphics processor.