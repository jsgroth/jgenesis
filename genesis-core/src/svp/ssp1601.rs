use crate::svp::{PmcWaitingFor, StatusRegister, Svp};
use jgenesis_traits::num::GetBit;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AluOp {
    Add,
    Subtract,
    Compare,
    And,
    Or,
    ExclusiveOr,
}

impl AluOp {
    fn from_opcode(opcode: u16) -> Option<Self> {
        match opcode & 0xE000 {
            0x2000 => Some(Self::Subtract),
            0x6000 => Some(Self::Compare),
            0x8000 => Some(Self::Add),
            0xA000 => Some(Self::And),
            0xC000 => Some(Self::Or),
            0xE000 => Some(Self::ExclusiveOr),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccumulateOp {
    Zero,
    Add,
    Subtract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Condition {
    True,
    Zero,
    NotZero,
    Negative,
    NotNegative,
}

impl Condition {
    fn from_opcode(opcode: u16) -> Self {
        match opcode & 0x01F0 {
            0x0000 => Self::True,
            0x0050 => Self::NotZero,
            0x0150 => Self::Zero,
            0x0070 => Self::NotNegative,
            0x0170 => Self::Negative,
            _ => panic!("Invalid SVP opcode (invalid condition): {opcode:04X}"),
        }
    }

    fn check(self, status: StatusRegister) -> bool {
        match self {
            Self::True => true,
            Self::Zero => status.zero,
            Self::NotZero => !status.zero,
            Self::Negative => status.negative,
            Self::NotNegative => !status.negative,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RamBank {
    Zero,
    One,
}

impl RamBank {
    fn from_opcode(opcode: u16) -> Self {
        if opcode.bit(8) { Self::One } else { Self::Zero }
    }
}

impl Display for RamBank {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero => write!(f, "RAM0"),
            Self::One => write!(f, "RAM1"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressingMode {
    // d / s
    GeneralRegister(u16),
    // ri / rj
    PointerRegister(RamBank, u16),
    // (ri) / (rj)
    Indirect { bank: RamBank, pointer: u16, modifier: u16 },
    // ((ri)) / ((rj))
    DoubleIndirect { bank: RamBank, pointer: u16, modifier: u16 },
    // addr
    Direct { bank: RamBank, address: u8 },
    // imm
    Immediate,
    // simm
    ShortImmediate(u8),
    // (A)
    AccumulatorIndirect,
}

impl AddressingMode {
    const ACCUMULATOR_REGISTER: Self = Self::GeneralRegister(3);
}

impl Display for AddressingMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GeneralRegister(0) => write!(f, "NULL"),
            Self::GeneralRegister(1) => write!(f, "X"),
            Self::GeneralRegister(2) => write!(f, "Y"),
            Self::GeneralRegister(3) => write!(f, "AH"),
            Self::GeneralRegister(4) => write!(f, "ST"),
            Self::GeneralRegister(5) => write!(f, "STACK"),
            Self::GeneralRegister(6) => write!(f, "PC"),
            Self::GeneralRegister(7) => write!(f, "P"),
            Self::GeneralRegister(8) => write!(f, "PM0/XSTStatus"),
            Self::GeneralRegister(9) => write!(f, "PM1"),
            Self::GeneralRegister(10) => write!(f, "PM2"),
            Self::GeneralRegister(11) => write!(f, "PM3/XST"),
            Self::GeneralRegister(12) => write!(f, "PM4"),
            Self::GeneralRegister(14) => write!(f, "PMC"),
            Self::GeneralRegister(15) => write!(f, "AL"),
            Self::GeneralRegister(_) => write!(f, "(invalid register)"),
            Self::PointerRegister(bank, pointer) => write!(f, "RIJ[{bank}, {pointer}]"),
            Self::Indirect { bank, pointer, modifier } => {
                write!(f, "(RIJ)[{bank}, {pointer}]/mod={modifier}")
            }
            Self::DoubleIndirect { bank, pointer, modifier } => {
                write!(f, "((RIJ))[{bank}, {pointer}]/mod={modifier}")
            }
            Self::Direct { bank, address } => write!(f, "{bank}[{address:02X}]"),
            Self::Immediate => write!(f, "#<imm>"),
            Self::ShortImmediate(value) => write!(f, "#<{value:02X}>"),
            Self::AccumulatorIndirect => write!(f, "(AH)"),
        }
    }
}

pub fn execute_instruction(svp: &mut Svp, rom: &[u8]) {
    let opcode = fetch_operand(svp, rom);

    log::trace!("PC={:04X}, opcode={opcode:04X}", svp.registers.pc.wrapping_sub(1));

    // The first 8 bits are enough to distinguish between all opcodes
    match opcode & 0xFF00 {
        0x0000 => {
            // ld d, s
            ld_d_s(svp, rom, opcode);
        }
        0x0200 | 0x0300 => {
            // ld d, (ri)
            ld_d_ri_indirect(svp, rom, opcode);
        }
        0x0400 | 0x0500 => {
            // ld (ri), s
            ld_ri_s_indirect(svp, rom, opcode);
        }
        0x0600 | 0x0700 => {
            // ld A, addr
            ld_a_addr(svp, rom, opcode);
        }
        0x0800 => {
            // ldi d, imm
            ldi_d_imm(svp, rom, opcode);
        }
        0x0A00 | 0x0B00 => {
            // ld d, ((ri))
            ld_d_ri_double_indirect(svp, rom, opcode);
        }
        0x0C00 | 0x0D00 => {
            // ldi (ri), imm
            ldi_ri_imm(svp, rom, opcode);
        }
        0x0E00 | 0x0F00 => {
            // ld addr, A
            ld_addr_a(svp, rom, opcode);
        }
        0x1200 | 0x1300 => {
            // ld d, ri
            ld_d_ri(svp, rom, opcode);
        }
        0x1400 | 0x1500 => {
            // ld ri, s
            ld_ri_s(svp, rom, opcode);
        }
        0x1800..=0x1F00 => {
            // ldi ri, simm
            ldi_ri_simm(svp, rom, opcode);
        }
        0x3700 => {
            // mpys (rj), (ri)
            execute_multiply_accumulate(svp, opcode, AccumulateOp::Subtract);
        }
        0x4800 | 0x4900 => {
            // call cond, addr
            execute_call(svp, rom, opcode);
        }
        0x4A00 => {
            // ld d, (a)
            ld_d_a_indirect(svp, rom, opcode);
        }
        0x4C00 | 0x4D00 => {
            // bra cond, addr
            execute_bra(svp, rom, opcode);
        }
        0x9000 | 0x9100 => {
            // mod cond, op
            execute_mod(svp, opcode);
        }
        0x9700 => {
            // mpya (rj), (ri)
            execute_multiply_accumulate(svp, opcode, AccumulateOp::Add);
        }
        0xB700 => {
            // mld (rj), (ri)
            execute_multiply_accumulate(svp, opcode, AccumulateOp::Zero);
        }
        0xFF00 => {
            // Treat as a no-op; do nothing
        }
        _ => {
            // ALU ops; highest 3 bits determine op, next 5 bits determine addressing mode
            execute_alu(svp, rom, opcode);
        }
    }
}

fn fetch_operand(svp: &mut Svp, rom: &[u8]) -> u16 {
    let operand = svp.read_program_memory(svp.registers.pc, rom);
    svp.registers.pc = svp.registers.pc.wrapping_add(1);
    operand
}

fn execute_load(svp: &mut Svp, rom: &[u8], source: AddressingMode, dest: AddressingMode) {
    log::trace!("  LD source={source}, dest={dest}");

    match (source, dest) {
        (AddressingMode::GeneralRegister(7), AddressingMode::GeneralRegister(3)) => {
            // P to A; copy all 32 bits
            svp.registers.accumulator = svp.registers.product();
            log::trace!("  A = {:08X}", svp.registers.accumulator);
        }
        (
            AddressingMode::GeneralRegister(0),
            AddressingMode::GeneralRegister(register @ 8..=15),
        ) => {
            // Blind write; program PM register (if applicable) and reset PMC state
            log::trace!("Blind write to register {register}");

            if register <= 12 {
                let pm_idx = (register - 8) as usize;
                svp.registers.pm_write[pm_idx]
                    .initialize(svp.registers.pmc.address, svp.registers.pmc.mode);

                log::trace!(
                    "Initialized PM{pm_idx} for writes: {:X?}",
                    svp.registers.pm_write[pm_idx]
                );
            }

            if register != 14 {
                svp.registers.pmc.waiting_for = PmcWaitingFor::Address;
            } else {
                // Blind writes to PMC toggle state instead of resetting
                svp.registers.pmc.waiting_for = svp.registers.pmc.waiting_for.toggle();
            }
        }
        (
            AddressingMode::GeneralRegister(register @ 8..=15),
            AddressingMode::GeneralRegister(0),
        ) => {
            // Blind read; program PM register (if applicable) and reset PMC state
            log::trace!("Blind read to register {register}");

            if register <= 12 {
                let pm_idx = (register - 8) as usize;
                svp.registers.pm_read[pm_idx]
                    .initialize(svp.registers.pmc.address, svp.registers.pmc.mode);

                log::trace!(
                    "Initialized PM{pm_idx} for reads: {:X?}",
                    svp.registers.pm_read[pm_idx]
                );
            }

            if register != 14 {
                svp.registers.pmc.waiting_for = PmcWaitingFor::Address;
            } else {
                // Blind reads to PMC toggle state instead of resetting
                svp.registers.pmc.waiting_for = svp.registers.pmc.waiting_for.toggle();
            }
        }
        _ => {
            // Normal 16-bit load
            let value = read_addressing_mode(svp, rom, source);
            write_addressing_mode(svp, dest, value);

            log::trace!("  Wrote value {value:04X}");
        }
    }
}

fn execute_alu(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let Some(op) = AluOp::from_opcode(opcode) else {
        panic!("Invalid SSP1601 opcode: {opcode:04X}");
    };

    let source = parse_alu_addressing_mode(opcode);

    log::trace!("  ALU op={op:?}, source={source}");

    // ALU operations are 32-bit, but most sources are 16-bit.
    // If A or P is the source, use all 32 bits; otherwise shift the 16-bit value into the high
    // word of a 32-bit value
    let operand = match source {
        AddressingMode::GeneralRegister(3) => svp.registers.accumulator,
        AddressingMode::GeneralRegister(7) => svp.registers.product(),
        _ => u32::from(read_addressing_mode(svp, rom, source)) << 16,
    };

    let accumulator = svp.registers.accumulator;
    let result = match op {
        AluOp::Add => accumulator.wrapping_add(operand),
        AluOp::Subtract | AluOp::Compare => accumulator.wrapping_sub(operand),
        AluOp::And => accumulator & operand,
        AluOp::Or => accumulator | operand,
        AluOp::ExclusiveOr => accumulator ^ operand,
    };

    update_flags(svp, result);
    if op != AluOp::Compare {
        svp.registers.accumulator = result;
    }

    log::trace!("  ALU result {result:08X}");
}

fn update_flags(svp: &mut Svp, accumulator: u32) {
    svp.registers.status.zero = accumulator == 0;
    svp.registers.status.negative = accumulator.bit(31);
}

fn execute_mod(svp: &mut Svp, opcode: u16) {
    let condition = Condition::from_opcode(opcode);
    if !condition.check(svp.registers.status) {
        log::trace!("  MODIFY cond={condition:?}, condition false");
        return;
    }

    // Lowest 3 bits determine operation
    match opcode & 0x0007 {
        0x0002 => {
            // Arithmetic right shift
            log::trace!("  ASR cond={condition:?}");

            svp.registers.accumulator = ((svp.registers.accumulator as i32) >> 1) as u32;
        }
        0x0003 => {
            // Left shift
            log::trace!("  SL cond={condition:?}");

            svp.registers.accumulator <<= 1;
        }
        0x0006 => {
            // Negate
            log::trace!("  NEG cond={condition:?}");

            svp.registers.accumulator = (!svp.registers.accumulator).wrapping_add(1);
        }
        0x0007 => {
            // Absolute value
            log::trace!("  ABS cond={condition:?}");

            if svp.registers.accumulator.bit(31) {
                svp.registers.accumulator = (!svp.registers.accumulator).wrapping_add(1);
            }
        }
        _ => panic!("Invalid SVP opcode: {opcode:04X}"),
    }

    update_flags(svp, svp.registers.accumulator);
}

fn execute_call(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let address = fetch_operand(svp, rom);
    let condition = Condition::from_opcode(opcode);

    log::trace!("  CALL cond={condition:?}, address={address:04X}");

    if condition.check(svp.registers.status) {
        svp.registers.stack.push(svp.registers.pc);
        svp.registers.pc = address;
    }
}

fn execute_bra(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let address = fetch_operand(svp, rom);
    let condition = Condition::from_opcode(opcode);

    log::trace!("  BRA cond={condition:?}, address={address:04X}");

    if condition.check(svp.registers.status) {
        svp.registers.pc = address;
    }
}

fn execute_multiply_accumulate(svp: &mut Svp, opcode: u16, op: AccumulateOp) {
    // Accumulation is performed before changing the multiply result via X and Y
    match op {
        AccumulateOp::Zero => {
            svp.registers.accumulator = 0;
        }
        AccumulateOp::Add => {
            svp.registers.accumulator =
                svp.registers.accumulator.wrapping_add(svp.registers.product());
        }
        AccumulateOp::Subtract => {
            svp.registers.accumulator =
                svp.registers.accumulator.wrapping_sub(svp.registers.product());
        }
    }

    update_flags(svp, svp.registers.accumulator);

    // X is always set to a value from RAM0
    let x_pointer = opcode & 0x03;
    let x_modifier = (opcode >> 2) & 0x03;
    let ram0_addr = read_pointer(svp, RamBank::Zero, x_pointer, x_modifier);
    svp.registers.x = svp.ram0[ram0_addr as usize];

    // Y is always set to a value from RAM1
    let y_pointer = (opcode >> 4) & 0x03;
    let y_modifier = (opcode >> 6) & 0x03;
    let ram1_addr = read_pointer(svp, RamBank::One, y_pointer, y_modifier);
    svp.registers.y = svp.ram1[ram1_addr as usize];

    log::trace!(
        "  MUL op={op:?}, X=RAM0[p{x_pointer}]/mod={x_modifier}, Y=RAM1[p{y_pointer}]/mod={y_modifier}"
    );
    log::trace!("  A={:08X}, P={:08X}", svp.registers.accumulator, svp.registers.product());
}

fn ld_d_s(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let source = AddressingMode::GeneralRegister(opcode & 0xF);
    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);
    execute_load(svp, rom, source, dest);
}

fn ld_d_ri(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let source = AddressingMode::PointerRegister(bank, pointer);

    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);

    execute_load(svp, rom, source, dest);
}

fn ld_ri_s(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let dest = AddressingMode::PointerRegister(bank, pointer);

    let source = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);

    execute_load(svp, rom, source, dest);
}

fn ld_d_ri_indirect(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let modifier = (opcode >> 2) & 0x03;
    let source = AddressingMode::Indirect { bank, pointer, modifier };

    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);

    execute_load(svp, rom, source, dest);
}

fn ld_ri_s_indirect(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let modifier = (opcode >> 2) & 0x03;
    let dest = AddressingMode::Indirect { bank, pointer, modifier };

    let source = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);

    execute_load(svp, rom, source, dest);
}

fn ld_d_ri_double_indirect(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let modifier = (opcode >> 2) & 0x03;
    let source = AddressingMode::DoubleIndirect { bank, pointer, modifier };

    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);

