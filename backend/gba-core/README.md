# gba-core

Emulation core for the Game Boy Advance.

This system contains the following components:
* ARM7TDMI CPU clocked at ~16.77 MHz
  * Implements the 32-bit ARMv4T instruction set (ARMv4 + Thumb)
* PPU (picture processing unit)
  * 240x160 LCD display
  * RGB555 color with up to 512 colors onscreen simultaneously (256 BG + 256 sprite)
  * 4 background layers, 2 of which support rotation/scaling
  * Up to 128 sprites per frame, with sprites ranging in size from 8x8 pixels to 64x64
  * Supports sprite rotation/scaling
  * Bitmap graphics modes that display pixels directly from a frame buffer in VRAM
  * Supports a mosaic filter for both backgrounds and sprites
  * Supports alpha blending between different layers
  * 96 KB of VRAM for storing tile data, tile maps, and bitmap mode frame buffers
  * 1 KB of palette RAM
  * 1 KB of OAM (object attribute memory) for storing the sprite table
* APU (audio processing unit)
  * 1-bit PWM output at ~16.77 MHz
  * PWM sample resolution ranging from 9-bit samples at 32768 Hz to 6-bit samples at 262144 Hz (most games use 8-bit at 65536 Hz)
  * Two 8-bit PCM channels (Direct Sound)
  * Four PSG channels, which are slightly modified versions of the four Game Boy Color APU channels
    * Two pulse generators, one with frequency sweep support
    * Custom wave channel that plays 4-bit PCM samples in a loop of either 32 samples or 64 samples
    * Pseudorandom noise generator
* 288 KB of working RAM, split into 32 KB of fast RAM (IWRAM) and 256 KB of slow RAM (EWRAM)
* 4 hardware timers driven by the CPU clock, with configurable prescalers and intervals
  * 2 of these timers can be used to control the sample rate of the APU's Direct Sound channels
* 4 DMA channels
* Game Pak prefetch hardware to speed up code executing out of cartridge ROM
* Support for cartridges with up to 32 MB of ROM
* 16 KB builtin BIOS ROM
