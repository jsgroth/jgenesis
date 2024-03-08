# mos6502-test-runner

Test harness for testing `mos6502-emu` against [TomHarte's 6502 tests](https://github.com/TomHarte/ProcessorTests/tree/main/nes6502).

This test harness skips all tests for illegal KIL opcodes but runs all others.

`mos6502-emu` currently passes all official opcode tests but fails on some of the illegal opcodes:
* $6B (ARR)
  * Works correctly for NES 6502 but not stock 6502
* $8B (XAA)
* $93 (SHA)
* $9B (TAS)
* $9C (SHY)
* $9E (SHX)
* $9F (SHA)
* $AB (LXA)

To run tests using stock 6502 behavior (decimal flag works):
```shell
cargo run --release --bin mos6502-test-runner -- -d ../ProcessorTests/6502/v1
```

To run tests using NES 6502 behavior (decimal flag does nothing):
```shell
cargo run --release --bin mos6502-test-runner -- -d ../ProcessorTests/nes6502/v1 --nes
```