    execute_load(svp, rom, source, dest);
}

fn ld_a_addr(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let address = opcode as u8;
    let source = AddressingMode::Direct { bank, address };

    let dest = AddressingMode::ACCUMULATOR_REGISTER;

    execute_load(svp, rom, source, dest);
}

fn ld_addr_a(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let address = opcode as u8;
    let dest = AddressingMode::Direct { bank, address };

    let source = AddressingMode::ACCUMULATOR_REGISTER;

    execute_load(svp, rom, source, dest);
}

fn ldi_d_imm(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let source = AddressingMode::Immediate;
    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);
    execute_load(svp, rom, source, dest);
}

fn ldi_ri_imm(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let bank = RamBank::from_opcode(opcode);
    let pointer = opcode & 0x03;
    let modifier = (opcode >> 2) & 0x03;
    let dest = AddressingMode::Indirect { bank, pointer, modifier };

    let source = AddressingMode::Immediate;

    execute_load(svp, rom, source, dest);
}

fn ldi_ri_simm(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let source = AddressingMode::ShortImmediate(opcode as u8);

    let bank = if opcode.bit(10) { RamBank::One } else { RamBank::Zero };
    let pointer = (opcode >> 8) & 0x03;
    let dest = AddressingMode::PointerRegister(bank, pointer);

    execute_load(svp, rom, source, dest);
}

