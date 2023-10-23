mod bus;

use crate::bus::RecordingBus;
use clap::Parser;
use env_logger::Env;
use serde::Deserialize;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::{fs, process};
use wdc65816_emu::core::{Registers, Wdc65816};

const MVN_OPCODE: u8 = 0x44;
const MVP_OPCODE: u8 = 0x54;

#[derive(Debug, Clone, Deserialize)]
struct State {
    pc: u16,
    s: u16,
    p: u8,
    a: u16,
    x: u16,
    y: u16,
    dbr: u8,
    d: u16,
    pbr: u8,
    e: u8,
    ram: Vec<(u32, u8)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusOp {
    Read(u32, u8),
    Write(u32, u8),
    Idle,
}

impl Display for BusOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(address, value) => write!(f, "Read({address:06X}, {value:02X})"),
            Self::Write(address, value) => write!(f, "Write({address:06X}, {value:02X})"),
            Self::Idle => write!(f, "Idle"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Cycle(Option<u32>, Option<u8>, String);

impl Cycle {
    fn is_valid(&self) -> bool {
        // STP and WAI tests use null address to indicate that the CPU has halted
        self.0.is_some()
    }

    fn to_bus_op(&self) -> BusOp {
        match (self.0, self.1) {
            (Some(address), Some(value)) => {
                if self.2.as_bytes()[3] == b'r' {
                    BusOp::Read(address, value)
                } else {
                    BusOp::Write(address, value)
                }
            }
            _ => BusOp::Idle,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TestDescription {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    final_: State,
    cycles: Vec<Cycle>,
}

#[derive(Debug, Parser)]
struct Args {
    /// Path to .json test file
    #[arg(short = 'f', long)]
    file_path: Option<String>,
    /// Path to directory of .json test files
    #[arg(short = 'd', long)]
    directory_path: Option<String>,
    /// Suppress logging when no test cases fail
    #[arg(short = 's', long)]
    suppress_success_logs: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    match (args.file_path, args.directory_path) {
        (Some(file_path), None) => {
            let file = File::open(&file_path)?;
            let file_name = Path::new(&file_path).file_name().and_then(OsStr::to_str).unwrap();
            test_file(file, file_name, &mut RecordingBus::new(), args.suppress_success_logs)?;
        }
        (None, Some(directory_path)) => {
            test_directory(&directory_path, args.suppress_success_logs)?;
        }
        (Some(_), Some(_)) | (None, None) => {
            eprintln!("ERROR: Exactly one of -d and -f must be set; use -h to see help output");
            process::exit(1);
        }
    }

    Ok(())
}

fn test_directory(directory_path: &str, suppress_success_logs: bool) -> Result<(), Box<dyn Error>> {
    let mut files: Vec<_> = fs::read_dir(directory_path)?
        .filter_map(|dir_entry| {
            let dir_entry = dir_entry.ok()?;

            let path = dir_entry.path();
            (path.extension().and_then(OsStr::to_str) == Some("json")).then_some(path)
        })
        .collect();

    files.sort_by(|a, b| a.file_name().unwrap().cmp(b.file_name().unwrap()));

    let mut bus = RecordingBus::new();
    for file in files {
        let file_name = file.file_name().and_then(OsStr::to_str).unwrap();
        let file = File::open(&file)?;
        test_file(file, file_name, &mut bus, suppress_success_logs)?;
    }

    Ok(())
}

fn test_file<R: Read>(
    reader: R,
    file_name: &str,
    bus: &mut RecordingBus,
    suppress_success_logs: bool,
) -> Result<(), Box<dyn Error>> {
    let test_descriptions = parse_tests(reader)?;
    let num_tests = test_descriptions.len();

    let mut failures = 0;
    for test_description in test_descriptions {
        let mut wdc65816 = Wdc65816::new();
        init_test(&mut wdc65816, bus, &test_description.initial);

        // Execute a single full instruction
        let opcode_addr = (u32::from(test_description.initial.pbr) << 16)
            | u32::from(test_description.initial.pc);
        let opcode = bus.ram[opcode_addr as usize];
        if opcode != MVN_OPCODE && opcode != MVP_OPCODE {
            wdc65816.tick(bus);
            while wdc65816.is_mid_instruction() {
                wdc65816.tick(bus);
            }
        } else {
            // For MVN and MVP, the test suite expects the CPU to execute either until A reaches $FFFF
            // or until it has executed 100 cycles
            wdc65816.tick(bus);
            while bus.ops.len() < 100
                && (wdc65816.is_mid_instruction() || wdc65816.registers().a != 0xFFFF)
            {
                wdc65816.tick(bus);
            }
        }

        let errors = check_test(&wdc65816, bus, &test_description.final_, &test_description.cycles);

        if !errors.is_empty() {
            failures += 1;

            log::error!("Failed test '{}'", test_description.name);
            for error in errors {
                log::error!("  {error}");
            }
        }
    }

    if failures > 0 || !suppress_success_logs {
        log::info!("Failed {failures} out of {num_tests} in '{file_name}'");
    }

    Ok(())
}

fn parse_tests<R: Read>(reader: R) -> Result<Vec<TestDescription>, Box<dyn Error>> {
    let mut test_descriptions: Vec<TestDescription> =
        serde_json::from_reader(BufReader::new(reader))?;

    for test_description in &mut test_descriptions {
        test_description.cycles.retain(Cycle::is_valid);
    }

    Ok(test_descriptions)
}

fn init_test(wdc65816: &mut Wdc65816, bus: &mut RecordingBus, state: &State) {
    wdc65816.set_registers(Registers {
        a: state.a,
        x: state.x,
        y: state.y,
        s: state.s,
        d: state.d,
        pbr: state.pbr,
        pc: state.pc,
        dbr: state.dbr,
        p: state.p.into(),
        emulation_mode: state.e != 0,
    });

    bus.clear();
    for &(address, value) in &state.ram {
        bus.ram[address as usize] = value;
    }
}

macro_rules! check_registers {
    ($([$name:literal: $actual:expr, $expected:expr],)* $(,)?) => {
        {
            let mut errors: Vec<String> = Vec::new();

            $(
                let actual = $actual;
                let expected = $expected;
                if actual != expected {
                    errors.push(format!("{}: actual={actual:04X}, expected={expected:04X}", $name));
                }
            )*

            errors
        }
    }
}

fn check_test(
    wdc65816: &Wdc65816,
    bus: &RecordingBus,
    state: &State,
    cycles: &[Cycle],
) -> Vec<String> {
    let registers = wdc65816.registers();
    let mut errors = check_registers!(
        ["A": registers.a, state.a],
        ["X": registers.x, state.x],
        ["Y": registers.y, state.y],
        ["S": registers.s, state.s],
        ["D": registers.d, state.d],
        ["PBR": registers.pbr, state.pbr],
        ["PC": registers.pc, state.pc],
        ["DBR": registers.dbr, state.dbr],
        ["P": u8::from(registers.p), state.p],
        ["E": u8::from(registers.emulation_mode), state.e],
    );

    for &(address, expected) in &state.ram {
        let actual = bus.ram[address as usize];
        if actual != expected {
            errors
                .push(format!("RAM[{address:06X}]: actual={actual:02X}, expected={expected:02X}"));
        }
    }

    let expected_bus_ops: Vec<_> = cycles.iter().map(Cycle::to_bus_op).collect();
    if bus.ops.len() != expected_bus_ops.len() {
        errors.push(format!(
            "Cycle count: actual={}, expected={}",
            bus.ops.len(),
            expected_bus_ops.len()
        ));
    }

    for (i, (actual_op, expected_op)) in bus.ops.iter().zip(&expected_bus_ops).enumerate() {
        if actual_op != expected_op {
            errors.push(format!("Cycle {i}: actual={actual_op}, expected={expected_op}"));
        }
    }

    errors
}
