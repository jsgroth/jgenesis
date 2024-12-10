# gba-core

Emulation core for the Game Boy Advance.

This system contains the following components:
* ARM7TDMI CPU clocked at ~16.77 MHz
  * 32-bit CPU implementing the ARMv4T instruction set (ARMv4 + Thumb)
* PPU (picture processing unit)
  * Renders to the GBA's 240x160 LCD screen
  * Contains 96KB of VRAM for storing graphics and tile maps
    * Internally split into 64KB of BG VRAM and 32KB of sprite VRAM
  * Supports RGB555 color (32,768 different colors) with up to 512 colors onscreen simultaneously (256 BG colors + 256 sprite colors)
  * Supports up to 4 background layers depending on background mode
  * Supports bitmap graphics modes for rendering high-color static images or software-rendered graphics
  * Supports affine transformations (scaling/rotation) for both background layers and sprites
  * Supports up to 128 sprites onscreen simultaneously, with sprites ranging in size from 8x8 pixels to 64x64 (with a much more flexible sprite size implementation than SNES)
    * Sprite-per-line limit varies based on sprite size and whether affine transformations are used
  * Supports basic additive alpha blending for transparency effects
  * Supports a mosaic filter for both background layers and sprites
* APU (audio processing unit)
  * Contains 6 audio channels: 2 PCM channels and 4 PSG channels
  * The 2 PCM channels ("Direct Sound") play 8-bit PCM samples at a configurable sample rate (controlled by one of the hardware timers)
  * The 4 PSG channels are the same channels as the Game Boy Color APU: 2 pulse wave generators, a noise generator, and a custom wave channel
  * Audio output is 1-bit PWM at ~16.77 MHz, with a configurable sampling cycle ranging from 32768 Hz (9-bit sample depth) to 262144 Hz (6-bit sample depth)
    * Almost all games use a sampling cycle of 65536 Hz (8-bit sample depth) because it generally provides the highest audio quality out of the available options
* 288KB of working RAM, split into 32KB of fast IWRAM (internal working RAM) and 256KB of slower EWRAM (external working RAM)
* 4 hardware timers, each tracking the system clock with a configurable clock divider
* 4 DMA channels: 1 meant for high-priority transfer (e.g. HBlank DMA), 2 meant for sending audio samples to the Direct Sound channels, and 1 meant for general-purpose transfer