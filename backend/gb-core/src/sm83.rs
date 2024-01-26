//! Sharp SM83, the Game Boy CPU
//!
//! SM83 is kind of like a Z80-lite, but it's different enough that you can't just drop in a
//! Z80 core and expect it to work

mod arithmetic;
mod bits;
pub mod bus;
mod disassemble;
mod flags;
mod flow;
mod load;

use crate::sm83::bus::BusInterface;
use crate::HardwareMode;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct Flags {
    zero: bool,
    subtract: bool,
    half_carry: bool,
    carry: bool,
}

impl From<Flags> for u8 {
    fn from(value: Flags) -> Self {
        (u8::from(value.zero) << 7)
            | (u8::from(value.subtract) << 6)
            | (u8::from(value.half_carry) << 5)
            | (u8::from(value.carry) << 4)
    }
}

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        Self {
            zero: value.bit(7),
            subtract: value.bit(6),
            half_carry: value.bit(5),
            carry: value.bit(4),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    a: u8,
    f: Flags,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    sp: u16,
    pc: u16,
    ime: bool,
}

macro_rules! impl_increment_register_pair {
    (@inner $name:ident, $r1:ident, $r2:ident, $overflowing_op:ident, $wrapping_op:ident) => {
        fn $name(&mut self) {
            let ($r2, carry) = self.$r2.$overflowing_op(1);
            self.$r2 = $r2;
            self.$r1 = self.$r1.$wrapping_op(carry.into());
        }
    };
    ($name:ident, $r1:ident, $r2:ident, increment) => {
        impl_increment_register_pair!(@inner $name, $r1, $r2, overflowing_add, wrapping_add);
    };
    ($name:ident, $r1:ident, $r2:ident, decrement) => {
        impl_increment_register_pair!(@inner $name, $r1, $r2, overflowing_sub, wrapping_sub);
    };
}

const ENTRY_POINT: u16 = 0x0100;
const HRAM_END: u16 = 0xFFFE;

impl Registers {
    fn new(hardware_mode: HardwareMode, pretend_to_be_gba: bool) -> Self {
        // Values from https://gbdev.io/pandocs/Power_Up_Sequence.html
        // Most important is that DMG sets A=$01 and CGB sets A=$11
        match hardware_mode {
            HardwareMode::Dmg => Self {
                a: 0x01,
                f: Flags { zero: true, subtract: false, half_carry: false, carry: false },
                b: 0x00,
                c: 0x13,
                d: 0x00,
                e: 0xD8,
                h: 0x01,
                l: 0x4D,
                sp: HRAM_END,
                pc: ENTRY_POINT,
                ime: false,
            },
            HardwareMode::Cgb => Self {
                a: 0x11,
                f: Flags { zero: true, subtract: false, half_carry: false, carry: false },
                // GBA sets B bit 0 at boot ($01), GBC does not ($00)
                b: pretend_to_be_gba.into(),
                c: 0x00,
                d: 0xFF,
                e: 0x56,
                h: 0x00,
                l: 0x0D,
                sp: HRAM_END,
                pc: ENTRY_POINT,
                ime: false,
            },
        }
    }

    fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }

    fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }

    fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f.into()])
    }

    impl_increment_register_pair!(increment_bc, b, c, increment);
    impl_increment_register_pair!(decrement_bc, b, c, decrement);

    impl_increment_register_pair!(increment_de, d, e, increment);
    impl_increment_register_pair!(decrement_de, d, e, decrement);

    impl_increment_register_pair!(increment_hl, h, l, increment);
    impl_increment_register_pair!(decrement_hl, h, l, decrement);

    fn increment_sp(&mut self) {
        self.sp = self.sp.wrapping_add(1);
    }

    fn decrement_sp(&mut self) {
        self.sp = self.sp.wrapping_sub(1);
    }

    fn set_hl(&mut self, hl: u16) {
        let [h, l] = hl.to_be_bytes();
        self.h = h;
        self.l = l;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    pending_ime_set: bool,
    handling_interrupt: bool,
    halted: bool,
    halt_bug_triggered: bool,
    executed_invalid_opcode: bool,
}