fn ld_d_a_indirect(svp: &mut Svp, rom: &[u8], opcode: u16) {
    let source = AddressingMode::AccumulatorIndirect;
    let dest = AddressingMode::GeneralRegister((opcode >> 4) & 0xF);
    execute_load(svp, rom, source, dest);
}

fn parse_alu_addressing_mode(opcode: u16) -> AddressingMode {
    match opcode & 0x1F00 {
        0x0000 => {
            // OP A, s
            AddressingMode::GeneralRegister(opcode & 0xF)
        }
        0x0200 | 0x0300 => {
            // OP A, (ri)
            let bank = RamBank::from_opcode(opcode);
            let pointer = opcode & 0x03;
            let modifier = (opcode >> 2) & 0x03;
            AddressingMode::Indirect { bank, pointer, modifier }
        }
        0x0600 | 0x0700 => {
            // OP A, addr
            let bank = RamBank::from_opcode(opcode);
            let address = opcode as u8;
            AddressingMode::Direct { bank, address }
        }
        0x0800 => {
            // OPi A, imm
            AddressingMode::Immediate
        }
        0x0A00 | 0x0B00 => {
            // OP A, ((ri))
            let bank = RamBank::from_opcode(opcode);
            let pointer = opcode & 0x03;
            let modifier = (opcode >> 2) & 0x03;
            AddressingMode::DoubleIndirect { bank, pointer, modifier }
        }
        0x1200 | 0x1300 => {
            // OP A, ri
            let bank = RamBank::from_opcode(opcode);
            let pointer = opcode & 0x03;
            AddressingMode::PointerRegister(bank, pointer)
        }
        0x1800 => {
            // OPi A, simm
            let immediate_value = opcode as u8;
            AddressingMode::ShortImmediate(immediate_value)
        }
        _ => panic!("Invalid SVP opcode: {opcode:04X}"),
    }
}

