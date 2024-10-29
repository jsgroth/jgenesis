//! Test runner for running tests assembled for CP/M, specifically ZEXDOC and ZEXALL.

use env_logger::Env;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::{env, fs, io, process};
use z80_emu::Z80;
use z80_emu::traits::{BusInterface, InterruptLine};

// This is an I/O routine that emulates the CP/M's $05 syscall, which does the following:
//
// If C=2, treat the contents of the E register as an ASCII code and print it
// If C=9, treat the contents of DE as a memory address, and continuously print ASCII characters
// starting from that address until a '$' character (36d / 0x24) is encountered
//
// Pure assembly version:
//
// PUSH AF
// PUSH DE
// LD A, C
// CP 2
// JR Z, singlechar
// CP 9
// JR Z, multichar
//
// return:
//   POP DE
//   POP AF
//   RET
//
// singlechar:
//   LD A, E
//   OUT (0), A
//   JR return
//
// multichar:
//   LD A, (DE)
//   CP 36
//   JR Z, return
//   OUT (0), A
//   INC DE
//   JR multichar
const CPM_IO_ROUTINE: &str = concat!(
    "F5",   // PUSH AF
    "D5",   // PUSH DE
    "79",   // LD A, C
    "FE02", // CP 2
    "2807", // JR Z, 7
    "FE09", // CP 9
    "2808", // JR Z, 8
    "D1",   // POP DE
    "F1",   // POP AF
    "C9",   // RET
    "7B",   // LD A, E
    "D300", // OUT (0), A
    "18F8", // JR -8
    "1A",   // LD A, (DE)
    "FE24", // CP 36
    "28F3", // JR Z, -13
    "D300", // OUT (0), A
    "13",   // INC DE
    "18F6", // JR -10
);

fn hex_to_bytes(s: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for i in (0..s.len()).step_by(2) {
        let hex_char = &s[i..i + 2];
        let byte = u8::from_str_radix(hex_char, 16).unwrap();
        bytes.push(byte);
    }
    bytes
}

struct FullyWritableBus {
    memory: Box<[u8; 0x10000]>,
    io_ports: [u8; 0x100],
}

impl FullyWritableBus {
    fn new() -> Self {
        Self {
            memory: vec![0; 0x10000].into_boxed_slice().try_into().unwrap(),
            io_ports: [0; 0x100],
        }
    }
}

impl BusInterface for FullyWritableBus {
    fn read_memory(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }

    fn read_io(&mut self, address: u16) -> u8 {
        self.io_ports[(address & 0xFF) as usize]
    }

    fn write_io(&mut self, address: u16, value: u8) {
        self.io_ports[(address & 0xFF) as usize] = value;
        print!("{}", value as char);
    }

    fn nmi(&self) -> InterruptLine {
        InterruptLine::High
    }

    fn int(&self) -> InterruptLine {
        InterruptLine::High
    }

    fn busreq(&self) -> bool {
        false
    }

    fn reset(&self) -> bool {
        false
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut args = env::args();
    if args.len() < 2 {
        io::stderr().write_all("ARGS: <filename>\n".as_bytes())?;
        process::exit(1);
    }
    args.next();

    let filename = args.next().unwrap();

    let bytes = fs::read(Path::new(&filename))?;

    let mut bus = FullyWritableBus::new();
    bus.memory[0x100..bytes.len() + 0x100].copy_from_slice(&bytes);

    let io_routine = hex_to_bytes(CPM_IO_ROUTINE);
    bus.memory[0x05..io_routine.len() + 0x05].copy_from_slice(&io_routine);

    let mut z80 = Z80::new();
    z80.set_pc(0x100);

    while z80.pc() != 0 {
        z80.execute_instruction(&mut bus);
    }

    Ok(())
}