impl State {
    fn new() -> Self {
        Self {
            pending_ime_set: false,
            handling_interrupt: false,
            halted: false,
            halt_bug_triggered: false,
            executed_invalid_opcode: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptType {
    VBlank,
    LcdStatus,
    Timer,
    Serial,
    Joypad,
}

impl InterruptType {
    // In descending priority order
    pub const ALL: [Self; 5] =
        [Self::VBlank, Self::LcdStatus, Self::Timer, Self::Serial, Self::Joypad];

    fn interrupt_vector(self) -> u16 {
        match self {
            Self::VBlank => 0x0040,
            Self::LcdStatus => 0x0048,
            Self::Timer => 0x0050,
            Self::Serial => 0x0058,
            Self::Joypad => 0x0060,
        }
    }

    pub fn register_mask(self) -> u8 {
        match self {
            Self::VBlank => 1 << 0,
            Self::LcdStatus => 1 << 1,
            Self::Timer => 1 << 2,
            Self::Serial => 1 << 3,
            Self::Joypad => 1 << 4,
        }
    }

    pub fn from_bits(bits: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|interrupt_type| bits & interrupt_type.register_mask() != 0)
    }
}

trait BusExt {
    fn write_u16(&mut self, address: u16, value: u16);
}

impl<B: BusInterface> BusExt for B {
    fn write_u16(&mut self, address: u16, value: u16) {
        let [lsb, msb] = value.to_le_bytes();
        self.write(address, lsb);
        self.write(address.wrapping_add(1), msb);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sm83 {
    registers: Registers,
    state: State,
}

impl Sm83 {
    pub fn new(hardware_mode: HardwareMode, pretend_to_be_gba: bool) -> Self {
        Self { registers: Registers::new(hardware_mode, pretend_to_be_gba), state: State::new() }
    }

    pub fn execute_instruction<B: BusInterface>(&mut self, bus: &mut B) {
        if self.state.executed_invalid_opcode || bus.halt() {
            // CPU is halted or frozen
            bus.idle();
            return;
        }

        if self.state.halted && !self.state.handling_interrupt {
            // HALT halts the CPU until an interrupt triggers. IME is not checked for this so the
            // CPU will not necessarily handle the interrupt
            if !bus.interrupt_pending() {
                bus.idle();
                return;
            }

            // TODO should there be a delay here?
            self.state.halted = false;
            if self.registers.ime {
                self.state.handling_interrupt = true;
            }
        }

        if self.state.handling_interrupt {
            self.execute_interrupt_service_routine(bus);

            self.state.halted = false;
            self.state.handling_interrupt = false;

            return;
        }

        if self.state.pending_ime_set {
            self.registers.ime = true;
            self.state.pending_ime_set = false;
        }

        let opcode = self.fetch_operand(bus);

        log::trace!(
            "Executing opcode {opcode:02X} ({}) from PC {:04X}; IME={}, A={:02X}, F={:02X}, B={:02X}, C={:02X}, D={:02X}, E={:02X}, H={:02X}, L={:02X}, SP={:04X}",
            disassemble::instruction_str(opcode),
            self.registers.pc.wrapping_sub(1),
            self.registers.ime,
            self.registers.a,
            u8::from(self.registers.f),
            self.registers.b,
            self.registers.c,
            self.registers.d,
            self.registers.e,
            self.registers.h,
            self.registers.l,
            self.registers.sp
        );

        self.execute_opcode(bus, opcode);

        self.poll_for_interrupts(bus);
    }

    fn execute_opcode<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        match opcode {
            // NOP
            0x00 => {}
            // LD rr, u16
            0x01 | 0x11 | 0x21 | 0x31 => self.ld_rr_nn(bus, opcode),
            // INC rr
            0x03 | 0x13 | 0x23 | 0x33 => self.inc_rr(bus, opcode),
            // DEC rr
            0x0B | 0x1B | 0x2B | 0x3B => self.dec_rr(bus, opcode),
            // ADD HL, rr
            0x09 | 0x19 | 0x29 | 0x39 => self.add_hl_rr(bus, opcode),
            // INC r / INC (HL)
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => self.inc_r(bus, opcode),
            // DEC r / DEC (HL)
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => self.dec_r(bus, opcode),
            // LD r, u8 / LD (HL), u8
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => self.ld_r_imm(bus, opcode),
            // LD (BC), A
            0x02 => self.ld_bc_a(bus),
            // RLCA
            0x07 => self.rlca(),
            // LD (u16), SP
            0x08 => self.ld_indirect_sp(bus),
            // LD A, (BC)
            0x0A => self.ld_a_bc(bus),
            // RRCA
            0x0F => self.rrca(),
            // STOP
            0x10 => self.stop(bus),
            // LD (DE), A
            0x12 => self.ld_de_a(bus),
            // RLA
            0x17 => self.rla(),
            // JR i8
            0x18 => self.jr_e(bus),
            // LD A, (DE)
            0x1A => self.ld_a_de(bus),
            // RRA
            0x1F => self.rra(),
            // JR cc, i8
            0x20 | 0x28 | 0x30 | 0x38 => self.jr_cc_e(bus, opcode),
            // LD (HL+), A
            0x22 => self.ld_hl_a_postinc(bus),
            // DAA
            0x27 => self.daa(),
            // LD A, (HL+)
            0x2A => self.ld_a_hl_postinc(bus),
            // CPL
            0x2F => self.cpl(),
            // LD (HL-), A
            0x32 => self.ld_hl_a_postdec(bus),
            // SCF
            0x37 => self.scf(),
            // LD A, (HL-)
            0x3A => self.ld_a_hl_postdec(bus),
            // CCF
            0x3F => self.ccf(),
            // LD r, r' / LD (HL), r / LD r, (HL)
            0x40..=0x75 | 0x77..=0x7F => self.ld_r_r(bus, opcode),
            // HALT
            0x76 => self.halt(bus),
            // ADD A, r / ADD A, (HL)
            0x80..=0x87 => self.add_a_r(bus, opcode),
            // ADC A, r / ADC A, (HL)
            0x88..=0x8F => self.adc_a_r(bus, opcode),
            // SUB A, r / SUB A, (HL)
            0x90..=0x97 => self.sub_a_r(bus, opcode),
            // SBC A, r / SBC A, (HL)
            0x98..=0x9F => self.sbc_a_r(bus, opcode),
            // AND A, r / AND A, (HL)
            0xA0..=0xA7 => self.and_a_r(bus, opcode),
            // XOR A, r / XOR A, (HL)
            0xA8..=0xAF => self.xor_a_r(bus, opcode),
            // OR A, r / OR A, (HL)
            0xB0..=0xB7 => self.or_a_r(bus, opcode),
            // CP A, r / CP A, (HL)
            0xB8..=0xBF => self.cp_a_r(bus, opcode),
            // POP rr
            0xC1 | 0xD1 | 0xE1 | 0xF1 => self.pop_rr(bus, opcode),
            // PUSH rr
            0xC5 | 0xD5 | 0xE5 | 0xF5 => self.push_rr(bus, opcode),
            // RET cc
            0xC0 | 0xC8 | 0xD0 | 0xD8 => self.ret_cc(bus, opcode),
            // JP cc, u16
            0xC2 | 0xCA | 0xD2 | 0xDA => self.jp_cc_nn(bus, opcode),
            // CALL cc, u16
            0xC4 | 0xCC | 0xD4 | 0xDC => self.call_cc_nn(bus, opcode),
            // RST $xx
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => self.rst(bus, opcode),
            // JP u16
            0xC3 => self.jp_nn(bus),
            // ADD A, u8
            0xC6 => self.add_a_imm(bus),
            // RET
            0xC9 => self.ret(bus),
            // $CB prefix requires a second opcode fetch to determine instruction
            0xCB => self.execute_cb_prefix_opcode(bus),
            // CALL nn
            0xCD => self.call_nn(bus),
            // ADC A, u8
            0xCE => self.adc_a_imm(bus),
            // SUB A, u8
            0xD6 => self.sub_a_imm(bus),
            // RETI
            0xD9 => self.reti(bus),
            // SBC A, u8
            0xDE => self.sbc_a_imm(bus),
            // LDH (u8), A
            0xE0 => self.ldh_imm_a(bus),
            // LD ($FF00+C), A
            0xE2 => self.ld_c_a_high_page(bus),
            // AND A, u8
            0xE6 => self.and_a_imm(bus),
            // ADD SP, i8
            0xE8 => self.add_sp_e(bus),
            // JP HL
            0xE9 => self.jp_hl(),
            // LD (u16), A
            0xEA => self.ld_indirect_a(bus),
            // XOR A, u8
            0xEE => self.xor_a_imm(bus),
            // LDH A, (u8)
            0xF0 => self.ldh_a_imm(bus),
            // LD A, ($FF00+C)
            0xF2 => self.ld_a_c_high_page(bus),
            // DI
            0xF3 => self.di(),
            // OR A, u8
            0xF6 => self.or_a_imm(bus),
            // LD HL, SP+i8
            0xF8 => self.ld_hl_sp_e(bus),
            // LD SP, HL
            0xF9 => self.ld_sp_hl(bus),
            // LD A, (u16)
            0xFA => self.ld_a_indirect(bus),
            // EI
            0xFB => self.ei(),
            // CP A, u8
            0xFE => self.cp_a_imm(bus),
            // Invalid opcodes; executing one of these causes the CPU to lock up
            0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
                log::error!(
                    "SM83 executed invalid opcode ${opcode:02X} at address ${:04X}; CPU is now frozen",
                    self.registers.pc.wrapping_sub(1)
                );
                self.state.executed_invalid_opcode = true;
            }
        }
    }

    fn execute_cb_prefix_opcode<B: BusInterface>(&mut self, bus: &mut B) {
        let opcode = self.fetch_operand(bus);

        log::trace!(
            "  CB prefix opcode: {opcode:02X} ({})",
            disassemble::cb_instruction_str(opcode)
        );

        match opcode {
            // RLC r / RLC (HL)
            0x00..=0x07 => self.rlc_r(bus, opcode),
            // RRC r / RRC (HL)
            0x08..=0x0F => self.rrc_r(bus, opcode),
            // RL r / RL (HL)
            0x10..=0x17 => self.rl_r(bus, opcode),
            // RR r / RR (HL)
            0x18..=0x1F => self.rr_r(bus, opcode),
            // SLA r / SLA (HL)
            0x20..=0x27 => self.sla(bus, opcode),
            // SRA r / SRA (HL)
            0x28..=0x2F => self.sra(bus, opcode),
            // SWAP r / SWAP (HL)
            0x30..=0x37 => self.swap(bus, opcode),
            // SRL r / SRL (HL)
            0x38..=0x3F => self.srl(bus, opcode),
            // BIT n, r / BIT n, (HL)
            0x40..=0x7F => self.bit(bus, opcode),
            // RES n, r / RES n, (HL)
            0x80..=0xBF => self.res(bus, opcode),
            // SET n, r / SET n, (HL)
            0xC0..=0xFF => self.set(bus, opcode),
        }
    }

    fn execute_interrupt_service_routine<B: BusInterface>(&mut self, bus: &mut B) {
        // The CPU idles for 2 M-cycles at the start of the interrupt service routine
        bus.idle();
        bus.idle();

        let [pc_lsb, pc_msb] = self.registers.pc.to_le_bytes();

        self.push_stack(bus, pc_msb);

        // The IE register gets read in between the MSB push and LSB push, as verified on hardware by the ie_push test
        // ROM in mooneye-test-suite.
        // This matters for the (contrived) case where one of the two stack pushes will write to the IE register
        let ie_register = bus.read_ie_register();
        self.push_stack(bus, pc_lsb);

        // Take the last idle cycle before reading IF; this *seems* to fix Pinball Deluxe
        bus.idle();

        let if_register = bus.read_if_register();
        self.registers.ime = false;

        let Some(interrupt_type) = InterruptType::from_bits(ie_register & if_register) else {
            // IE & IF can equal 0 if one of the stack pushes wrote to IE and cleared bits that were previously set.
            // In this case PC is set to $0000, as verified on hardware by the ie_push test ROM in mooneye-test-suite
            self.registers.pc = 0x0000;
            return;
        };
        bus.acknowledge_interrupt(interrupt_type);

        self.registers.pc = interrupt_type.interrupt_vector();

        log::trace!("Handling interrupt of type {interrupt_type:?}");
    }

    fn fetch_operand<B: BusInterface>(&mut self, bus: &mut B) -> u8 {
        let operand = bus.read(self.registers.pc);
        if self.state.halt_bug_triggered {
            // If the HALT bug just triggered, don't increment PC for this opcode fetch
            // The Smurfs (SGB) depends on this to boot
            self.state.halt_bug_triggered = false;
        } else {
            self.registers.pc = self.registers.pc.wrapping_add(1);
        }

        log::trace!("  Fetched operand {operand:02X}");

        operand
    }

    fn fetch_operand_u16<B: BusInterface>(&mut self, bus: &mut B) -> u16 {
        let operand_lsb = self.fetch_operand(bus);
        let operand_msb = self.fetch_operand(bus);
        u16::from_le_bytes([operand_lsb, operand_msb])
    }

    fn push_stack<B: BusInterface>(&mut self, bus: &mut B, value: u8) {
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        bus.write(self.registers.sp, value);
    }

    fn push_stack_u16<B: BusInterface>(&mut self, bus: &mut B, value: u16) {
        let [value_lsb, value_msb] = value.to_le_bytes();
        self.push_stack(bus, value_msb);
        self.push_stack(bus, value_lsb);
    }

    fn pop_stack<B: BusInterface>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.registers.sp);
        self.registers.sp = self.registers.sp.wrapping_add(1);
        value
    }

