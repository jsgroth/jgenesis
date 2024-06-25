# m68000-test-runner

Test harness for testing `m68000-emu` against the 68000 test suite in [TomHarte's CPU tests](https://github.com/TomHarte/ProcessorTests/).

This test harness also compares cycle counts, though any test cases that trigger address errors are ignored for cycle count testing purposes. Test cases that trigger address errors are still run when checking correctness.

To run against a single test:
```
cargo run --release --bin m68000-test-runner -- -f /path/to/test.json.gz
```

To run against the full directory of tests:
```
cargo run --release --bin m68000-test-runner -- -d /path/to/ProcessorTests/680x0/68000/v1/
```

Sample output (single test):
```
[2023-09-16T01:49:56Z INFO  m68000_test_runner] Loaded 8065 tests
[2023-09-16T01:49:56Z INFO  m68000_test_runner] 0 failed out of 8065 tests in ../ProcessorTests/680x0/68000/v1/MOVEM.w.json.gz
[2023-09-16T01:49:56Z INFO  m68000_test_runner] 0 timing mismatches out of 4281 tests in ../ProcessorTests/680x0/68000/v1/MOVEM.w.json.gz
```

## Known Failures

* `ADD.l` / `SUB.l`: The test suite seems to expect `ADDQ.l #<d>, An` and `SUBQ.l #<d>, An` to take 6 cycles, when all documentation I can find suggests that these should take 8 cycles (same as `ADDQ.w` and `SUBQ.w` with an address direct destination)
* `ASL.b` / `ASR.b` / `ASR.w` / `ASR.l`: The test cases have incorrect flag values, see https://github.com/TomHarte/ProcessorTests/issues/21
* `DIVS`: The test cases that trigger signed overflow have incorrect cycle counts compared to tests verified on actual hardware (e.g. https://gendev.spritesmind.net/forum/viewtopic.php?f=8&t=3321)
* `DIVU`: The one test case that triggers a divide by zero exception pushes the wrong PC value onto the stack compared to actual hardware; matching the test case breaks After Burner Complete (32X)