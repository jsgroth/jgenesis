# sh2-emu

Emulation core for the Hitachi SH-2 CPU, used in the Sega 32X and the Sega Saturn. This includes emulation of the
hardware units in the SH7604 package which is used in both systems:
* 4KB of fast internal RAM that can be used as a mixed instruction/data cache
* A 2-channel DMA controller
* A division unit (signed 64-bit ÷ 32-bit division with 32-bit quotient and 32-bit remainder)
* A free-running timer
* A watchdog timer that can also be used as an interval timer
* A serial communication interface

The free-running timer is not currently emulated but the rest of these components are, at least as far as 32X software uses them.

Unlike the other CPU emulators, this emulator does not attempt to track timing. Almost all instructions take 1 cycle
plus memory access delays.