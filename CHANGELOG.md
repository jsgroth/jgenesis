# Next Release

## New Features
* (**Genesis** / **SNES**) Added a new video setting to disable deinterlacing in the handful of games that use interlaced display modes (e.g. _Sonic the Hedgehog 2_ in 2P Vs. mode, _Ys III_ (Genesis) with the in-game "Int Mode" option enabled,  _Air Strike Patrol_ in mission briefing screens)
  * Deinterlacing enabled matches the behavior in previous versions: normal-resolution interlaced modes display the same as progressive mode, and high-res interlaced modes make the graphics processor render all 448/480 lines every frame
* (**Sega CD**) Added an audio setting for whether to apply low-pass filtering to CD audio track playback, which defaults to disabled
* (**Sega CD**) Added an option to overclock the sub CPU by decreasing the master clock divider (#138)
* (**Sega CD**) Added an option to increase the disc drive speed when reading data tracks (#138)
  * This has low compatibility but can shorten loading times in some games. Compatibility is _slightly_ higher when the sub CPU is overclocked
* (**Sega CD**) Added an audio enhancement setting to apply quintic (5th-order) Hermite interpolation to PCM chip channels, which in some cases produces slightly cleaner audio than cubic Hermite interpolation
* Added a new hotkey to quickly toggle whether overclocking settings are enabled, for the systems that support overclocking (this includes Sega CD's new drive speed setting)
  * This is mainly useful for Sega CD, where increasing the drive speed can shorten loading times during gameplay but almost always breaks FMVs and animated cutscenes

## Improvements
* GUI: When opening a game that requires a BIOS ROM or firmware ROM (e.g. any Sega CD game), if the BIOS/firmware ROM path is not configured, the error window now contains a button to configure the appropriate ROM path and immediately launch the game
* CLI: If no config file exists, the CLI will now attempt to write out the default config to the config path so that it can be edited manually if desired
* Save state files are now internally compressed using zstd which should reduce save state file size by at least 50%, often by 70-80%
* (**Genesis**) Slightly improved performance by optimizing VDP rendering and tile fetching code
* (**SMS**) The "crop vertical borders" video setting now defaults to enabled instead of disabled; unlike the left border, the vertical borders will only ever show the current backdrop color

## Fixes
* (**Genesis**) Fixed the 68000 incorrectly being allowed to access audio RAM while the Z80 is on the bus; this fixes freezing in _Joe & Mac_ (#144)
* (**Genesis**) Fixed Z80 RESET not clearing the Z80's HALT status
* (**Sega CD**) Fixed a regression introduced in v0.8.3 that caused PCM chip channels to skip the first sample after being enabled (this made little-to-no audible difference in practice because the first sample is usually 0)
* (**Sega CD**) Fixed slightly inaccurate emulation of PCM chip looping behavior at sample rates higher than 0x0800 / 32552 Hz
* (**Sega CD**) Fixed inaccurate emulation of CD-DA fader volumes 1-3 out of 1024 (should be 50-60 dB of attenuation instead of complete silence)
* (**Sega CD**) Unmapped/unknown address accesses will now log an error instead of crashing the emulator
* Fixed a performance bug in the audio resampling code that could have caused intermittent extremely poor performance due to performing arithmetic on subnormal floating-point numbers, which can apparently be up to 100 times slower than normal floating-point arithmetic (#135)
* Linux: AppImage builds now exclude all Wayland-related system libraries during packaging; this fixes the emulator failing to launch in some distros, e.g. Solus Plasma (#143)
* Linux: AppImage builds now interpret relative paths in command-line arguments as being relative to the original working directory where the AppImage was launched from, not the AppImage internal runner directory (#147)
* Linux/BSD CLI: For these platforms only and for the CLI only, reverted the change to estimate window scale factor because `SDL_GetDisplayDPI` does not return reliable values on Linux/BSD
  * This does not affect the GUI which still passes along the scale factor determined by eframe/winit
* Adjusted frame time sync's sleep implementation to fix frame time sync potentially causing slowdown on some platforms
* Save state files are now explicitly versioned, which fixes potential crashing when attempting to load an incompatible save state file from a different version

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
