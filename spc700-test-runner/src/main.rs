use clap::Parser;
use env_logger::Env;
use serde::Deserialize;
use spc700_emu::traits::BusInterface;
use spc700_emu::{Registers, Spc700};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::{fs, process};

const RAM_LEN: usize = 1 << 16;

const SLEEP_OPCODE: u8 = 0xEF;
const STOP_OPCODE: u8 = 0xFF;
const STOP_TEST_CYCLES: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusOp {
    Read(u16, u8),
    Write(u16, u8),
    Idle,
}

impl Display for BusOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(address, value) => write!(f, "Read({address:04X}, {value:02X})"),
            Self::Write(address, value) => write!(f, "Write({address:04X}, {value:02X})"),
            Self::Idle => write!(f, "Idle"),
        }
    }
}

#[derive(Debug, Clone)]
struct RecordingBus {
    ram: Box<[u8; RAM_LEN]>,
    ops: Vec<BusOp>,
}

impl RecordingBus {
    fn new() -> Self {
        Self { ram: vec![0; RAM_LEN].into_boxed_slice().try_into().unwrap(), ops: Vec::new() }
    }

    fn clear(&mut self) {
        self.ops.clear();
    }
}

impl BusInterface for RecordingBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram[address as usize];
        self.ops.push(BusOp::Read(address, value));
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.ops.push(BusOp::Write(address, value));
        self.ram[address as usize] = value;
    }

    fn idle(&mut self) {
        self.ops.push(BusOp::Idle);
    }
}

#[derive(Debug, Clone, Deserialize)]
struct State {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    sp: u8,
    psw: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Debug, Clone, Deserialize)]
struct Cycle(Option<u16>, Option<u8>, String);

impl Cycle {
    fn to_bus_op(&self) -> BusOp {
        match (self.0, self.1, self.2.as_str()) {
            (None, _, _) | (_, None, _) | (_, _, "wait") => BusOp::Idle,
            (Some(address), Some(value), "read") => BusOp::Read(address, value),
            (Some(address), Some(value), "write") => BusOp::Write(address, value),
            _ => panic!("unexpected cycle descriptor string: {}", self.2),
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

#[derive(Debug, Clone, Parser)]
struct Args {
    #[arg(short = 'f', long)]
    file_path: Option<String>,
    #[arg(short = 'd', long)]
    directory_path: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    match (args.file_path, args.directory_path) {
        (Some(file_path), None) => {
            run_test(&file_path)?;
        }
        (None, Some(directory_path)) => {
            run_directory(&directory_path)?;
        }
        _ => {
            eprintln!(
                "ERROR: Exactly one of -f and -d must be set; use -h to see full help output"
            );
            process::exit(1);
        }
    }

    Ok(())
}

fn run_directory(directory_path: &str) -> Result<(), Box<dyn Error>> {
    let mut file_paths: Vec<_> = fs::read_dir(directory_path)?
        .filter_map(Result::ok)
        .filter_map(|dir_entry| {
            let path = dir_entry.path();
            (path.extension().and_then(OsStr::to_str) == Some("json")).then_some(path)
        })
        .collect();

    file_paths.sort();

    for file_path in file_paths {
        run_test(&file_path)?;
    }

    Ok(())
}

fn run_test<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn Error>> {
    let file_path = file_path.as_ref();

    let file = File::open(file_path)?;
    let test_descriptions: Vec<TestDescription> = serde_json::from_reader(BufReader::new(file))?;
    let num_tests = test_descriptions.len();

    let mut bus = RecordingBus::new();

    let mut failures = 0;
    for test_description in test_descriptions {
        let mut cpu = Spc700::new();

        init_test(&mut cpu, &mut bus, &test_description.initial);

        let opcode = bus.ram[test_description.initial.pc as usize];
        if opcode != SLEEP_OPCODE && opcode != STOP_OPCODE {
            // Run CPU for a full instruction
            cpu.tick(&mut bus);
            while cpu.is_mid_instruction() {
                cpu.tick(&mut bus);
            }
        } else {
            // SLEEP/STOP: Run CPU for a fixed number of cycles (7); the CPU should remain halted
            // after executing the instruction
            for _ in 0..STOP_TEST_CYCLES {
                cpu.tick(&mut bus);
            }
        }

        let errors = check_test(&cpu, &bus, &test_description.final_, &test_description.cycles);
        if !errors.is_empty() {
            failures += 1;

            log::error!("Failed test '{}':", test_description.name);
            for error in errors {
                log::error!("  {error}");
            }
        }

        bus.clear();
    }

    if failures != 0 {
        log::info!("Failed {failures} out of {num_tests} in '{}'", file_path.display());
    }

    Ok(())
}

macro_rules! check_registers {
    ($([$name:literal: $actual:expr, $expected:expr]),* $(,)?) => {
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

fn init_test(cpu: &mut Spc700, bus: &mut RecordingBus, state: &State) {
    cpu.set_registers(Registers {
        a: state.a,
        x: state.x,
        y: state.y,
        sp: state.sp,
        pc: state.pc,
        psw: state.psw.into(),
    });

    for &(address, value) in &state.ram {
        bus.ram[address as usize] = value;
    }
}

fn check_test(cpu: &Spc700, bus: &RecordingBus, state: &State, cycles: &[Cycle]) -> Vec<String> {
    let registers = cpu.registers();
    let mut errors = check_registers!(
        ["A": registers.a, state.a],
        ["X": registers.x, state.x],
        ["Y": registers.y, state.y],
        ["SP": registers.sp, state.sp],
        ["PC": registers.pc, state.pc],
        ["PSW": u8::from(registers.psw), state.psw],
    );

    for &(address, expected_value) in &state.ram {
        let actual_value = bus.ram[address as usize];
        if actual_value != expected_value {
            errors.push(format!(
                "RAM[{address:04X}]: actual={actual_value:02X}, expected={expected_value:02X}"
            ));
        }
    }

    let expected_ops: Vec<_> = cycles.iter().map(Cycle::to_bus_op).collect();
    if bus.ops.len() != expected_ops.len() {
        errors.push(format!(
            "Cycle count: actual={}, expected={}",
            bus.ops.len(),
            expected_ops.len()
        ));
    }

    for (i, (actual_op, expected_op)) in bus.ops.iter().zip(&expected_ops).enumerate() {
        if actual_op != expected_op {
            errors.push(format!("Cycle {i}: actual={actual_op}, expected={expected_op}"));
        }
    }

    errors
}
