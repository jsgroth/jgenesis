# mos6502-test-runner

Test harness for testing `mos6502-emu` against [TomHarte's 6502 tests](https://github.com/TomHarte/ProcessorTests/tree/main/nes6502).

This test harness skips all tests for illegal KIL opcodes but runs all others.

`mos6502-emu` currently passes all official opcode tests but fails on some of the illegal opcodes:
* $8B (XAA)
* $93 (SHA)
* $9B (TAS)
* $9C (SHY)
* $9E (SHX)
* $9F (SHA)
* $AB (LXA)
