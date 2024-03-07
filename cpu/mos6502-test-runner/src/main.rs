use clap::Parser;
use env_logger::Env;
use mos6502_emu::bus::BusInterface;
use mos6502_emu::{CpuRegisters, Mos6502, StatusFlags, StatusReadContext};
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::mem;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusCycle {
    Read(u16, u8),
    Write(u16, u8),
}

struct Bus {
    ram: Vec<u8>,
    addresses_written: Vec<u16>,
    cycles: Vec<BusCycle>,
}

impl Bus {
    fn new() -> Self {
        Self { ram: vec![0; 64 * 1024], addresses_written: Vec::new(), cycles: Vec::new() }
    }

    fn clear(&mut self) {
        for address in mem::take(&mut self.addresses_written) {
            self.ram[address as usize] = 0;
        }
        self.cycles.clear();
    }
}

impl BusInterface for Bus {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        let value = self.ram[address as usize];
        self.cycles.push(BusCycle::Read(address, value));
        value
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.ram[address as usize] = value;
        self.addresses_written.push(address);
        self.cycles.push(BusCycle::Write(address, value));
    }

    #[inline]
    fn nmi(&self) -> bool {
        false
    }

    #[inline]
    fn acknowledge_nmi(&mut self) {}

    #[inline]
    fn irq(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SystemState {
    pc: u16,
    s: u8,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Debug, Clone, Deserialize)]
struct Cycle(u16, u8, String);

impl Cycle {
    fn to_bus_cycle(&self) -> BusCycle {
        match self.2.as_str() {
            "read" => BusCycle::Read(self.0, self.1),
            "write" => BusCycle::Write(self.0, self.1),
            _ => panic!("Invalid bus cycle type, expected read/write: {}", self.2),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TestDescription {
    name: String,
    initial: SystemState,
    #[serde(rename = "final")]
    final_: SystemState,
    cycles: Vec<Cycle>,
}

#[derive(Debug, Parser)]
struct Args {
    /// Directory containing JSON tests
    #[arg(long, short = 'd')]
    dir_path: String,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let mut bus = Bus::new();

    for opcode in 0x00..=0xFF {
        let file_path = Path::new(&args.dir_path).join(&format!("{opcode:02x}.json"));
        let tests: Vec<TestDescription> =
            serde_json::from_reader(BufReader::new(File::open(&file_path)?))?;

        let mut failures = 0;
        let test_count = tests.len();
        for test in tests {
            bus.clear();
            for &(address, value) in &test.initial.ram {
                bus.write(address, value);
            }

            let mut cpu = Mos6502::new(&mut bus);
            cpu.set_registers(CpuRegisters {
                accumulator: test.initial.a,
                x: test.initial.x,
                y: test.initial.y,
                status: StatusFlags::from_byte(test.initial.p),
                pc: test.initial.pc,
                sp: test.initial.s,
            });

            bus.cycles.clear();
            cpu.tick(&mut bus);
            while cpu.is_mid_instruction() && !cpu.frozen() {
                cpu.tick(&mut bus);
            }

            if cpu.frozen() {
                // Don't bother testing KIL opcodes
                continue;
            }

            if check_state(&cpu, &bus, &test.final_, &test.cycles) {
                failures += 1;
                log::debug!("Above failures in '{}'", test.name);
            }
        }

        if failures != 0 {
            log::error!("Failed {failures} out of {test_count} tests for opcode {opcode:02X}");
        }
    }

    Ok(())
}

fn check_state(cpu: &Mos6502, bus: &Bus, final_state: &SystemState, cycles: &[Cycle]) -> bool {
    let mut errors = false;

    for &(address, expected_value) in &final_state.ram {
        let actual_value = bus.ram[address as usize];
        if expected_value != actual_value {
            errors = true;
            log::debug!(
                "RAM[{address:04X}]: expected={expected_value:02X}, actual={actual_value:02X}"
            );
        }
    }

    let registers = cpu.registers();
    errors |= check_register("A", final_state.a, registers.accumulator);
    errors |= check_register("X", final_state.x, registers.x);
    errors |= check_register("Y", final_state.y, registers.y);
    errors |= check_register("S", final_state.s, registers.sp);
    errors |= check_register(
        "P",
        final_state.p | 0x10,
        registers.status.to_byte(StatusReadContext::Brk) | 0x10,
    );

    if final_state.pc != registers.pc {
        log::debug!("PC: expected={:04X} actual={:04X}", final_state.pc, registers.pc);
        errors = true;
    }

    if cycles.len() != bus.cycles.len() {
        log::debug!(
            "Cycle count does not match: expected={}, actual={}",
            cycles.len(),
            bus.cycles.len()
        );
        log::debug!("  Expected: {cycles:?}");
        log::debug!("  Actual: {:?}", bus.cycles);
        errors = true;
    } else {
        for (i, (expected, &actual)) in cycles.iter().zip(&bus.cycles).enumerate() {
            let expected = expected.to_bus_cycle();
            if expected != actual {
                log::debug!("Cycle {i} mismatch: expected={expected:?}, actual={actual:?}");
                errors = true;
            }
        }
    }

    errors
}

fn check_register(name: &str, expected: u8, actual: u8) -> bool {
    if expected != actual {
        log::debug!("{name}: expected={expected:02X}, actual={actual:02X}");
        true
    } else {
        false
    }
}
