# segacd-core

Sega CD / Mega CD emulation core.

The Sega CD is a lot more than just a CD-ROM drive add-on. It's practically a second console that plugs into the Genesis through an expansion slot. It contains the following components:
* 1x CD-ROM drive
* Sanyo LC8951, a CD-ROM decoder & error correction chip
* An additional 68000 CPU clocked at 12.5 MHz, about 63% faster than the Genesis 68000
* Graphics ASIC that can perform hardware-accelerated image scaling and rotation
* Ricoh RF5C164, an 8-channel PCM sound chip with 64KB of attached waveform RAM
  * This is in addition to CD audio playback
* 512KB of working/program RAM for the Sega CD 68000
* 256KB of "word RAM" that can be exchanged between the two 68000s; meant primarily for data transfer from the Sega CD components to the Genesis components
* 8KB of battery-backed RAM for storing save data, as well as support for an optional 128KB battery-backed RAM cartridge
* 128KB BIOS mapped into the Genesis 68000's address space; contains a boot ROM and routines for controlling the CD-ROM drive
* Lots of new registers, mostly for controlling the new components and for coordination between the two 68000s

All of the Genesis components run in parallel with the new components, and all rendering is still performed using the Genesis VDP.
 
This crate contains code for the following:
* CD-ROM file parsing and I/O
* Sega CD's CD drive
* LC8951 CD decoder chip
* Sega CD's graphics ASIC
* PCM sound chip
* Sega CD memory map
* Sega CD main loop
