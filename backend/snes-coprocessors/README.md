# snes-coprocessors

Emulation of coprocessors used in SNES cartridges.

## Supported

### CX4

Programmable Hitachi HG51B CPU clocked at 20 MHz. Used by 2 games, _Mega Man X2_ and _Mega Man X3_

### DSP-1 / DSP-2 / DSP-3 / DSP-4

All 4 of these use a pre-programmed NEC µPD77C25 CPU clocked at 8 MHz, but with different program and data ROMs.

DSP-1 was used by 16 games, including _Super Mario Kart_ and _Pilotwings_. DSP-2, DSP-3, and DSP-4 were each only used in 1 game: _Dungeon Master_ (DSP-2), _SD Gundam GX_ (DSP-3), and _Top Gear 3000_ (DSP-4)

### SA-1

Programmable 65C816 CPU clocked at 10.74 MHz. Used by 35 games, including _Kirby Super Star_, _Kirby's Dream Land 3_, and _Super Mario RPG_

### S-DD1

Data decompression chip with a compression algorithm tailored to SNES graphical data. Used by 2 games, _Star Ocean_ and _Street Fighter Alpha 2_

### SPC7110

Data decompression chip with a compression algorithm tailored to SNES graphical data, similar to S-DD1 but different algorithm. Used by 3 games, all by Hudson: _Tengai Makyou Zero_, _Momotarou Dentsetsu Happy_, and _Super Power League 4_

_Tengai Makyou Zero_ additionally included an Epson RTC-4513 real-time clock chip

### S-RTC

Sharp real-time clock chip used by 1 game, _Daikaijuu Monogatari II_ / _Super Shell Monsters Story II_

### ST010 / ST011

Both of these use a pre-programmed NEC µPD96050 CPU (with different program and data ROMs), with the ST010 clocked at 10 MHz and the ST011 clocked at 15 MHz.

Each of these was only used in 1 game: _F1 ROC II: Race of Champions_ (ST010) and _Hayazashi Nidan Morita Shogi_ (ST011)

### Super FX

Programmable custom-designed RISC-like CPU with hardware to automatically convert plotted bitmap graphics to the SNES bitplane graphics format. Used by 8 released games including _Star Fox_ and _Yoshi's Island_, as well as the (originally) unreleased _Star Fox 2_
