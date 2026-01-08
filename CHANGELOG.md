# 0.11.0

## Game Boy Advance
* Game Boy Advance emulation is now supported; requires a Game Boy Advance BIOS ROM
* Video options for color correction and LCD ghosting emulation (frame blending), both enabled by default
* Audio enhancement option to apply enhanced interpolation to the Direct Sound channels rather than accurately emulating actual hardware's resampling behavior
  * This massively reduces audio aliasing and noise in most cases, but it can also make audio sound muffled when games use very low sample rates (as most GBA games do)
* CPU/prefetch/DMA/etc. timing is not perfect but should be moderately accurate
* All types of cartridge save memory supported, with an option to force a specific type if auto-detection gets it wrong
* The cartridge RTC chip and solar sensor are emulated for the handful of games that use them (e.g. _Boktai_), other cartridge peripherals are not currently emulated

## New Features
* (**Genesis**) Added an option to ignore configured aspect ratio and display square pixels when the VDP is in H40/H320px mode (#442)
* (**Genesis**) Added support for games with 24C64 EEPROM chips; _College Slam_ and _Frank Thomas Big Hurt Baseball_ are now playable (#459)
* (**Genesis**) New audio option to enable/disable individual YM2612 audio channels
* (**Genesis**) New audio option to adjust volume of individual sound sources
* (**Genesis**) Added some additional memory/register views to the Memory Viewer window
* (**Sega CD**) Added support for per-region BIOS configuration, where the emulator will automatically select the appropriate BIOS based on game region
* (**32X**) Added two new color display options (#492)
  * New option to darken Genesis colors relative to 32X colors, enabled by default; on actual hardware the brightest 32X colors are noticeably brighter than the brightest Genesis colors
  * New option to apply a yellow or purple tint to 32X colors, roughly as observed on actual hardware
* (**32X**) Added an option to overclock the SH-2s; this is extremely CPU-intensive but can reduce slowdown in some games
* (**32X**) Added options to hide all pixels of a given priority (#494)
* (**NES**) Added palette customization options (#424)
  * Can now load a custom palette from a file; both 64-color and full 512-color palette files supported
  * GUI now has a builtin NTSC palette generator with a graphic displaying the current palette
* (**SNES**) Added support for the ST018 coprocessor, used by _Hayazashi Nidan Morita Shougi 2_
  * This is emulated using an ARM7TDMI implementation (ARMv4T) rather than an ARM6 (ARMv3), but this should not make any functional difference
* (**GB**) Added a frame blending option that simulates LCD ghosting, enabled by default; enabling this fixes graphical effects in a few games and demos (#469)
* (**GB**) When color correction is enabled, you can now manually adjust the LCD gamma value used for gamma correction
* (**GB**) Added support for booting from a boot ROM (#404)
* (**GB**) Added an option to run original Game Boy software in Game Boy Color mode (#150)
  * This feature requires a GBC boot ROM in order to initialize the compatibility palettes, and it is off by default because it (accurately) causes major bugs in a few games
* (**GB**) Added (proper) support for the MBC30 mapper, used by the Japanese version of _Pocket Monsters Crystal Version_ (#478)
* (**GB**) Added support for unlicensed/homebrew games with an SRAM size byte of \$01 (2 KB) (#485)
* (**Game Gear**) Added support for booting from a BIOS / boot ROM (#404)
* Added turbo button support for face and shoulder buttons on all emulated systems

## Improvements
* Upgraded [SDL](https://www.libsdl.org/) from SDL2 to SDL3; for the most part this should hopefully have no noticeable impact except for audio playback maybe working a little better than before
  * As part of this, I removed the borderless vs. exclusive fullscreen setting because this seemed much less straightforward to change in SDL3; fullscreen will now always use whatever the platform and graphics driver default to (probably always borderless on modern platforms)
* The auto-prescale video setting can now use separate scale factors for width and height, which produces a less aliased image when games use display modes with sub-1 pixel aspect ratio (e.g. Genesis NTSC H40/H320px mode, SNES high-res modes)
  * Auto-prescale is also now enabled by default (the previous default was a fixed 3x upscale factor)
* (**SNES**) When using the Super Scope, the emulator now displays the new Super Scope Turbo state whenever you toggle Turbo on/off

## General Fixes
* Fixed a number of cases that would cause the emulator to crash, none triggered by official releases (as far as I know) but some triggered by homebrew/demos/test ROMs
  * (**Genesis**) Loading a ROM file smaller than 1 KB or so (#434)
  * (**Genesis**) Z80 tries to access its own memory through the 32KB 68K memory bank by mapping it to \$A00000-\$A07FFF or \$A08000-\$A0FFFF
  * (**Genesis**) VRAM-to-VRAM copy DMA with source address higher than \$1FFFF
  * (**Genesis**) 68000 triggers a privilege violation exception
  * (**Genesis**) 68000 triggers an address error while handling an exception
  * (**32X**) One of the SH-2s executes a SLEEP instruction (#431)
  * (**32X**) One of the SH-2s executes an illegal opcode
  * (**32X**) One of the SH-2s tries to access certain invalid memory addresses (highest 3 bits set to 100 or 101)
  * (**32X**) 68000 performs a 16-bit read from certain invalid memory addresses
  * (**SMS** / **Game Gear** / **Genesis**) Z80 handles an IRQ while in interrupt mode 2
  * (**SNES**) Loading a ROM file smaller than 32 KB (#538)
  * (**SNES**) Loading an SA-1 cartridge where the cartridge header claims it does not have any BW-RAM (anything that does this now gets 64 KB of BW-RAM)
  * (**SNES**) SA-1 Character Conversion DMA Type 1 with an I-RAM destination address higher than \$7FF
  * (**GB**) CPU executes a STOP instruction with no CGB speed switch armed (#465)

## Genesis / Sega CD / 32X Fixes
* Fixed the VDP rendering too many pixels of the previous color when a game makes a mid-scanline backdrop color change while display is disabled; this improves accuracy of the glitchy lines in _Gouketsuji Ichizoku_ / _Power Instinct_ (#462)
* Fixed the 6-button controller timeout period being a little too short (#445)
* Fixed two games failing to boot due to the emulator not giving them the correct type of cartridge save memory
  * _Honoo no Toukyuuji: Dodge Danpei_ has a 24C01 EEPROM chip (#460)
  * _Al Michaels Announces HardBall III_ has 32 KB of SRAM (#546)
* Slightly adjusted memory refresh delay timing when the main CPU is executing out of RAM; this fixes incorrectly visible CRAM dots in _Snatcher_
* (**Sega CD**) Fixed implementation of VDP DMA reads from word RAM being delayed; this fixes glitchy background graphics in _Snatcher_ (again)
  * This was a regression that affected v0.10.0 and v0.10.1; it was broken by VDP DMA changes in v0.10.0
* (**Sega CD** / **32X**) Adjusted inter-CPU/VDP timings to account for some VDP timing changes made in v0.10.0; this fixes the Sega CD version of _Mickey Mania_ having glitchy graphics on the right side of the screen in the 3D chase stages (#491)
  * This was also a regression that affected v0.10.0 and v0.10.1
* (**32X**) Improved accuracy of VDP auto fill timing; this fixes occasional major graphical glitches in _Shadow Squadron_ / _Stellar Assault_ during the intro and takeoff sequences (#225 / #439)
* (**32X**) Fixed some inaccuracies around VDP register latching; this fixes some early 32X demos not working properly (#430)
* (**32X**) Fixed some PWM resampling bugs that sometimes caused erroneous pops in PWM audio output
* (**32X**) Slightly improved SH-2 timing accuracy, particularly around 32-bit SDRAM writes being way too slow (#493)
* (**32X**) Fixed 32X HINT counter behavior not matching Genesis HINT behavior, which caused SH-2 horizontal interrupts to be off by a line (#559)
* (**32X**) Fixed 32X VDP per-line events (HBlank, line render) triggering slightly too early in the line
* Several timing fixes that fix graphical bugs in the Chaekopon demo by Limp Ninja (#435)
  * Adjusted low-level sprite processing timings so that sprite attribute fetching is performed slightly earlier in the line
  * Fixed a pretty egregious DMA timing bug where DMA would take way too long to start copying if initiated during active display with display enabled
* Fixed a number of inaccuracies identified by the testpico test ROM (#482)
  * Fixed implementation of how Genesis VINT and HINT are delayed by 1 instruction when re-enabled; the previous implementation did the wrong thing when two consecutive MOVE instructions enabled then disabled VDP interrupts
  * Fixed the Genesis VDP's F flag starting to read 1 (VINT pending) slightly later than it's supposed to
  * (**32X**) Fixed behaviors related to enabling/disabling/clearing SH-2 VINT during VBlank
  * (**32X**) Fixed some register bits being incorrectly writable / not writable
  * (**32X**) Updates to initial state at power-on to more closely match actual hardware

## NES Fixes
* Very slightly adjusted sprite 0 hit timing; this fixes glitchy lines on the title screen of _Indiana Jones and the Last Crusade_ (#314)
* DMC DMA timing is now emulated; this fixes screen jitter in _Ultimate Air Combat_ (#268)
* Fixed non-power-of-two CHR ROM sizes not working correctly (#573)
* Several fixes for inaccuracies identified by the AccuracyCoin test ROM (#551)
  * Fixed an APU timing issue that sometimes caused an incorrectly skipped frame counter clock when software wrote 0x80 to \$4017 with two consecutive `sta $4017` instructions
  * More accurate APU frame counter timing
  * More accurate OAM DMA timing
  * DMA dummy reads are now emulated (with a new option to disable dummy controller port reads because they cause input glitches in some games)
  * CPU open bus is now properly emulated (it was previously faked by always returning the address high byte)
  * More accurate PPU open bus emulation
  * Most PPU registers are no longer writable immediately after reset
  * Fixed the 6502 incorrectly polling the interrupt lines during the final cycle of BRK instructions and IRQ handling
  * More accurate controller port behaviors around strobing and consecutive-cycle reads

## SNES Fixes
* Fixed BG2 incorrectly not rendering in Mode 7 if BG1 is disabled and BG2+EXTBG are enabled; this fixes missing background graphics in _F-1 Grand Prix_ (#529)
* Fixed V IRQs incorrectly not triggering when a game either 1. changes VTIME to the current line while V IRQs are enabled, or 2. enables V IRQs while VTIME is set to the current line; this fixes graphical corruption in _F-1 Grand Prix_, _F-1 Grand Prix Part III_, _S.O.S.: Sink or Swim_, and _RoboCop vs. The Terminator_ (#529 / #530 / #543)
* Fixed multiple bugs in the BG vertical mosaic implementation, which was previously quite wrong; this fixes graphical bugs in _Beavis and Butt-Head_ and _Jurassic Park Part 2: The Chaos Continues_ (#532 / #540)
* Improved DMA timing accuracy; this fixes graphical glitches in _Circuit USA_ (#531)
* Fixed incorrect DSP-1 port address mapping for DSP-1 cartridges with more than 1 MB of ROM; this fixes _Super Bases Loaded 2_ failing to boot (#534)
* Fixed some revisions of _Tintin in Tibet_ incorrectly defaulting to NTSC timings instead of PAL (#539)
* The 65816 stack pointer is now initialized to \$01FF instead of \$0100; this fixes some homebrew games failing to boot due to stack corruption (#468)
* Fixed SA-1 Character Conversion DMA Type 1 in 2bpp or 4bpp mode incorrectly treating the input packed pixel data as big-endian within a byte when it is actually little-endian; this fixes graphical glitches in the SA-1 tech demo cartridge (#537)
* Implemented multiplication/division timing; this fixes some old homebrew that accidentally depends on reading intermediate division results (#576)

## Game Boy [Color] Fixes
* Fixed the GBC color correction implementation nonsensically performing calculations in sRGB color space rather than linear color space; this makes a particularly huge difference for the GBA LCD option
* Slightly adjusted timing of when the STAT LY=LYC bit reads 1; this fixes a glitchy line in _Elevator Action_ (#472)
* The serial port transfer data register (SB / \$FF01) is now read/write; this fixes _Card Game_ not allowing you to start the game (#471)
* Fixed sample output behavior of pulse and wavetable channels when the DAC is enabled but the channel is inactive
* Added approximate emulation of DAC fading when a channel's DAC is turned on or off; combined with the above change, this fixes buzzing noises in _Cannon Fodder_, _3D Pocket Pool_, and others (#475)
* Fixed VRAM DMA incorrectly terminating prematurely when the destination address increments from \$9FFF to \$A000; this fixes freezing in _F1 Championship Season 2000_ (#464)
* HDMA5 writes with bit 7 set are now allowed to change the length of in-progress HDMAs; this fixes corrupted graphics in _NASCAR 2000_ (#467)
* HDMA is now able to halt the CPU mid-instruction; this fixes occasional glitchy frames in _Toy Story Racer_

# 0.10.1

## Fixes
* (**SNES**) Corrected implementation of how Mode 7 clips scrolled center point coordinates; this fixes missing background graphics on some screens in _Super Metroid_ (#426)

# 0.10.0

## New Features
* (**Genesis**) CRAM dots are now emulated
  * These are normally not visible within active display because it's uncommon to modify CRAM while the VDP is actively rendering, but they're visible in many games if vertical border rendering is enabled
* (**Genesis** / **Sega CD**) Audio low-pass filter cutoff frequencies are now configurable
* (**Genesis**) New option to apply a second-order low-pass filter only to YM2612 audio output, which should be similar to the audio circuitry in later Model 2 consoles (when used in combination with a first-order filter applied to all audio output)
* (**Genesis**) Added an aspect ratio "Auto" option (now default) that will function as either NTSC (8:7 / 32:35 PAR) or PAL (11:8 / 11:10 PAR) based on the current timing mode
* (**Genesis**) Added an option for whether to emulate YM2612 or YM3438 busy flag behavior, which affects audio in a few games (e.g. _Earthworm Jim_ and _Hellfire_)
  * There is also an Always 0 option that is less accurate to hardware but produces the "correct" behavior for both of these games
* (**SMS** / **Game Gear**) Added a hardware region "Auto" setting that attempts to auto-detect region from the cartridge header (#214)
* (**SMS**) Added the option to boot from a BIOS rather than booting directly into the game
* (**SMS** / **Sega CD**) Added the ability to boot directly into the BIOS with no cartridge/disc inserted
* (**NES**) Added an option to disable vertical overscan cropping in NTSC mode (i.e. display in 256x240 instead of 256x224)
* Added an audio option to mute all emulator audio output (#248)
* Added an option to configure the initial window size when not running in fullscreen (#409)
* GUI: Added a new File menu button and hotkey to quickly open the most recently opened ROM file (#248)

## Multi-System Fixes
* Fixed the emulator not reading gamepad inputs while the window does not have focus (#248)
* Fixed the GUI sometimes segfaulting when you close the main GUI window while an emulator is running
* The GUI window now remembers its size when the application is closed and reopened
* Initial window size now takes aspect ratio into account

## Genesis / Mega Drive Fixes
* Improved both behavioral accuracy and timing accuracy of VDP ports, VDP DMA, and the VDP FIFO; this fixes a number of bugs
  * Fixes _Clue_ sometimes having corrupted main menu graphics (#159)
  * Fixes _Gaiares_ having flickering text on the title screen
  * The emulator now fully passes the VDPFIFOTesting test ROM (#103)
  * Fixes incorrect color palettes in some demos (#183)
  * Fixes a glitch on the title screen of the homebrew _Rick Dangerous 2_ port (#102)
* Significantly improved performance due primarily to optimizations related to the YM2612 code
* Lots of mostly minor fixes to YM2612 sound chip emulation
  * Fixed multiple timing precision bugs with the LFO and hardware timers
  * Fixed vibrato / LFO FM calculations incorrectly using 11-bit precision instead of 12-bit
  * Fixed vibrato incorrectly affecting how detune computes key code
  * Fixed the accurate quantization option incorrectly quantizing channel outputs instead of carrier outputs
  * More accurate emulation of DAC crossover distortion (ladder effect)
  * Added emulation for operator evaluation pipelining in channel output calculations
  * Fixed the DAC channel not respecting the channel 6 panning bits
* Fixed a Z80 timing bug caused by a VDP DMA "optimization" introduced in v0.8.2; this fixes video/audio desync in Overdrive 2
* Fixed behavior when the controller port TH pin is set to input; this fixes controls not working properly in _Micro Machines_ (#226)
* Improved display behavior when games switch between H32 and H40 modes shortly after the start of VBlank; this fixes glitchy frames in _Bugs Bunny in Double Trouble_ (#252)
* Fixed the window nametable address not being masked correctly in H40 mode; this fixes glitchy graphics on some screens in _Cheese Cat-Astrophe Starring Speedy Gonzales_ (#253)
* Added a 1-instruction delay to handling HINT if a game enables HINTs while an HINT is pending; this fixes _Fatal Rewind_ / _The Killing Game Show_ failing to boot (#254)
* Added a 1 CPU cycle delay on every 68000 access to the Z80 side of the bus; this fixes broken audio in _Pac-Man 2: The New Adventures_ (#255)
* Fixed several major bugs in how the V counter and the VBlank status flag are emulated in interlaced modes; this fixes _Combat Cars_ freezing in 2P mode as well as occasional sprite glitches in _Sonic the Hedgehog 2_'s Vs. mode (#258)
* Fixed the Z80 RESET line not resetting the YM2612 in addition to the Z80; this fixes audio glitches in _Fantastic Dizzy_ (#397)
* Fixed the emulator not correctly initializing cartridge SRAM when the cartridge header specifies less common RAM types; this fixes the Mega Drive Mode 7 demo not working (#250)
* Fixed the interlaced ODD flag in the VDP status register not toggling correctly in single-screen interlaced mode if deinterlacing is enabled (#354)
* Fixed a number of European games with bad region headers defaulting to NTSC mode instead of PAL (#176 / #394)
* The 68000 interrupt handler now takes 54 CPU cycles instead of 44; this is more accurate and fixes a minor glitch in Overdrive (#419)
* Fixed the emulator crashing if a game reads from Z80 $7F0C-$7F0F or writes to Z80 $7F08-$7F0F
* When horizontal border rendering is enabled, fixed the right border rendering as the wrong color if the backdrop color is changed between lines (Overdrive 1 does this on some screens)
* The non-linear VDP color scale option is now enabled by default because it is more accurate to actual hardware's video output (#249)

## Sega CD Fixes
* Fixed the CDD reset register (\$FF8001) not correctly resetting CDD state; this fixes the _Pier Solar_ enhanced audio disc failing to boot in SCD Mode 2 (#215)
* Fixed the CUE parser being too strict around parsing leading whitespace on lines (#418)
* Fixed some bugs related to changing discs while a game is running (particularly when using a Model 1 BIOS)
* Implemented more accurate memory mirroring in sub CPU memory map; this fixes excessive error logging in _WWF: Rage in the Cage_ (#216)

## 32X Fixes
* Fixed the Genesis VDP and 32X VDP frames incorrectly lining up exactly when the Genesis VDP is in H32 mode; this fixes some minor graphical issues in _NFL Quarterback Club_ (#230)
* Files with .bin extensions are now auto-detected as 32X instead of Genesis if they contain the 32X security program at the expected location in ROM (#259)
* Horizontal blur shaders now scale the effect properly when the Genesis VDP is in H32 mode
* The "apply Genesis low-pass filter to PWM" setting is now on by default because that seems to be more accurate to actual hardware's audio circuitry
* Fixed initial window size being slightly too small to fit integer-height-scaled output when the Genesis VDP is in H32 mode (#420)

## Master System / Game Gear Fixes
* Somewhat improved VDP-related timings; this fixes glitchy cutscene graphics in _Madou Monogatari I_ (#213) and fixes most tests in the SMSVDPTest test ROM (#190)
* (**SMS**) Fixed sprites never displaying on the topmost line of active display
* (**SMS**) Fixed the "crop vertical borders" setting incorrectly cropping the top 16 lines and bottom 16 lines in 224-line mode
* (**Game Gear**) The region bit in I/O port \$00 now properly reflects the hardware region instead of being hardcoded to 1; this fixes the Start button not working on the title screen of _Pop Breaker_ (#214)
* (**Game Gear**) Fixed the viewport Y offset being 16 lines off in 224-line mode; this fixes glitchy graphics in _Micro Machines_ (#221)
* (**Game Gear**) I/O port \$01 is now read/write; this fixes _Primal Rage_ freezing at the title screen (#220)

## NES Fixes
* Improved accuracy of Namco 163 expansion audio emulation (used by _Megami Tensei II_ among other games)
* Cartridge PRG RAM is now initialized to all 1s instead of all 0s; this fixes _Famicom Jump II_ crashing on first boot (#280)
* CNROM cartridges (iNES mapper 3) are now allowed to have PRG RAM; this fixes _Hayauchi Super Igo_ effectively freezing upon starting a game (#273)
* Fixed MMC5 PRG RAM bank mapping in MMC5 cartridges that have two 8KB RAM chips; this fixes _Uncharted Waters_ being completely broken upon starting a game (#275)
* Added support for NROM cartridges (iNES mapper 0) with only 8KB of PRG ROM; this fixes _Galaxian_ failing to boot (#261)
* Fixed a VRC4 mapper bug where the highest bit of the 9-bit CHR ROM bank number was not working correctly; this fixes corrupted graphics in _World Hero_ (#283)
* Fixed the DMC sample address incorrectly defaulting to \$8000 at power-on instead of \$C000 (#292)
* PPU palette RAM is now initialized to the palette in the power\_up\_palette test ROM instead of all 0s
* Fixed iNES header parsing reading the wrong byte when checking for the PAL bit; this fixes the _Populous_ prototype incorrectly defaulting to NTSC instead of PAL (#391)
* OAMADDR is now reset to 0 on every line during rendering; this fixes _Ghostbusters II_ freezing after the title screen (#421)

## SNES Fixes
* Fixed incorrect cartridge SRAM mapping for LoROM cartridges with more than 32 KB of SRAM; this fixes _Kaite Tsukutte Asoberu Dezaemon_ failing to boot (#234)
* Fixed Mode 7 incorrectly clipping the scrolled center point to signed 11-bit rather than signed 10-bit; this fixes glitched Mode 7 graphics in _Kaite Tsukutte Asoberu Dezaemon_
* Fixed incorrect emulation of interactions between offset-per-tile modes and BG1/BG2 horizontal scrolling; this fixes glitchy graphics in some stages in _The Adventures of Batman & Robin_ (#246)
* WRAM contents are now randomized at power-on; this fixes major bugs in _Power Drive_ and the European version of _PGA Tour Golf_ (#188 / #235)
* Cartridge SRAM is now initialized to all 1s instead of all 0s; this fixes _Ken Griffey Jr. Presents Major League Baseball_ crashing when you select Season mode (#231)
* Fixed incorrect mapping of the DSP-1 port mirrors in LoROM cartridges; this fixes the DSP-1 tech demo prototype not working properly (#233)
* Fixed some revisions of _Dungeon Master_ being incorrectly detected as DSP-1 instead of DSP-2

## Game Boy [Color] Fixes
* The MBC5 ROM bank is now initialized to 1 instead of 0; this fixes _Project S-11_ failing to boot (#410)
* Cartridge SRAM is now initialized to all 1s instead of all 0s

# v0.9.0

## New Features
* (**Genesis** / **Sega CD** / **32X**) Replaced the low-pass filtering settings added in v0.8.3 with a new set of options that should be more accurate to actual hardware
  * New option to apply a first-order 3.39 KHz low-pass filter to Genesis audio output; this is **ON** by default (biggest change from previous default settings)
  * New option to apply a second-order 7.97 KHz low-pass filter to Sega CD PCM audio output; this is **ON** by default
  * New options to individually configure whether the Genesis low-pass filter is applied to Sega CD and 32X audio output; these are all **OFF** by default
* (**Genesis** / **SNES**) Added a new video setting to disable deinterlacing in the handful of games that use interlaced display modes (e.g. _Sonic the Hedgehog 2_ in 2P Vs. mode, _Ys III_ (Genesis) with the in-game "Int Mode" option enabled,  _Air Strike Patrol_ in mission briefing screens)
  * Deinterlacing enabled matches the behavior in previous versions: normal-resolution interlaced modes display the same as progressive mode, and high-res interlaced modes make the graphics processor render all 448/480 lines every frame
* (**Sega CD**) Added an option to overclock the sub CPU by decreasing the master clock divider (#138)
* (**Sega CD**) Added an option to increase the disc drive speed when reading data tracks (#138)
  * This has low compatibility but can shorten loading times in some games. Compatibility is _slightly_ higher when the sub CPU is overclocked
* (**Sega CD**) Added an additional PCM chip interpolation option for 6-point cubic Hermite interpolation, which in some cases produces a slightly cleaner sound than 4-point cubic Hermite (the existing setting)
* Added a new hotkey to quickly toggle whether overclocking settings are enabled, for the systems that support overclocking (this includes Sega CD's new drive speed setting)
  * This is mainly useful for Sega CD, where increasing the drive speed can shorten loading times during gameplay but almost always breaks FMVs and animated cutscenes

## Improvements
* Audio resampling code has been rewritten to use the windowed sinc interpolation algorithm, which is much higher quality than the previous resampling implementation at a relatively low performance cost (for most emulated systems)
  * Windowed sinc interpolation can be very performance-intensive for NES and GB/GBC audio resampling, so these two systems have a new audio setting to choose between windowed sinc interpolation and the old resampling algorithm (low-pass filter followed by nearest neighbor interpolation)
* (**Genesis**) Slightly improved performance by optimizing VDP rendering and tile fetching code
* (**Genesis**) Frontends now recognize .gen and .smd as file extensions for Genesis / Mega Drive ROM images (#149)
  * This includes attempting to auto-detect when a ROM image is interleaved (common for .smd files), and deinterleaving it during load
* (**SMS**) The "crop vertical borders" video setting now defaults to enabled instead of disabled; unlike the left border, the vertical borders will only ever show the current backdrop color
* (**SMS**) The SMS model setting now defaults to SMS1, which emulates a VDP hardware quirk that is required for the Japanese version of _Ys_ to render correctly (#182)
* (**SMS** / **Game Gear**) Reduced log level of a warning message that caused excessively verbose log output in _Virtua Fighter Mini_ (#199)
* (**SNES**) In games that use the SA-1 coprocessor, the SA-1 CPU now gets a wait cycle every time it accesses SA-1 BW-RAM, similar to actual hardware
  * The SA-1 CPU still runs faster than actual hardware in some cases because bus conflict wait cycles are not emulated
* GUI: When opening a game that requires a BIOS ROM or firmware ROM (e.g. any Sega CD game), if the BIOS/firmware ROM path is not configured, the error window now contains a button to configure the appropriate ROM path and immediately launch the game
* CLI: If no config file exists, the CLI will now attempt to write out the default config to the config path so that it can be edited manually if desired
* Save state files are now internally compressed using zstd which should reduce save state file size by at least 50%, often by 70-80%
* Frontends should now correctly handle files with uppercase file extensions

## Multi-System Fixes
* Fixed a performance bug in the audio resampling code that could have caused intermittent extremely poor performance due to performing arithmetic on subnormal floating-point numbers, which can be up to 100 times slower than normal floating-point arithmetic on some CPUs (#135)
* Linux: AppImage builds now exclude all Wayland-related system libraries during packaging; this fixes the emulator failing to launch in some distros, e.g. Solus Plasma (#143)
* Linux: AppImage builds now interpret relative paths in command-line arguments as being relative to the original working directory where the AppImage was launched from, not the AppImage internal runner directory (#147)
* Linux/BSD CLI: For these platforms only and for the CLI only, reverted the change to estimate window scale factor because `SDL_GetDisplayDPI` does not return reliable values on Linux/BSD
* Adjusted frame time sync's sleep implementation to fix frame time sync potentially causing slowdown on some platforms
* Save state files are now explicitly versioned, which fixes potential crashing when attempting to load an incompatible save state file from a different version

## Genesis / Mega Drive Fixes
* Fixed the 68000 incorrectly being allowed to access audio RAM while the Z80 is on the bus; this fixes freezing in _Joe & Mac_ (#144)
* Fixed Z80 RESET not clearing the Z80's HALT status
* Fixed writes to YM2612 F-num high / block registers (\$A4-\$A6 and \$AC-\$AE) taking effect immediately instead of after the next F-num low register write; this fixes some music glitches in _Valis_
* Implemented more accurate emulation of how the YM2612 computes operator amplitude from phase and envelope attenuation
* Fixed in-game saves not working correctly when _Sonic & Knuckles_ is locked on to a cartridge with SRAM (e.g. Sonic 3)
* Fixed certain revisions of _QuackShot_ not loading correctly due to having non-standard cartridge ROM address mappings (#174)
* Fixed some illegal 68000 opcodes incorrectly decoding to "valid" instructions (#184 / #185)
* Fixed an edge case related to how sprite tile/pixel overflow interacts with H=0 sprite masking (#186)

## Sega CD Fixes
* Implemented a higher minimum seek time for small seek distances; this fixes _Thunder Storm FX_ (JP) failing to boot (#178)
* Fixed a regression introduced in v0.8.3 that caused PCM chip channels to skip the first sample after being enabled (this made little-to-no audible difference in practice because the first sample is usually 0)
* Fixed slightly inaccurate emulation of PCM chip looping behavior at sample rates higher than 0x0800 / 32552 Hz
* Fixed inaccurate emulation of CD-DA fader volumes 1-3 out of 1024 (should be 50-60 dB of attenuation instead of complete silence)
* Unmapped/unknown address accesses will now log an error instead of crashing the emulator

## 32X Fixes
* Fixed a major bug in the PWM resampling code that caused PWM audio output to sound significantly more poppy and crackly than it's supposed to
* Fixed a bug around synchronizing SH-2 accesses to 32X communication ports that could have caused writes to be skipped in some cases; this fixes freezing in the _Sonic Robo Blast 32X_ demo (#160)
* Significantly improved timing of 32X VDP interrupts for the SH-2s (#166)
* Significantly improved synchronization between the SH-2s and the 68000
* Fixed PWM DMA transfer rate via DREQ1 not taking the PWM timer interval into account; this fixes broken sound effects in _BC Racers_ (#179)

## Master System / Game Gear Fixes
* Fixed the Z80's RETI instruction not correctly copying IFF2 to IFF1 like RETN does; this fixes _Desert Strike_ from freezing when you press Start/Pause (#181)
* Fixed incorrect handling of non-power-of-two ROM sizes, which fixes several homebrew games and demos (#201 / #203 / #204)
* (**Game Gear**) Fixed the emulator crashing if a game enables the VDP's 224-line mode, as the homebrew _GG Turrican_ does (#202)

## SNES Fixes
* Implemented more accurate clipping and truncation in Mode 7 intermediate calculations; this fixes glitched Mode 7 graphics in _Tiny Toon Adventures: Wacky Sports Challenge_ (#161)
* Mode 7 registers are now latched about 12 pixels before line rendering begins; this fixes a glitchy line near the bottom of the play area in _Battle Clash_, where the screen transitions from Mode 1 to Mode 7
* Implemented an obscure behavior regarding the effects of writing to OAM during active display; this fixes incorrect sprite display in _Uniracers_' Vs. mode (#164)
* Made a best effort at implementing the effects on sprites of toggling forced blanking during active display; this mostly fixes some test ROMs that exercise this (#162)
* Adjusted behavior of APU communication ports when the 65816 writes to a port on the same cycle that the SPC700 clears the port; this fixes _Kishin Douji Zenki: Tenchi Meidou_ failing to boot (#187)

## Game Boy [Color] Fixes
* Implemented an obscure behavior where pulse channels should output a constant 0 after power-on until after the first phase increment; this fixes missing voice samples in _Daiku no Gen-san - Robot Teikoku no Yabou_ (#151)
* Fixed a bug related to the pulse channel phase counter reloading on the same cycle as a frequency change via NR13/NR14/NR23/NR24; this combined with the above change fixes missing voice samples in _Keitai Denjuu Telefang_ (#47)
* Added emulation for a hardware quirk where the Mode 2 STAT interrupt appears to trigger 145 times per frame, not 144; this fixes [GBVideoPlayer](https://github.com/LIJI32/GBVideoPlayer) (#155)
* CGB palette RAM auto-increment flags now default to 1 (#156)
* Slightly adjusted timings related to powering on the PPU; this combined with the above change fixes [GBVideoPlayer2](https://github.com/LIJI32/GBVideoPlayer2) (#156)
* Fixed an edge case where LYC writes at the beginning of a line were not triggering the LY=LYC STAT interrupt under certain conditions; this fixes glitchy graphics on the title screen of the _SQRKZ_ homebrew (#154)
* The contents of OBJ palette RAM are now randomized at power-on (#152)

# v0.8.3

## New Features
* (**Genesis / Sega CD / 32X**) Added an audio setting to select 1 of 4 different audio low-pass filters, with cutoff frequencies ranging from about 15000 Hz (comparable to the existing filter) to about 5000 Hz (produces a very soft sound)
* (**Genesis / Sega CD / 32X**) Added a video setting to enable/disable individual graphics layers
* (**Sega CD**) Added an audio enhancement setting to apply linear interpolation or cubic Hermite interpolation to PCM sound chip channels; this _significantly_ reduces audio noise and audio aliasing in games that play music or voice acting through the PCM chip (e.g. _Lunar: Eternal Blue_ all the time, _Sonic CD_ in past stages, basically every FMV game for cutscene audio)
* (**GB**) Added an option to use a custom 4-color palette, with a color picker UI for configuring the custom palette colors
* Added a new hotkey that completely exits the application (#140)
  * The previous "quit" hotkey (which only closed the currently running game) has been renamed to "power off"

## Improvements
* (**32X**) PWM chip audio output resampling now uses cubic interpolation rather than a filter that assumed a source frequency of 22 KHz; this should improve audio quality in games that use PWM sample rates other than 22 KHz (e.g. _After Burner Complete_ and _Space Harrier_)
* Input mappings that use modifier keys (Shift / Ctrl / Alt) no longer distinguish between Left and Right versions of the modifier, e.g. Left Shift and Right Shift are now both treated as simply "Shift" for input mapping purposes (#139)
* Redesigned most of the audio low-pass filters to explicitly target a cutoff frequency of about 15000 Hz with a stopband edge frequency of about 20000 Hz, which should further reduce resampling-related audio aliasing
  * For performance reasons, NES and GB/GBC instead target a cutoff frequency of roughly 10000 Hz with a less steep attenuation slope past the cutoff frequency
* Implemented a performance optimization in how audio low-pass filters are applied when running on CPUs that support x86_64 AVX and FMA instructions (which is almost every x86_64 CPU made in the last 10 years; AVX2 is not needed)
* (**SMS / Game Gear / Genesis**) Improved video memory viewer UI so that it's now possible to view CRAM and VRAM simultaneously, as well as current VDP settings (captured once per frame at the beginning of VBlank)
* Display scale factor / DPI is now taken into account when determining initial emulator window size in windowed mode
* GUI: The GUI window is now repainted immediately when a directory scan finishes, rather than requiring mouse movement or a keyboard input to trigger the repaint

## Fixes
* (**32X**) Fixed the 68000 incorrectly being allowed to change the PWM timer interrupt interval via \$A15130 writes; this fixes _Primal Rage_ having horribly broken sound effects
* Fixed an input configuration bug that made it effectively impossible to correctly configure any gamepad where SDL reads digital buttons as analog axes, such as the 8BitDo M30 with its C and R buttons (#135)
* Fixed some minor bugs in the common audio resampling code related to how low-pass filters are applied
* CLI: For options that only accept a fixed set of possible values, the list of possible values in the help text is now auto-generated at compile time; this fixes at least one case where an option's help text listed a possible value that does not exist, and another case where an option's help text omitted a valid possible value

# v0.8.2

## New Features
* Video/audio sync improvements which should enable significantly improved frame pacing without needing to rely on 60Hz VSync (which can cause very noticeable input latency on some platforms)
  * Added a new "frame time sync" option that uses the host system clock to match the emulated system's framerate and frame timing as closely as possible without relying on host GPU synchronization (i.e. VSync)
  * Added a new option for dynamic audio resampling ratio, which periodically adjusts the audio resampling ratio to try and avoid audio buffer underflows and overflows (which both cause audio popping)
  * Audio sync now checks the audio buffer size every 16 samples enqueued rather than only checking once per frame, which should significantly reduce stuttering when audio sync is enabled without VSync or frame time sync
  * Adjusted default sync/audio settings values to hopefully make stuttering and audio popping less likely when running with default settings
  * In the GUI, video/audio sync settings have been moved to a new window under Settings > Synchronization
* Input mapping overhaul to make input mapping/configuration more flexible (#134 / #137)
  * Keyboard and gamepad settings are no longer separate configurations; each system now supports up to 2 mappings for each emulated button where each mapping can be a keyboard key, a gamepad input, or a mouse button
  * Key/input/button combinations (2 or 3 simultaneous inputs) are now supported for mappings in addition to individual keys/inputs
  * Hotkeys can now be mapped to gamepad inputs, mouse buttons, and combinations in addition to individual keyboard keys
  * Each input settings window now has a button to apply one of two keyboard presets for P1 inputs, one with arrow keys mapped to the d-pad and one with WASD mapped to the d-pad
  * Added a new set of hotkeys for saving/loading specific save state slots (#134)
* (**Genesis / Sega CD / 32X**) Added an option to overclock the main Genesis CPU (the 68000) by decreasing the master clock divider, which can reduce or eliminate slowdown in games (#133)
* (**SMS / Game Gear**) Replaced the "double Z80 CPU speed" setting with an option to overclock at finer granularity by decreasing the Z80 master clock divider
* Added an option to only hide the mouse cursor when in fullscreen, in addition to the previous settings of "always hide" and "never hide"
* Added an option to change the fullscreen mode from borderless to exclusive
* Added an option to change the audio output frequency from 48000 Hz to 44100 Hz

## Improvements
* (**Genesis / Sega CD**) Slightly improved performance by advancing the emulated clock in larger intervals while a long VDP DMA is in progress
* (**32X**) Slightly improved performance by optimizing SH-2 instruction decoding
* (**GB**) Improved video frame delivery behavior when the PPU is powered off to make it play a little nicer with VSync and frame time sync
* The emulator window is now explicitly focused/raised when a game is loaded; previously this wouldn't always happen automatically, particularly on Windows

## Fixes
* (**Sega CD**) Slightly extended the delay between a game sending a CDD Play/Read command and the CD drive reading the first sector; this fixes Time Gal having excruciatingly long "load times"
* (**Sega CD**) Fixed a bug where some backend settings would not correctly persist after loading a save state (they would temporarily revert to what they were when the save state was created)

# v0.8.1

## New Features
* Made the game save file and save state locations configurable (#132)
* Added a new --load-save-state <SLOT> command-line arg to load a specific save state slot at game launch (#132)
* Added an option to attempt to load the most recently saved state when launching a game
* (**Game Gear**) Added an option to render at SMS resolution (256x192) instead of native resolution (160x144)

## Fixes
* (**Sega CD**) Fixed a bug where loading a save state could possibly crash the emulator due to a stack overflow; this was particularly likely to happen on Windows due to the small default stack size
* (**SMS/Game Gear**) Fixed the VDP display disabled implementation so that it properly blanks the display rather than leaving the previous frame onscreen
* (**SMS**) Fixed the NTSC/PAL and SMS Model settings not having any effect when loading a game from a .zip/.7z file rather than a .sms file
* (**NES**) Fixed multiple bugs related to how the PPU determines what color to display when rendering is disabled while PPUADDR points to palette RAM; this fixes Micro Machines having a solid gray bar in the middle of the title screen, as well as several test ROMs that rely on this hardware quirk for high-color display (#53 / #55 / #56)
* GUI: Saving or loading a save state slot from the GUI window now also changes the selected save state slot
* CLI: Fixed the 32X option missing from the help text for the --hardware arg (#131)
* The video memory viewer window now renders without VSync; this fixes likely stuttering and audio popping while the memory viewer window is open

# v0.8.0

## 32X Support
* Added support for emulating the Sega 32X / Mega 32X
* All released 32X games plus Doom 32X Resurrection should be playable except for the 6 FMV games that require the Sega CD 32X combo
* SH-2 CPU cache and basic SH-2 memory access timings are emulated, so overall SH-2 speed should be moderately accurate (though still faster than actual hardware in some cases)
* SH-2 emulation is currently not optimized well - full-speed 32X emulation requires a CPU with decent single-core performance, and fast-forward speed will be very limited

## New Features
* Added support for loading directly from .zip and .7z compressed archives for every console except Sega CD (#91)
* (**SNES**) Added an audio enhancement option for cubic Hermite interpolation between decoded ADPCM samples, which usually makes the audio sound sharper and less muffled
* (**Genesis**) Added an option to have no controller plugged into one or both of the controller ports, for games that behave differently based on the presence or absence of a controller (#113)
* (**NES**) Added support for the UNROM 512 mapper (iNES mapper 30), a homebrew mapper used by a number of games including Black Box Challenge and Battle Kid 2 (#73 / #86)
* (**GB**) Added partial support for the Hudson HuC-3 mapper, used by Robopon and a few Japan-only games (#89)
* GUI: Added a new "Open Using" menu option to open a file using a specific emulator core, rather than always choosing the core based on file extension (#121)
* GUI: Added an option to explicitly set the UI theme to light or dark rather than always using the system default

## Improvements
* (**Genesis**) YM2612 DAC crossover distortion (aka the "ladder effect") is now emulated, which significantly improves music accuracy in a number of games; this is extremely noticeable in Streets of Rage, Streets of Rage 2, and After Burner II, among others 
  * There is also a new option to disable ladder effect emulation, since the effect was less pronounced on later console models (and also because I think it's neat to hear how it affects the sound by toggling a checkbox)
* (SMS/GG/Genesis) Replaced the PSG and YM2612 low-pass filters with much more aggressive ones; this should generally improve audio quality, and in some cases will remove erroneous buzzing/popping noises that were present before (e.g. in The Adventures of Batman & Robin) (#108)
* Improved audio output behavior for all emulator backends, which should significantly reduce the likelihood of audio pops caused by audio buffer underflow
* GUI: Added help text to most options menus
* GUI: Improved performance when the main list table is large

## Fixes (Genesis / Mega Drive)
* Fixed the PSG's noise channel not oscillating when the period is set to 0 (which should behave the same as period of 1); this fixes missing high-frequency noise in Knuckles' Chaotix among other games
* Fixed a degenerate case for performance when a game repeatedly writes the same value to specific VDP registers during active display, as After Burner Complete does
* Fixed some 68000 CPU bugs discovered while working on 32X support
  * Implemented line 1010/1111 exception handling for when the 68000 executes an illegal opcode where the highest 4 bits are 1010 or 1111; Zaxxon's Motherbase 2000 depends on this to boot
  * Fixed divide by zero exception handling pushing the wrong PC value onto the stack; After Burner Complete frequently divides by zero and depends on correctly handling the exception
  * Fixed the DIVS instruction finishing way too quickly in some cases where the division overflows a signed 16-bit result but the CPU doesn't detect the overflow early
* Fixed an off-by-one error in determining whether to set the sprite overflow flag in the VDP status register; this fixes flickering sprite graphics in Alex Kidd in the Enchanted Castle (#125)
  * This was a regression introduced in v0.6.1 as part of the changes to get Overdrive 2's textured cube effect working
* Adjusted how writes to the controller CTRL registers (\$A10009 / \$A1000B) affect the controller's TH line; this fixes controls not working properly in Trouble Shooter (#110)
* Made it possible for games to read the VINT flag in the VDP status register as 1 slightly before the 68000 INT6 interrupt is raised; this fixes Tyrants: Fight Through Time and Ex-Mutants failing to boot (#127)
* Implemented undocumented behavior regarding how the Z80 BIT instruction sets the S and P/V flags; this fixes missing audio in Ex-Mutants, which relies on this behavior in its audio driver code 
* Implemented approximate emulation of memory refresh delay
  * This is emulated by simply stalling the 68000 for 2 out of every 128 mclk cycles, unless it executes a very long instruction that doesn't access the bus mid-instruction (e.g. multiplication or division)
  * Memory refresh delay is not emulated in 32X mode because it seemed to break audio synchronization between the Genesis and 32X hardware in some games
* Added SRAM mappings for several games that have SRAM in the cartridge but don't declare it in the cartridge header: NHL 96, Might and Magic, and Might and Magic III (#107 / #116 / #117)
* Little-endian ROM images are now detected and byteswapped on load; this along with a custom ROM address mapping fixes Triple Play failing to boot (#112)
* The emulator will now recognize the unconventionial region string "EUROPE" as meaning that the game only supports PAL/EU; this fixes Another World incorrectly defaulting to NTSC/US mode instead of PAL/EU (#122)
* Unused bits in the Z80 BUSACK register (\$A11100) now read approximate open bus instead of 0; this fixes Danny Sullivan's Indy Heat failing to boot (#120)
* Improved VDP DMA timing; this fixes corrupted graphics in OutRunners (#118)
* The vertical interrupt is now delayed by one 68000 instruction if a game enables vertical interrupts while a vertical interrupt is pending; this fixes Sesame Street: Counting Cafe failing to boot (#119)
* The Z80 BUSACK line now changes immediately in response to bus arbiter register writes instead of waiting for the next Z80 instruction time slot; this fixes the Arkagis Revolution demo failing to boot (#123)
* The emulator will now enable the bank-switching Super Street Fighter 2 mapper if the cartridge header declares the system as "SEGA DOA" in addition to the standard value of "SEGA SSF"; this fixes the Demons of Asteborg demo not working properly (#115)

## Fixes (Other)
* Fixed save state slots not working properly if the ROM filename contains multiple dots; before this fix, only one slot would ever be used
* (**Sega CD**) When a game issues a CDD command while the drive is playing, the drive now continues to read one more sector before it changes behavior in response to the new command; this fixes Radical Rex crashing during the intro (#100)
* (**Sega CD**) Writes to PRG RAM by the main CPU and the Z80 are now blocked unless the sub CPU is removed from the bus; this fixes Dungeon Explorer from crashing after the title screen (#104)
* (**Sega CD**) The sub CPU is now halted if it accesses word RAM in 2M mode while word RAM is owned by the main CPU, and it remains halted until the main CPU transfers ownership back to the sub CPU. This fixes glitched graphics in Marko's Magic Football (#101)
* (**Sega CD**) Various fixes to CDC register and DMA behavior; with this plus all of the above fixes, the emulator now fully passes the mcd-verificator test suite (#105)
* (**NES**) The UxROM mapper code (iNES mapper 2) no longer assumes that the cartridge has no PRG RAM; this fixes Alwa's Awakening: The 8-Bit Edition failing to boot (#93)
* (**SNES**) Adjusted timing of PPU line rendering to occur 4 mclk cycles later; this fixes Lemmings having a flickering line at the top of the screen during gameplay
  * This worked correctly prior to v0.7.2 - it was broken by the CPU timing adjustment that fixed Rendering Ranger R2 from constantly freezing
* (**GB**) Fixed the window X condition incorrectly being able to trigger when WX=255 and fine X scrolling is used (SCX % 8 != 0); this fixes corrupted graphics in Pocket Family GB 2
* Fixed the emulator crashing if prescale factor is set so high that the upscaled frame size exceeds 8192x8192 in either dimension


# Older

See https://github.com/jsgroth/jgenesis/releases
