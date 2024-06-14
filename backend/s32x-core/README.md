# s32x-core

Emulation core for the Sega 32X, the second and final add-on for the Sega Genesis.

The 32X plugs into the Genesis cartridge slot and has its own cartridge connector on top. It also does video passthrough:
Genesis video output is passed through the 32X which then outputs the combined Genesis/32X video to the display.

Sega CD games can also make use of the 32X hardware if it is plugged in - games that support this were marketed as
"Sega CD 32X" games. This was mainly (only?) used by FMV games, which use the 32X VDP to display frames in higher color
depth and resolution. This functionality is not currently emulated.

The 32X contains the following hardware:
* A pair of Hitachi SH-2 CPUs clocked at 23.01 MHz, exactly 3 times the Genesis 68000 clock speed
  * Each CPU has 4KB of internal RAM that can be used as either 4KB cache or 2KB cache + 2KB fast RAM
  * The two CPUs share the bus, and one of them will temporarily stall if they access the bus simultaneously
  * Trivia: These are the exact same CPUs that were later used as the Saturn's main CPUs, though the Saturn runs them slightly faster (about 28.63 MHz)
* VDP (Video Display Processor)
  * Displays pixels from a frame buffer; contains no specialized drawing hardware
  * Contains 256KB of frame buffer RAM, split into two 128KB frame buffers
    * Software can freely update one frame buffer while the VDP is displaying from the other frame buffer
  * Contains 512 bytes of CRAM / palette RAM for storing the color palette used by the 256-color modes
  * Video output is either 320x224 or 320x240 (PAL only)
  * Supports 15-bit RGB555 color, 32,768 different colors
  * Three different frame buffer pixel formats
    * Direct color (32768-color): One pixel per 16-bit word, each pixel contains an RGB555 color
      * Frame buffers do not have enough space for 320x224 frames in direct color format; some lines must be duplicated
    * Packed pixel (256-color): Two pixels per 16-bit word, each pixel contains an 8-bit index into 32X CRAM
    * Run length (256-color compressed): Each 16-bit word contains an 8-bit index into 32X CRAM and the number of pixels to render in that color (1-256)
  * Can composite pixels between Genesis VDP frames and 32X VDP frames
    * Supports priority compositing only, no blending
* 2-channel PWM sound chip
* 256KB of SDRAM shared between the two SH-2s