fn read_addressing_mode(svp: &mut Svp, rom: &[u8], source: AddressingMode) -> u16 {
    match source {
        AddressingMode::GeneralRegister(register) => read_register(svp, rom, register),
        AddressingMode::PointerRegister(bank, register) => match (bank, register) {
            (RamBank::Zero, 0..=2) => svp.registers.ram0_pointers[register as usize].into(),
            (RamBank::One, 0..=2) => svp.registers.ram1_pointers[register as usize].into(),
            (RamBank::Zero | RamBank::One, 3) => 0,
            _ => panic!("invalid pointer register: {register}"),
        },
        AddressingMode::Indirect { bank, pointer, modifier } => {
            // Read a value from internal RAM using a pointer register
            let ram_addr = read_pointer(svp, bank, pointer, modifier);
            match bank {
                RamBank::Zero => svp.ram0[ram_addr as usize],
                RamBank::One => svp.ram1[ram_addr as usize],
            }
        }
        AddressingMode::DoubleIndirect { bank, pointer, modifier } => {
            // Read a value from program memory, with the address determined by an indirect read
            // The address stored in internal RAM is incremented after the read
            let ram_addr = read_pointer(svp, bank, pointer, modifier);
            let ram = match bank {
                RamBank::Zero => &mut svp.ram0,
                RamBank::One => &mut svp.ram1,
            };

            let indirect_addr = ram[ram_addr as usize];
            ram[ram_addr as usize] = indirect_addr.wrapping_add(1);

            log::trace!("  Indirect addr = {indirect_addr:04X}");

            svp.read_program_memory(indirect_addr, rom)
        }
        AddressingMode::Direct { bank, address } => match bank {
            // Read a value from internal RAM using a direct address
            RamBank::Zero => svp.ram0[address as usize],
            RamBank::One => svp.ram1[address as usize],
        },
        AddressingMode::Immediate => {
            // 16-bit immediate value, specified in the next word in program memory
            let value = fetch_operand(svp, rom);
            log::trace!("  Immediate value: {value:04X}");
            value
        }
        AddressingMode::ShortImmediate(value) => {
            // 8-bit immediate value embedded in the opcode, zero extended to 16 bits
            value.into()
        }
        AddressingMode::AccumulatorIndirect => {
            // Read program memory, using the high word of the accumulator as the address
            let address = (svp.registers.accumulator >> 16) as u16;
            svp.read_program_memory(address, rom)
        }
    }
}

