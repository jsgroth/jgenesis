mod load;

use crate::bus::{cartridge, Bus};
use crate::cpu;
use crate::cpu::{CpuRegisters, CpuState, StatusReadContext};
use std::collections::HashMap;

#[derive(Default)]
struct ExpectedState {
    a: Option<u8>,
    x: Option<u8>,
    y: Option<u8>,
    p: Option<u8>,
    s: Option<u8>,
    pc: Option<u16>,
    memory: HashMap<u16, u8>,
    cycles: Option<u32>,
}

macro_rules! assert_state_eq {
    ($(($name:literal, $expected:expr, $actual:expr)),+$(,)?) => {
        {
            let mut errors: Vec<String> = Vec::new();

            $(
                if let Some(expected) = $expected {
                    let actual = $actual;
                    if expected != actual {
                        errors.push(format!("[{} mismatch: expected = {:02X}, actual = {:02X}]", stringify!($name), expected, actual));
                    }
                }
            )*

            errors
        }
    }
}

impl ExpectedState {
    fn assert_eq(&self, cpu_registers: &CpuRegisters, bus: &mut Bus, cycle_count: u32) {
        let mut errors = assert_state_eq!(
            ("A", self.a, cpu_registers.accumulator),
            ("X", self.x, cpu_registers.x),
            ("Y", self.y, cpu_registers.y),
            (
                "P",
                self.p,
                cpu_registers.status.to_byte(StatusReadContext::PushStack)
            ),
            ("S", self.s, cpu_registers.sp),
            ("PC", self.pc, cpu_registers.pc),
            ("Cycles", self.cycles, cycle_count),
        );

        for (&address, &value) in &self.memory {
            let actual_value = bus.cpu().read_address(address);
            if value != actual_value {
                errors.push(format!("[Mismatch at memory address 0x{address:04X}: expected = 0x{value:02X}, actual = 0x{actual_value:02X}]"));
            }
        }

        if !errors.is_empty() {
            panic!("Expected state mismatch: {}", errors.join(", "));
        }
    }
}

fn run_test(program: &str, expected_state: ExpectedState) {
    let mut prg_rom = vec![0; 32768];
    // Set RESET vector to 0x8000
    prg_rom[32765] = 0x80;

    for (chunk, prg_byte) in program.as_bytes().chunks_exact(2).zip(prg_rom.iter_mut()) {
        let hex = String::from_utf8(Vec::from(chunk)).unwrap();
        let value = u8::from_str_radix(&hex, 16).unwrap();
        *prg_byte = value;
    }

    let mapper = cartridge::new_mmc1(prg_rom);

    let mut bus = Bus::from_cartridge(mapper);

    let mut cpu_state = CpuState::new(CpuRegisters::create(&mut bus.cpu()));

    let program_len = (program.len() / 2) as u16;
    let mut cycle_count = 0;
    while cpu_state.registers.pc < 0x8000 + program_len || !cpu_state.at_instruction_start() {
        cpu::tick(&mut cpu_state, &mut bus);
        bus.tick();

        cycle_count += 1;
    }

    expected_state.assert_eq(&cpu_state.registers, &mut bus, cycle_count);
}

macro_rules! hash_map {
    ($($key:literal: $value:expr),+$(,)?) => {
        {
            let mut map = std::collections::HashMap::new();

            $(
                map.insert($key, $value);
            )*

            map
        }
    }
}

use hash_map;
