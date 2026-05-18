use clap::Parser;
use huc6280_emu::bus::{BusInterface, ClockSpeed, InterruptLines};
use huc6280_emu::{Flags, Huc6280, Registers};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

type DynError = dyn Error + Send + Sync + 'static;

#[derive(Debug, Clone, Deserialize)]
struct TestState {
    #[serde(rename = "A")]
    a: u8,
    #[serde(rename = "X")]
    x: u8,
    #[serde(rename = "Y")]
    y: u8,
    #[serde(rename = "S")]
    s: u8,
    #[serde(rename = "P")]
    p: u8,
    #[serde(rename = "PC")]
    pc: u16,
    #[serde(rename = "MPR")]
    mpr: [u8; 8],
    #[serde(rename = "RAM")]
    ram: Vec<(u32, u8)>,
}

impl From<&TestState> for Registers {
    fn from(value: &TestState) -> Self {
        Self {
            a: value.a,
            x: value.x,
            y: value.y,
            pc: value.pc,
            s: value.s,
            p: value.p.into(),
            mpr: value.mpr,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TestDefinition {
    name: String,
    opcode: u8,
    initial: TestState,
    #[serde(rename = "final")]
    final_: TestState,
    num_cycles: u32,
}

struct TestBus {
    ram: FxHashMap<u32, u8>,
    cycles: u32,
}

impl TestBus {
    fn new(initial_state: &TestState) -> Self {
        Self { ram: initial_state.ram.iter().copied().collect(), cycles: 0 }
    }

    fn read_ram(&self, address: u32) -> u8 {
        self.ram.get(&address).copied().unwrap_or(0)
    }
}

impl BusInterface for TestBus {
    fn read(&mut self, address: u32) -> u8 {
        self.cycles += 1;
        self.read_ram(address)
    }

    fn write(&mut self, address: u32, value: u8) {
        self.cycles += 1;
        self.ram.insert(address, value);
    }

    fn idle(&mut self) {
        self.cycles += 1;
    }

    fn interrupt_lines(&self) -> InterruptLines {
        InterruptLines::default()
    }

    fn set_clock_speed(&mut self, _speed: ClockSpeed) {}
}

#[derive(Debug, Parser)]
struct Args {
    /// Path to a JSON test file or to a directory full of JSON test files
    #[arg(long, short = 'f')]
    file_path: PathBuf,
}

fn main() -> Result<(), Box<DynError>> {
    env_logger::Builder::from_env(
        env_logger::Env::new().default_filter_or("info,huc6280_emu=error"),
    )
    .init();

    let args = Args::parse();

    let errors = if args.file_path.is_dir() {
        run_test_directory(&args.file_path)?
    } else {
        run_test_file(&args.file_path)?
    };

    if errors.is_empty() {
        println!("All tests passed!");
    } else {
        eprintln!("{} errors!", errors.len());
        for error in errors {
            eprintln!("{error}");
        }
    }

    Ok(())
}

fn run_test_directory(dir_path: &Path) -> Result<Vec<String>, Box<DynError>> {
    let mut json_paths = Vec::new();

    for dir_entry in fs::read_dir(dir_path)? {
        let dir_entry = dir_entry?;

        if !dir_entry.metadata()?.is_file() {
            continue;
        }

        let file_path = dir_entry.path();
        if file_path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }

        json_paths.push(file_path);
    }

    json_paths.sort();

    let errors = json_paths
        .par_iter()
        .map(|json_path| run_test_file(json_path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    Ok(errors)
}

fn run_test_file(file_path: &Path) -> Result<Vec<String>, Box<DynError>> {
    let file = File::open(file_path)?;
    let tests: Vec<TestDefinition> = serde_json::from_reader(BufReader::new(file))?;

    let mut errors = Vec::new();

    for test in tests {
        if should_skip_test(&test) {
            continue;
        }

        let mut bus = TestBus::new(&test.initial);

        let mut cpu = Huc6280::new();
        cpu.set_registers(Registers::from(&test.initial));

        loop {
            cpu.execute_instruction(&mut bus);
            if !cpu.is_mid_block_transfer() {
                break;
            }
        }

        let expected_registers = Registers::from(&test.final_);
        if &expected_registers != cpu.registers() {
            errors.push(format!(
                "[{:02X} '{}'] registers: expected {expected_registers:#X?}, actual {:#X?}",
                test.opcode,
                test.name,
                cpu.registers()
            ));
        }

        for &(address, expected) in &test.final_.ram {
            let actual = bus.read_ram(address);
            if expected != actual {
                errors.push(format!("[{:02X} '{}'] RAM[{address:06X}]: expected {expected:02X}, actual {actual:02X}", test.opcode, test.name));
            }
        }

        if bus.cycles != test.num_cycles {
            errors.push(format!(
                "[{:02X} '{}'] cycles: expected {}, actual {}",
                test.opcode, test.name, test.num_cycles, bus.cycles
            ));
        }
    }

    Ok(errors)
}

fn should_skip_test(test: &TestDefinition) -> bool {
    const TMA_OPCODE: u8 = 0x43;
    const BLOCK_TRANSFER_OPCODES: &[u8] = &[0x73, 0xC3, 0xD3, 0xE3, 0xF3];
    const SBC_OPCODES: &[u8] = &[0xE1, 0xE5, 0xE9, 0xED, 0xF1, 0xF2, 0xF5, 0xF9, 0xFD];

    // Skip TMA tests where the operand doesn't have exactly one bit set
    // When multiple bits are set, behavior is officially undefined and varies between different emulators
    // When no bits are set, A should get set to the MPR buffer contents, but the buffer contents
    // aren't specified anywhere in the test definition
    if test.opcode == TMA_OPCODE {
        let physical_pc = (u32::from(test.initial.mpr[(test.initial.pc >> 13) as usize]) << 13)
            | u32::from(test.initial.pc & 0x1FFF);
        for &(address, value) in &test.initial.ram {
            if address == physical_pc + 1 && value.count_ones() != 1 {
                return true;
            }
        }
    }

    // Skip block transfer tests that hit the JSON test RAM limits; too many caveats to be useful
    if BLOCK_TRANSFER_OPCODES.contains(&test.opcode) && test.final_.ram.len() >= 499 {
        return true;
    }

    // Skip SBC decimal mode tests that have invalid inputs (approximated by checking for invalid output)
    // Behavior is officially undefined and varies between different emulators
    if SBC_OPCODES.contains(&test.opcode) && Flags::from(test.initial.p).decimal {
        let result = test.final_.a;
        if result & 0x0F > 0x09 || result & 0xF0 > 0x90 {
            return true;
        }
    }

    false
}