fn write_addressing_mode(svp: &mut Svp, dest: AddressingMode, value: u16) {
    match dest {
        AddressingMode::GeneralRegister(register) => {
            write_register(svp, register, value);
        }
        AddressingMode::PointerRegister(bank, pointer) => {
            // Pointer registers 3/7 are not writable
            if pointer < 3 {
                match bank {
                    RamBank::Zero => {
                        svp.registers.ram0_pointers[pointer as usize] = value as u8;
                    }
                    RamBank::One => {
                        svp.registers.ram1_pointers[pointer as usize] = value as u8;
                    }
                }
            }
        }
        AddressingMode::Indirect { bank, pointer, modifier } => {
            // Write a value to internal RAM using a pointer register
            let ram_addr = read_pointer(svp, bank, pointer, modifier);
            match bank {
                RamBank::Zero => {
                    svp.ram0[ram_addr as usize] = value;
                }
                RamBank::One => {
                    svp.ram1[ram_addr as usize] = value;
                }
            }
        }
        AddressingMode::Direct { bank, address } => match bank {
            // Write a value to internal RAM using a direct address
            RamBank::Zero => {
                svp.ram0[address as usize] = value;
            }
            RamBank::One => {
                svp.ram1[address as usize] = value;
            }
        },
        AddressingMode::DoubleIndirect { .. }
        | AddressingMode::Immediate
        | AddressingMode::ShortImmediate(..)
        | AddressingMode::AccumulatorIndirect => panic!("Invalid write addressing mode: {dest:?}"),
    }
}

