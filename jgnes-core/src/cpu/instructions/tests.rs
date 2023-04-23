use crate::bus::cartridge::{Cartridge, Mapper, NromMirroring};
use crate::bus::Bus;
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
                errors.push(format!("[Mismatch at memory address {address:04X}: expected = {value:02X}, actual = {actual_value:02X}]"));
            }
        }

        if !errors.is_empty() {
            panic!("Expected state mismatch: {}", errors.join(", "));
        }
    }
}

fn run_test(program: &str, expected_state: ExpectedState) {
    let mut prg_rom = vec![0; 16384];
    // Set RESET vector to 0x8000
    prg_rom[16381] = 0x80;

    for (chunk, prg_byte) in program.as_bytes().chunks_exact(2).zip(prg_rom.iter_mut()) {
        let hex = String::from_utf8(Vec::from(chunk)).unwrap();
        let value = u8::from_str_radix(&hex, 16).unwrap();
        *prg_byte = value;
    }

    let prg_rom_size = prg_rom.len() as u16;

    let cartridge = Cartridge {
        prg_rom,
        prg_ram: Vec::new(),
        chr_rom: vec![0; 8192],
        chr_ram: Vec::new(),
    };

    let mapper = Mapper::Nrom {
        prg_rom_size,
        nametable_mirroring: NromMirroring::Vertical,
    };

    let mut bus = Bus::from_cartridge(cartridge, mapper);

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

#[test]
fn lda_immediate() {
    run_test(
        // LDA #$78
        "A978",
        ExpectedState {
            a: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$DD
        "A9DD",
        ExpectedState {
            a: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$00
        "A900",
        ExpectedState {
            a: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn ldx_immediate() {
    run_test(
        // LDX #$78
        "A278",
        ExpectedState {
            x: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$DD
        "A2DD",
        ExpectedState {
            x: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$00
        "A200",
        ExpectedState {
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn ldy_immediate() {
    run_test(
        // LDY #$78
        "A078",
        ExpectedState {
            y: Some(0x78),
            p: Some(0x34),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$DD
        "A0DD",
        ExpectedState {
            y: Some(0xDD),
            p: Some(0xB4),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$00
        "A000",
        ExpectedState {
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(3),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tax() {
    run_test(
        // TAX
        "AA",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$45; TAX
        "A945AA",
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAX
        "A9CDAA",
        ExpectedState {
            a: Some(0xCD),
            x: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tay() {
    run_test(
        // TAY
        "A8",
        ExpectedState {
            a: Some(0x00),
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$45; TAY
        "A945A8",
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDA #$CD; TAY
        "A9CDA8",
        ExpectedState {
            a: Some(0xCD),
            y: Some(0xCD),
            p: Some(0xB4),
            cycles: Some(5),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn txs() {
    run_test(
        // TXS
        "9A",
        ExpectedState {
            x: Some(0x00),
            s: Some(0x00),
            p: Some(0x34),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$FF; LDA #$01; TXS
        "A2FFA9019A",
        ExpectedState {
            a: Some(0x01),
            x: Some(0xFF),
            s: Some(0xFF),
            p: Some(0x34),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tsx() {
    run_test(
        // TSX
        "BA",
        ExpectedState {
            x: Some(0xFD),
            s: Some(0xFD),
            p: Some(0xB4),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // TXS; TSX; LDX #$FF; TSX
        "9ABAA2FFBA",
        ExpectedState {
            x: Some(0x00),
            s: Some(0x00),
            p: Some(0x36),
            cycles: Some(9),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn txa() {
    run_test(
        // TXA
        "8A",
        ExpectedState {
            a: Some(0x00),
            x: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$45; LDA #$00; TXA
        "A245A9008A",
        ExpectedState {
            a: Some(0x45),
            x: Some(0x45),
            p: Some(0x34),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDX #$EE; LDA #$00; TXA
        "A2EEA9008A",
        ExpectedState {
            a: Some(0xEE),
            x: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}

#[test]
fn tya() {
    run_test(
        // TYA
        "98",
        ExpectedState {
            a: Some(0x00),
            y: Some(0x00),
            p: Some(0x36),
            cycles: Some(2),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$45; LDA #$00; TYA
        "A045A90098",
        ExpectedState {
            a: Some(0x45),
            y: Some(0x45),
            p: Some(0x34),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );

    run_test(
        // LDY #$EE; LDA #$00; TYA
        "A0EEA90098",
        ExpectedState {
            a: Some(0xEE),
            y: Some(0xEE),
            p: Some(0xB4),
            cycles: Some(8),
            ..ExpectedState::default()
        },
    );
}
