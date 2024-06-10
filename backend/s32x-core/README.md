# s32x-core

Emulation core for the Sega 32X, the second and final add-on for the Sega Genesis.

The 32X plugs into the Genesis cartridge slot and has its own cartridge connector on top. It also does video passthrough:
Genesis video output is passed through the 32X which then outputs the combined Genesis/32X video to the display.

Sega CD games can also make use of the 32X hardware if it is plugged in - games that support this were marketed as
"Sega CD 32X" games. This was mainly (only?) used by FMV games, which use the 32X VDP to display frames in higher color
depth and resolution. This functionality is not currently emulated.

The 32X contains the following hardware:
* A pair of Hitachi SH-2 CPUs clocked at ~23.01 MHz, exactly 3 times the Genesis 68000 clock speed
  * Each CPU has 4KB of internal RAM that can be used as either 4KB cache or 2KB cache + 2KB fast RAM
  * Each CPU includes a 2-channel DMA controller, two timers, a division unit, and a serial interface
    * DMA channels can be used to transfer data within 32X memory, to transfer data from the Genesis, and to transfer data to the PWM sound chip
    * Serial interface can be used to communicate between the two CPUs
  * The two CPUs share the bus in a master/slave configuration; the slave CPU will temporarily stall if they access the bus simultaneously
* VDP (Video Display Processor)
  * Displays pixels from a frame buffer; contains no drawing hardware
  * Contains 256KB of frame buffer RAM, split into two 128KB frame buffers
    * Software can freely update one frame buffer while the VDP is displaying from the other frame buffer
  * Contains 512 bytes of CRAM / palette RAM for storing the color palette used by the 256-color modes
  * Frame buffer resolution is either 320x224 or 320x240 (PAL only)
  * Supports 15-bit RGB555 color, 32,768 different colors
  * Three different frame buffer modes
    * Direct color (32768-color): One pixel per 16-bit word, each pixel contains an RGB555 color
      * Frame buffers do not have enough space for 320x224 frames in direct color mode; some lines must be duplicated
    * Packed pixel (256-color): Two pixels per 16-bit word, each pixel contains an 8-bit index into 32X CRAM
    * Run length (256-color compressed): Each 16-bit word contains an 8-bit index into 32X CRAM and the number of pixels to render in that color (1-256)
  * Can composite pixels between Genesis VDP frames and 32X VDP frames
    * Supports priority compositing only, no color blending
* 2-channel PWM sound chip with a configurable sample rate ranging from ~5.62 KHz to the 32X system clock rate of ~23.01 MHz
  * The sound chip has no attached RAM, only two 3-word FIFOs for upcoming pulse width samples; one of the CPUs must continuously send samples to the FIFOs
  * In practice, most games set the sample rate to 22 KHz and nothing sets it higher than 44.1 KHz
* 256KB of SDRAM shared between the two SH-2s