#[allow(clippy::match_same_arms)]
fn read_register(svp: &mut Svp, rom: &[u8], register: u16) -> u16 {
    match register {
        0 => {
            // Dummy/null register; always reads $FFFF
            0xFFFF
        }
        1 => {
            // X register
            svp.registers.x
        }
        2 => {
            // Y register
            svp.registers.y
        }
        3 => {
            // Accumulator, high word
            (svp.registers.accumulator >> 16) as u16
        }
        4 => {
            // Status register
            svp.registers.status.into()
        }
        5 => {
            // Stack register; reads pop
            svp.registers.stack.pop()
        }
        6 => {
            // PC
            svp.registers.pc
        }
        7 => {
            // P register
            (svp.registers.product() >> 16) as u16
        }
        8 => {
            // PM0 / XST status
            if svp.registers.status.st_bits_set() {
                // PM0
                pm_read(svp, rom, 0)
            } else {
                // XST status
                svp.registers.xst.ssp_read_status()
            }
        }
        9 => {
            // PM1
            pm_read(svp, rom, 1)
        }
        10 => {
            // PM2
            pm_read(svp, rom, 2)
        }
        11 => {
            // PM3 / XST register
            if svp.registers.status.st_bits_set() {
                // PM3
                pm_read(svp, rom, 3)
            } else {
                // XST register
                svp.registers.xst.value
            }
        }
        12 => {
            // PM4
            pm_read(svp, rom, 4)
        }
        13 => {
            // Unknown; unused by SVP code
            0xFFFF
        }
        14 => {
            // PMC register
            log::trace!("PMC read");
            svp.registers.pmc.read()
        }
        15 => {
            // Accumulator, low word
            svp.registers.accumulator as u16
        }
        _ => panic!("Invalid SVP register number: {register}"),
    }
}