    fn pop_stack_u16<B: BusInterface>(&mut self, bus: &mut B) -> u16 {
        let lsb = self.pop_stack(bus);
        let msb = self.pop_stack(bus);
        u16::from_le_bytes([lsb, msb])
    }

    fn poll_for_interrupts<B: BusInterface>(&mut self, bus: &mut B) {
        self.state.handling_interrupt = self.registers.ime && bus.interrupt_pending();
    }

    fn read_register<B: BusInterface>(&self, bus: &mut B, register_bits: u8) -> u8 {
        match register_bits & 0x7 {
            0x0 => self.registers.b,
            0x1 => self.registers.c,
            0x2 => self.registers.d,
            0x3 => self.registers.e,
            0x4 => self.registers.h,
            0x5 => self.registers.l,
            // Indirect HL
            0x6 => bus.read(self.registers.hl()),
            0x7 => self.registers.a,
            _ => unreachable!("value & 0x7 is always <= 0x7"),
        }
    }

    fn write_register<B: BusInterface>(&mut self, bus: &mut B, register_bits: u8, value: u8) {
        match register_bits & 0x7 {
            0x0 => self.registers.b = value,
            0x1 => self.registers.c = value,
            0x2 => self.registers.d = value,
            0x3 => self.registers.e = value,
            0x4 => self.registers.h = value,
            0x5 => self.registers.l = value,
            // Indirect HL
            0x6 => bus.write(self.registers.hl(), value),
            0x7 => self.registers.a = value,
            _ => unreachable!("value & 0x7 is always <= 0x7"),
        }
    }
}
