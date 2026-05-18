# huc6280-emu

Emulation core for the HuC6280, the PC Engine CPU. Heavily based on the 65C02 but it contains an internal MMU that maps 16-bit logical addresses to 21-bit physical addresses, along with a number of additional instructions (some PCE-specific).

This core aims to be cycle-accurate, but the bus implementation is responsible for counting cycles and synchronizing other components on each call to `read()`, `write()`, or `idle()`.

This crate only implements the 65C02-based CPU core, not other functionality included in the HuC6280 chip (e.g. the PSG and timer).
