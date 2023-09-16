# jgenesis-cli

Command-line interface that invokes `jgenesis-native-driver`.

To run with all default settings:
```
cargo run --release --bin jgenesis-cli -- -f /path/to/rom.md
```

To view all command-line args:
```
cargo run --release --bin jgenesis-cli -- -h
```

## Limitations

The command-line interface does not currently support configuring DirectInput gamepad inputs, nor does it support configuring Player 2 controls.