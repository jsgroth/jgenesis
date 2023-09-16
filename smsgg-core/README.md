# smsgg-core

Sega Master System / Game Gear emulation core.

The Game Gear is nearly identical to the Master System hardware-wise, with a few key differences:
* The VDP still renders 256x192 frames, but only the center 160x144 pixels are displayed
* The VDP color format is changed from RGB222 to RGB444, and color RAM is doubled in size to accommodate this
* The Start/Pause button flips a bit in a register instead of generating an NMI
* There is a new stereo sound control register that enables hard panning each of the 4 audio channels

This crate contains code for:
* SMS/GG VDP (video display processor)
* SMS/GG PSG (programmable sound generator)
* SMS/GG memory map
* SMS FM sound unit expansion (YM2413 core)
* Tying together all of the SMS/GG components