#[allow(clippy::match_same_arms)]
fn write_register(svp: &mut Svp, register: u16, value: u16) {
    match register {
        0 => {
            // Dummy/null register; writes do nothing
        }
        1 => {
            // X register
            svp.registers.x = value;
        }
        2 => {
            // Y register
            svp.registers.y = value;
        }
        3 => {
            // Accumulator, high word
            svp.registers.accumulator =
                (svp.registers.accumulator & 0x0000_FFFF) | (u32::from(value) << 16);
        }
        4 => {
            // Status register
            svp.registers.status.write(value);
        }
        5 => {
            // Stack; writes push
            svp.registers.stack.push(value);
        }
        6 => {
            // PC
            svp.registers.pc = value;
        }
        7 => {
            // P register; writes do nothing because P always equals 2 * X * Y
        }
        8 => {
            // PM0 / XST status
            if svp.registers.status.st_bits_set() {
                // PM0
                pm_write(svp, 0, value);
            } else {
                // XST status; is this even writable? SVP code seems to write to it
                svp.registers.xst.m68k_written = value.bit(1);
                svp.registers.xst.ssp_written = value.bit(0);
            }
        }
        9 => {
            // PM1
            pm_write(svp, 1, value);
        }
        10 => {
            // PM2
            pm_write(svp, 2, value);
        }
        11 => {
            // PM3 / XST register
            if svp.registers.status.st_bits_set() {
                // PM3
                pm_write(svp, 3, value);
            } else {
                // XST register
                svp.registers.xst.ssp_write(value);
            }
        }
        12 => {
            // PM4
            pm_write(svp, 4, value);
        }
        13 => {
            // Unknown; unused by SVP
        }
        14 => {
            // PMC
            log::trace!("PMC write: {value:04X}");

            svp.registers.pmc.write(value);
        }
        15 => {
            // Accumulator, low word
            svp.registers.accumulator =
                (svp.registers.accumulator & 0xFFFF_0000) | u32::from(value);
        }
        _ => panic!("Invalid SVP register number: {register}"),
    }
}

fn pm_read(svp: &mut Svp, rom: &[u8], pm_idx: usize) -> u16 {
    log::trace!("PM{pm_idx} read");

    let pm_register = &mut svp.registers.pm_read[pm_idx];
    let address = pm_register.get_and_increment_address();

    // Reading a PM register always updates the address/mode words stored in PMC
    svp.registers.pmc.update_from(pm_register);

    svp.read_external_memory(address, rom)
}

fn pm_write(svp: &mut Svp, pm_idx: usize, value: u16) {
    log::trace!("PM{pm_idx} write {value:04X}");

    let pm_register = &mut svp.registers.pm_write[pm_idx];
    let address = pm_register.get_and_increment_address();
    let overwrite_mode = pm_register.overwrite_mode;

    // Writing a PM register always updates the address/mode words stored in PMC
    svp.registers.pmc.update_from(pm_register);

    if overwrite_mode {
        // This is a read-modify-write, so verify that the SSP isn't attempting to write to ROM
        if !(0x000000..=0x0FFFFF).contains(&address) {
            let existing_value = svp.read_external_memory(address, &[]);

            // Overwrite mode splits words into 4 nibbles and only writes the non-0 nibbles
            let new_value = [0x000F, 0x00F0, 0x0F00, 0xF000]
                .into_iter()
                .map(|mask| if value & mask != 0 { value & mask } else { existing_value & mask })
                .reduce(|a, b| a | b)
                .unwrap();
            svp.write_external_memory(address, new_value);
        }
    } else {
        svp.write_external_memory(address, value);
    }
}

fn read_pointer(svp: &mut Svp, bank: RamBank, pointer: u16, modifier: u16) -> u8 {
    if pointer < 3 {
        // Pointer register
        let registers = match bank {
            RamBank::Zero => &mut svp.registers.ram0_pointers,
            RamBank::One => &mut svp.registers.ram1_pointers,
        };

        let ram_addr = registers[pointer as usize];
        increment_pointer_register(
            &mut registers[pointer as usize],
            modifier,
            svp.registers.status.loop_modulo(),
        );
        ram_addr
    } else {
        // "Fake" pointer register (p3/p7) used for access to internal RAM addresses 0-3
        // Modifier bits are used as the address
        modifier as u8
    }
}

fn increment_pointer_register(register: &mut u8, modifier: u16, loop_modulo: u8) {
    match modifier {
        0 => {
            // No auto-increment/decrement; do nothing
        }
        1 => {
            // Auto-increment with no modulo
            *register = (*register).wrapping_add(1);
        }
        2 => {
            // Modulo decrement
            *register = modulo_decrement(*register, loop_modulo);
        }
        3 => {
            // Modulo increment
            *register = modulo_increment(*register, loop_modulo);
        }
        _ => panic!("invalid pointer register modifier: {modifier}"),
    }
}

fn modulo_increment(value: u8, modulo: u8) -> u8 {
    let mask = modulo.wrapping_sub(1);
    (value & !mask) | (value.wrapping_add(1) & mask)
}

fn modulo_decrement(value: u8, modulo: u8) -> u8 {
    let mask = modulo.wrapping_sub(1);
    (value & !mask) | (value.wrapping_sub(1) & mask)
}
