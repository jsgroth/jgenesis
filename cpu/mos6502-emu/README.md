# mos6502-emu

Cycle-based emulation core for the MOS 6502 CPU, used in the NES. While not the most capable 8-bit CPU, the 6502 was very popular during its time due to its affordability.

This implementation supports both the stock 6502 and the NES 6502. The only difference between them is that in the NES 6502, the decimal mode flag does nothing instead of enabling BCD arithmetic.
