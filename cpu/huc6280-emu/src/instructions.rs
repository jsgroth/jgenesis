use crate::bus::{BusInterface, ClockSpeed};
use crate::{BlockTransferState, BlockTransferStep, Flags, Huc6280, Registers};
use jgenesis_common::num::GetBit;
use std::mem;

// HuC6280 "zero" page is at logical $2000-$20FF, not $0000-$00FF like on 6502
const ZERO_PAGE_BASE: u16 = 0x2000;

// And stack is at logical $2100-$21FF
const STACK_BASE: u16 = 0x2100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptType {
    Irq1,
    Irq2,
    Tiq,
    Brk,
    // NMI is not connected in PC Engine
}

impl InterruptType {
    fn vector_address(self) -> u16 {
        match self {
            Self::Irq2 | Self::Brk => 0xFFF6,
            Self::Irq1 => 0xFFF8,
            Self::Tiq => 0xFFFA,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadRegister {
    A,
    X,
    Y,
    S,
    Z, // Zero; for STZ instructions
}

impl ReadRegister {
    fn get(self, registers: &Registers) -> u8 {
        match self {
            Self::A => registers.a,
            Self::X => registers.x,
            Self::Y => registers.y,
            Self::S => registers.s,
            Self::Z => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RwRegister {
    A,
    X,
    Y,
    S,
}

impl RwRegister {
    fn set(self, registers: &mut Registers, value: u8) {
        match self {
            Self::A => registers.a = value,
            Self::X => registers.x = value,
            Self::Y => registers.y = value,
            Self::S => registers.s = value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexRegister {
    X,
    Y,
}

impl IndexRegister {
    fn get(self, registers: &Registers) -> u8 {
        match self {
            Self::X => registers.x,
            Self::Y => registers.y,
        }
    }
}

macro_rules! impl_read_fn {
    ($name:ident, $execute_fn:ident($op:ident)) => {
        fn $name(&mut self) {
            self.$execute_fn(Self::$op);
        }
    };
}

macro_rules! impl_store_fn {
    ($name:ident, $execute_fn:ident($register:ident)) => {
        fn $name(&mut self) {
            self.$execute_fn(ReadRegister::$register);
        }
    };
}

// Same as impl_read_fn, but I think it's clearer for the RMW instructions to invoke a separate macro
macro_rules! impl_modify_fn {
    ($name:ident, $execute_fn:ident($op:ident)) => {
        impl_read_fn!($name, $execute_fn($op));
    };
}

macro_rules! impl_branch {
    ($name:ident) => {
        fn $name(&mut self) {
            self.conditional_branch(true);
        }
    };
    ($name:ident, $field:ident == $value:literal) => {
        fn $name(&mut self) {
            self.conditional_branch(self.cpu.registers.p.$field == $value);
        }
    };
}

macro_rules! impl_set_flag {
    ($name:ident, $flag:ident = $value:literal) => {
        fn $name(&mut self) {
            self.bus_idle();

            self.cpu.registers.p.$flag = $value;
        }
    };
}

pub struct InstructionExecutor<'cpu, 'bus, Bus> {
    cpu: &'cpu mut Huc6280,
    bus: &'bus mut Bus,
}

impl<'cpu, 'bus, Bus> InstructionExecutor<'cpu, 'bus, Bus>
where
    Bus: BusInterface,
{
    pub fn new(cpu: &'cpu mut Huc6280, bus: &'bus mut Bus) -> Self {
        Self { cpu, bus }
    }

    pub fn execute_instruction(mut self) {
        if self.cpu.state.block_transfer.is_some() {
            // Interrupts cannot interrupt in-progress block transfers
            return self.progress_block_transfer();
        }

        if self.cpu.state.pending_interrupt {
            self.cpu.state.pending_interrupt = false;

            // TIQ > IRQ1 > IRQ2 in priority
            if self.cpu.state.latched_interrupt_lines.tiq {
                return self.handle_interrupt(InterruptType::Tiq);
            }

            if self.cpu.state.latched_interrupt_lines.irq1 {
                return self.handle_interrupt(InterruptType::Irq1);
            }

            if self.cpu.state.latched_interrupt_lines.irq2 {
                return self.handle_interrupt(InterruptType::Irq2);
            }
        }

        // T bit gets latched and then cleared at the start of every instruction
        // Interrupts push the T bit onto the stack intact, but it is always clear when pushed
        // using a PHP or BRK instruction
        self.cpu.state.memory_op_at_fetch = self.cpu.registers.p.memory_op;
        self.cpu.registers.p.memory_op = false;

        let opcode = self.fetch_operand();
        match opcode {
            0x00 => self.handle_interrupt(InterruptType::Brk), // BRK
            0x01 => self.ora_zero_page_indirect_x(),
            0x02 => self.sxy(),
            0x03 => self.st0(),
            0x04 => self.tsb_zero_page(),
            0x05 => self.ora_zero_page(),
            0x06 => self.asl_zero_page(),
            0x07 | 0x17 | 0x27 | 0x37 | 0x47 | 0x57 | 0x67 | 0x77 => self.rmbi(opcode),
            0x08 => self.php(),
            0x09 => self.ora_immediate(),
            0x0A => self.asl_accumulator(),
            0x0C => self.tsb_absolute(),
            0x0D => self.ora_absolute(),
            0x0E => self.asl_absolute(),
            0x0F | 0x1F | 0x2F | 0x3F | 0x4F | 0x5F | 0x6F | 0x7F => self.bbri(opcode),
            0x10 => self.bpl(),
            0x11 => self.ora_zero_page_indirect_y(),
            0x12 => self.ora_zero_page_indirect(),
            0x13 => self.st1(),
            0x14 => self.trb_zero_page(),
            0x15 => self.ora_zero_page_x(),
            0x16 => self.asl_zero_page_x(),
            0x18 => self.clc(),
            0x19 => self.ora_absolute_y(),
            0x1A => self.inc_accumulator(),
            0x1C => self.trb_absolute(),
            0x1D => self.ora_absolute_x(),
            0x1E => self.asl_absolute_x(),
            0x20 => self.jsr(),
            0x21 => self.and_zero_page_indirect_x(),
            0x22 => self.sax(),
            0x23 => self.st2(),
            0x24 => self.bit_zero_page(),
            0x25 => self.and_zero_page(),
            0x26 => self.rol_zero_page(),
            0x28 => self.plp(),
            0x29 => self.and_immediate(),
            0x2A => self.rol_accumulator(),
            0x2C => self.bit_absolute(),
            0x2D => self.and_absolute(),
            0x2E => self.rol_absolute(),
            0x30 => self.bmi(),
            0x31 => self.and_zero_page_indirect_y(),
            0x32 => self.and_zero_page_indirect(),
            0x34 => self.bit_zero_page_x(),
            0x35 => self.and_zero_page_x(),
            0x36 => self.rol_zero_page_x(),
            0x38 => self.sec(),
            0x39 => self.and_absolute_y(),
            0x3A => self.dec_accumulator(),
            0x3C => self.bit_absolute_x(),
            0x3D => self.and_absolute_x(),
            0x3E => self.rol_absolute_x(),
            0x40 => self.rti(),
            0x41 => self.eor_zero_page_indirect_x(),
            0x42 => self.say(),
            0x43 => self.tma(),
            0x44 => self.bsr(),
            0x45 => self.eor_zero_page(),
            0x46 => self.lsr_zero_page(),
            0x48 => self.pha(),
            0x49 => self.eor_immediate(),
            0x4A => self.lsr_accumulator(),
            0x4C => self.jmp_absolute(),
            0x4D => self.eor_absolute(),
            0x4E => self.lsr_absolute(),
            0x50 => self.bvc(),
            0x51 => self.eor_zero_page_indirect_y(),
            0x52 => self.eor_zero_page_indirect(),
            0x53 => self.tam(),
            0x54 => self.csl(),
            0x55 => self.eor_zero_page_x(),
            0x56 => self.lsr_zero_page_x(),
            0x58 => self.cli(),
            0x59 => self.eor_absolute_y(),
            0x5A => self.phy(),
            0x5D => self.eor_absolute_x(),
            0x5E => self.lsr_absolute_x(),
            0x60 => self.rts(),
            0x61 => self.adc_zero_page_indirect_x(),
            0x62 => self.cla(),
            0x64 => self.stz_zero_page(),
            0x65 => self.adc_zero_page(),
            0x66 => self.ror_zero_page(),
            0x68 => self.pla(),
            0x69 => self.adc_immediate(),
            0x6A => self.ror_accumulator(),
            0x6C => self.jmp_absolute_indirect(),
            0x6D => self.adc_absolute(),
            0x6E => self.ror_absolute(),
            0x70 => self.bvs(),
            0x71 => self.adc_zero_page_indirect_y(),
            0x72 => self.adc_zero_page_indirect(),
            0x73 => self.tii(),
            0x74 => self.stz_zero_page_x(),
            0x75 => self.adc_zero_page_x(),
            0x76 => self.ror_zero_page_x(),
            0x78 => self.sei(),
            0x79 => self.adc_absolute_y(),
            0x7A => self.ply(),
            0x7C => self.jmp_absolute_indirect_x(),
            0x7D => self.adc_absolute_x(),
            0x7E => self.ror_absolute_x(),
            0x80 => self.bra(),
            0x81 => self.sta_zero_page_indirect_x(),
            0x82 => self.clx(),
            0x83 => self.tst_zero_page(),
            0x84 => self.sty_zero_page(),
            0x85 => self.sta_zero_page(),
            0x86 => self.stx_zero_page(),
            0x87 | 0x97 | 0xA7 | 0xB7 | 0xC7 | 0xD7 | 0xE7 | 0xF7 => self.smbi(opcode),
            0x88 => self.dey(),
            0x89 => self.bit_immediate(),
            0x8A => self.txa(),
            0x8C => self.sty_absolute(),
            0x8D => self.sta_absolute(),
            0x8E => self.stx_absolute(),
            0x8F | 0x9F | 0xAF | 0xBF | 0xCF | 0xDF | 0xEF | 0xFF => self.bbsi(opcode),
            0x90 => self.bcc(),
            0x91 => self.sta_zero_page_indirect_y(),
            0x92 => self.sta_zero_page_indirect(),
            0x93 => self.tst_absolute(),
            0x94 => self.sty_zero_page_x(),
            0x95 => self.sta_zero_page_x(),
            0x96 => self.stx_zero_page_y(),
            0x98 => self.tya(),
            0x99 => self.sta_absolute_y(),
            0x9A => self.txs(),
            0x9C => self.stz_absolute(),
            0x9D => self.sta_absolute_x(),
            0x9E => self.stz_absolute_x(),
            0xA0 => self.ldy_immediate(),
            0xA1 => self.lda_zero_page_indirect_x(),
            0xA2 => self.ldx_immediate(),
            0xA3 => self.tst_zero_page_x(),
            0xA4 => self.ldy_zero_page(),
            0xA5 => self.lda_zero_page(),
            0xA6 => self.ldx_zero_page(),
            0xA8 => self.tay(),
            0xA9 => self.lda_immediate(),
            0xAA => self.tax(),
            0xAC => self.ldy_absolute(),
            0xAD => self.lda_absolute(),
            0xAE => self.ldx_absolute(),
            0xB0 => self.bcs(),
            0xB1 => self.lda_zero_page_indirect_y(),
            0xB2 => self.lda_zero_page_indirect(),
            0xB3 => self.tst_absolute_x(),
            0xB4 => self.ldy_zero_page_x(),
            0xB5 => self.lda_zero_page_x(),
            0xB6 => self.ldx_zero_page_y(),
            0xB8 => self.clv(),
            0xB9 => self.lda_absolute_y(),
            0xBA => self.tsx(),
            0xBC => self.ldy_absolute_x(),
            0xBD => self.lda_absolute_x(),
            0xBE => self.ldx_absolute_y(),
            0xC0 => self.cpy_immediate(),
            0xC1 => self.cmp_zero_page_indirect_x(),
            0xC2 => self.cly(),
            0xC3 => self.tdd(),
            0xC4 => self.cpy_zero_page(),
            0xC5 => self.cmp_zero_page(),
            0xC6 => self.dec_zero_page(),
            0xC8 => self.iny(),
            0xC9 => self.cmp_immediate(),
            0xCA => self.dex(),
            0xCC => self.cpy_absolute(),
            0xCD => self.cmp_absolute(),
            0xCE => self.dec_absolute(),
            0xD0 => self.bne(),
            0xD1 => self.cmp_zero_page_indirect_y(),
            0xD2 => self.cmp_zero_page_indirect(),
            0xD3 => self.tin(),
            0xD4 => self.csh(),
            0xD5 => self.cmp_zero_page_x(),
            0xD6 => self.dec_zero_page_x(),
            0xD8 => self.cld(),
            0xD9 => self.cmp_absolute_y(),
            0xDA => self.phx(),
            0xDD => self.cmp_absolute_x(),
            0xDE => self.dec_absolute_x(),
            0xE0 => self.cpx_immediate(),
            0xE1 => self.sbc_zero_page_indirect_x(),
            0xE3 => self.tia(),
            0xE4 => self.cpx_zero_page(),
            0xE5 => self.sbc_zero_page(),
            0xE6 => self.inc_zero_page(),
            0xE8 => self.inx(),
            0xE9 => self.sbc_immediate(),
            0xEA => self.nop(),
            0xEC => self.cpx_absolute(),
            0xED => self.sbc_absolute(),
            0xEE => self.inc_absolute(),
            0xF0 => self.beq(),
            0xF1 => self.sbc_zero_page_indirect_y(),
            0xF2 => self.sbc_zero_page_indirect(),
            0xF3 => self.tai(),
            0xF4 => self.set(),
            0xF5 => self.sbc_zero_page_x(),
            0xF6 => self.inc_zero_page_x(),
            0xF8 => self.sed(),
            0xF9 => self.sbc_absolute_y(),
            0xFA => self.plx(),
            0xFD => self.sbc_absolute_x(),
            0xFE => self.inc_absolute_x(),
            0x0B | 0x1B | 0x2B | 0x33 | 0x3B | 0x4B | 0x5B | 0x5C | 0x63 | 0x6B | 0x7B | 0x8B
            | 0x9B | 0xAB | 0xBB | 0xCB | 0xDB | 0xDC | 0xE2 | 0xEB | 0xFB | 0xFC => {
                // Illegal opcodes; supposedly function as NOP?
                log::warn!("Executed illegal opcode {opcode:02X}");
                self.nop();
            }
        }
    }

    // Interrupt lines and the I flag are latched/polled at the beginning of the final cycle of each instruction
    // For simplicity, latch them at the beginning of every cycle
    fn poll_interrupt_lines(&mut self) {
        let interrupt_lines = self.bus.interrupt_lines();
        self.cpu.state.pending_interrupt =
            !self.cpu.registers.p.irq_disable && interrupt_lines.any();
        self.cpu.state.latched_interrupt_lines = interrupt_lines;
    }

    fn bus_read(&mut self, address: u32) -> u8 {
        self.poll_interrupt_lines();
        self.bus.read(address)
    }

    fn bus_write(&mut self, address: u32, value: u8) {
        self.poll_interrupt_lines();
        self.bus.write(address, value);
    }

    fn bus_idle(&mut self) {
        self.poll_interrupt_lines();
        self.bus.idle();
    }

    // NOP: No operation
    fn nop(&mut self) {
        self.bus_idle();
    }

    fn handle_interrupt(&mut self, interrupt: InterruptType) {
        // Dummy read; BRK advances PC
        match interrupt {
            InterruptType::Brk => {
                self.fetch_operand();
            }
            _ => {
                self.bus_read(self.map_address(self.cpu.registers.pc));
            }
        }

        self.push_stack_u16(self.cpu.registers.pc);

        let status = match interrupt {
            InterruptType::Brk => self.cpu.registers.p.to_u8_brk(),
            _ => self.cpu.registers.p.to_u8_interrupt(),
        };
        self.push_stack(status);

        self.cpu.registers.p.memory_op = false;
        self.cpu.registers.p.decimal = false;
        self.cpu.registers.p.irq_disable = true;

        let vector_addr = interrupt.vector_address();
        let pc_lsb = self.bus_read(self.map_address(vector_addr));
        let pc_msb = self.bus_read(self.map_address(vector_addr.wrapping_add(1)));
        self.cpu.registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        self.bus_idle();
    }

    fn fetch_operand(&mut self) -> u8 {
        let operand = self.bus_read(self.map_address(self.cpu.registers.pc));
        self.cpu.registers.pc = self.cpu.registers.pc.wrapping_add(1);
        operand
    }

    fn fetch_operand_u16(&mut self) -> u16 {
        let operand_lsb = self.fetch_operand();
        let operand_msb = self.fetch_operand();
        u16::from_le_bytes([operand_lsb, operand_msb])
    }

    fn push_stack(&mut self, value: u8) {
        let stack_addr = STACK_BASE | u16::from(self.cpu.registers.s);
        self.bus_write(self.map_address(stack_addr), value);
        self.cpu.registers.s = self.cpu.registers.s.wrapping_sub(1);
    }

    fn push_stack_u16(&mut self, value: u16) {
        let [lsb, msb] = value.to_le_bytes();
        self.push_stack(msb);
        self.push_stack(lsb);
    }

    fn pull_stack(&mut self) -> u8 {
        self.cpu.registers.s = self.cpu.registers.s.wrapping_add(1);
        let stack_addr = STACK_BASE | u16::from(self.cpu.registers.s);
        self.bus_read(self.map_address(stack_addr))
    }

    fn pull_stack_u16(&mut self) -> u16 {
        let lsb = self.pull_stack();
        let msb = self.pull_stack();
        u16::from_le_bytes([lsb, msb])
    }

    fn map_address(&self, logical_addr: u16) -> u32 {
        self.cpu.registers.map_address(logical_addr)
    }

    fn map_zero_page(&self, zero_page_addr: u8) -> u32 {
        self.map_address(ZERO_PAGE_BASE | u16::from(zero_page_addr))
    }

    fn read_immediate(&mut self, op: impl FnOnce(&mut Self, u8)) {
        let operand = self.fetch_operand();
        op(self, operand);
    }

    #[inline(always)]
    fn read_zero_page_indexed(
        &mut self,
        index: Option<IndexRegister>,
        op: impl FnOnce(&mut Self, u8),
    ) {
        let zero_page_addr = self.fetch_operand();

        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = zero_page_addr.wrapping_add(offset);
        let operand = self.bus_read(self.map_zero_page(indexed_addr));
        op(self, operand);
    }

    fn read_zero_page(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_zero_page_indexed(None, op);
    }

    fn read_zero_page_x(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_zero_page_indexed(Some(IndexRegister::X), op);
    }

    fn read_zero_page_y(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_zero_page_indexed(Some(IndexRegister::Y), op);
    }

    #[inline(always)]
    fn read_absolute_indexed(
        &mut self,
        index: Option<IndexRegister>,
        op: impl FnOnce(&mut Self, u8),
    ) {
        let address = self.fetch_operand_u16();

        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = address.wrapping_add(offset.into());
        let operand = self.bus_read(self.map_address(indexed_addr));
        op(self, operand);
    }

    fn read_absolute(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_absolute_indexed(None, op);
    }

    fn read_absolute_x(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_absolute_indexed(Some(IndexRegister::X), op);
    }

    fn read_absolute_y(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_absolute_indexed(Some(IndexRegister::Y), op);
    }

    #[inline(always)]
    fn read_indirect_indexed(
        &mut self,
        index: Option<IndexRegister>,
        op: impl FnOnce(&mut Self, u8),
    ) {
        let mut zero_page_addr = self.fetch_operand();
        self.bus_idle();

        if index == Some(IndexRegister::X) {
            // Zero page indexed indirect
            zero_page_addr = zero_page_addr.wrapping_add(self.cpu.registers.x);
        }

        let address_lsb = self.bus_read(self.map_zero_page(zero_page_addr));
        let address_msb = self.bus_read(self.map_zero_page(zero_page_addr.wrapping_add(1)));
        let mut address = u16::from_le_bytes([address_lsb, address_msb]);
        self.bus_idle();

        if index == Some(IndexRegister::Y) {
            // Zero page indirect indexed
            address = address.wrapping_add(self.cpu.registers.y.into());
        }

        let operand = self.bus_read(self.map_address(address));
        op(self, operand);
    }

    fn read_zero_page_indirect(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_indirect_indexed(None, op);
    }

    fn read_zero_page_indirect_x(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_indirect_indexed(Some(IndexRegister::X), op);
    }

    fn read_zero_page_indirect_y(&mut self, op: impl FnOnce(&mut Self, u8)) {
        self.read_indirect_indexed(Some(IndexRegister::Y), op);
    }

    #[inline(always)]
    fn load(&mut self, register: RwRegister, operand: u8) {
        register.set(&mut self.cpu.registers, operand);

        self.cpu.registers.p.zero = operand == 0;
        self.cpu.registers.p.negative = operand.bit(7);
    }

    // LDA: Load A
    fn lda(&mut self, operand: u8) {
        self.load(RwRegister::A, operand);
    }

    // LDX: Load X
    fn ldx(&mut self, operand: u8) {
        self.load(RwRegister::X, operand);
    }

    // LDY: Load Y
    fn ldy(&mut self, operand: u8) {
        self.load(RwRegister::Y, operand);
    }

    impl_read_fn!(lda_immediate, read_immediate(lda));
    impl_read_fn!(lda_zero_page, read_zero_page(lda));
    impl_read_fn!(lda_zero_page_x, read_zero_page_x(lda));
    impl_read_fn!(lda_absolute, read_absolute(lda));
    impl_read_fn!(lda_absolute_x, read_absolute_x(lda));
    impl_read_fn!(lda_absolute_y, read_absolute_y(lda));
    impl_read_fn!(lda_zero_page_indirect, read_zero_page_indirect(lda));
    impl_read_fn!(lda_zero_page_indirect_x, read_zero_page_indirect_x(lda));
    impl_read_fn!(lda_zero_page_indirect_y, read_zero_page_indirect_y(lda));

    impl_read_fn!(ldx_immediate, read_immediate(ldx));
    impl_read_fn!(ldx_zero_page, read_zero_page(ldx));
    impl_read_fn!(ldx_zero_page_y, read_zero_page_y(ldx));
    impl_read_fn!(ldx_absolute, read_absolute(ldx));
    impl_read_fn!(ldx_absolute_y, read_absolute_y(ldx));

    impl_read_fn!(ldy_immediate, read_immediate(ldy));
    impl_read_fn!(ldy_zero_page, read_zero_page(ldy));
    impl_read_fn!(ldy_zero_page_x, read_zero_page_x(ldy));
    impl_read_fn!(ldy_absolute, read_absolute(ldy));
    impl_read_fn!(ldy_absolute_x, read_absolute_x(ldy));

    fn add_binary(accumulator: u8, operand: u8, flags: &mut Flags) -> u8 {
        let existing_carry: u8 = flags.carry.into();

        let (result, carry1) = accumulator.overflowing_add(operand);
        let (result, carry2) = result.overflowing_add(existing_carry);
        let new_carry = carry1 || carry2;

        let bit_6_carry = (accumulator & 0x7F) + (operand & 0x7F) + existing_carry >= 0x80;
        let overflow = new_carry ^ bit_6_carry;

        flags.negative = result.bit(7);
        flags.overflow = overflow;
        flags.zero = result == 0;
        flags.carry = new_carry;

        result
    }

    fn add_decimal(accumulator: u8, operand: u8, flags: &mut Flags) -> u8 {
        // Formulas from http://www.6502.org/tutorials/decimal_mode.html#A
        // (65C02 versions)
        let existing_carry: u8 = flags.carry.into();

        let mut al = (accumulator & 0x0F) + (operand & 0x0F) + existing_carry;
        if al >= 0x0A {
            al = 0x10 | ((al + 0x06) & 0x0F);
        }

        let mut a = u16::from(accumulator & 0xF0) + u16::from(operand & 0xF0) + u16::from(al);
        if a >= 0xA0 {
            a += 0x60;
        }

        let result = a as u8;

        flags.zero = result == 0;
        flags.carry = a >= 0x0100;
        flags.negative = result.bit(7);
        // HuC6280 does not set V in decimal ADC/SBC

        result
    }

    fn sub_binary(accumulator: u8, operand: u8, flags: &mut Flags) -> u8 {
        Self::add_binary(accumulator, !operand, flags)
    }

    fn sub_decimal(accumulator: u8, operand: u8, flags: &mut Flags) -> u8 {
        // Formulas from http://www.6502.org/tutorials/decimal_mode.html#A
        // (65C02 versions)

        let existing_carry: i16 = flags.carry.into();

        let al = i16::from(accumulator & 0x0F) - i16::from(operand & 0x0F) + existing_carry - 1;
        let mut a = i16::from(accumulator) - i16::from(operand) + existing_carry - 1;

        if a < 0 {
            a -= 0x60;
        }

        if al < 0 {
            a -= 0x06;
        }

        // Carry flag is set based on binary arithmetic
        let binary_borrow = u16::from(accumulator) < u16::from(operand) + u16::from(!flags.carry);

        let result = a as u8;

        flags.zero = result == 0;
        flags.negative = result.bit(7);
        flags.carry = !binary_borrow;
        // HuC6280 does not set V in decimal ADC/SBC

        result
    }

    // ADC: Add with carry
    fn adc(&mut self, operand: u8) {
        let accumulator = if self.cpu.state.memory_op_at_fetch {
            let value = self.bus_read(self.map_zero_page(self.cpu.registers.x));
            self.bus_idle();
            value
        } else {
            self.cpu.registers.a
        };

        let result = if self.cpu.registers.p.decimal {
            self.bus_idle();
            Self::add_decimal(accumulator, operand, &mut self.cpu.registers.p)
        } else {
            Self::add_binary(accumulator, operand, &mut self.cpu.registers.p)
        };

        if self.cpu.state.memory_op_at_fetch {
            self.bus_write(self.map_zero_page(self.cpu.registers.x), result);
        } else {
            self.cpu.registers.a = result;
        }
    }

    impl_read_fn!(adc_immediate, read_immediate(adc));
    impl_read_fn!(adc_zero_page, read_zero_page(adc));
    impl_read_fn!(adc_zero_page_x, read_zero_page_x(adc));
    impl_read_fn!(adc_absolute, read_absolute(adc));
    impl_read_fn!(adc_absolute_x, read_absolute_x(adc));
    impl_read_fn!(adc_absolute_y, read_absolute_y(adc));
    impl_read_fn!(adc_zero_page_indirect, read_zero_page_indirect(adc));
    impl_read_fn!(adc_zero_page_indirect_x, read_zero_page_indirect_x(adc));
    impl_read_fn!(adc_zero_page_indirect_y, read_zero_page_indirect_y(adc));

    // SBC: Subtract with carry
    fn sbc(&mut self, operand: u8) {
        self.cpu.registers.a = if self.cpu.registers.p.decimal {
            self.bus_idle();
            Self::sub_decimal(self.cpu.registers.a, operand, &mut self.cpu.registers.p)
        } else {
            Self::sub_binary(self.cpu.registers.a, operand, &mut self.cpu.registers.p)
        };
    }

    impl_read_fn!(sbc_immediate, read_immediate(sbc));
    impl_read_fn!(sbc_zero_page, read_zero_page(sbc));
    impl_read_fn!(sbc_zero_page_x, read_zero_page_x(sbc));
    impl_read_fn!(sbc_absolute, read_absolute(sbc));
    impl_read_fn!(sbc_absolute_x, read_absolute_x(sbc));
    impl_read_fn!(sbc_absolute_y, read_absolute_y(sbc));
    impl_read_fn!(sbc_zero_page_indirect, read_zero_page_indirect(sbc));
    impl_read_fn!(sbc_zero_page_indirect_x, read_zero_page_indirect_x(sbc));
    impl_read_fn!(sbc_zero_page_indirect_y, read_zero_page_indirect_y(sbc));

    #[inline(always)]
    fn logical_op(&mut self, operand: u8, op: impl FnOnce(u8, u8) -> u8) {
        let accumulator = if self.cpu.state.memory_op_at_fetch {
            let value = self.bus_read(self.map_zero_page(self.cpu.registers.x));
            self.bus_idle();
            value
        } else {
            self.cpu.registers.a
        };

        let result = op(accumulator, operand);
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);

        if self.cpu.state.memory_op_at_fetch {
            self.bus_write(self.map_zero_page(self.cpu.registers.x), result);
        } else {
            self.cpu.registers.a = result;
        }
    }

    // AND: Logical and
    fn and(&mut self, operand: u8) {
        self.logical_op(operand, |a, b| a & b);
    }

    // EOR: Exclusive or
    fn eor(&mut self, operand: u8) {
        self.logical_op(operand, |a, b| a ^ b);
    }

    // ORA: Logical or
    fn ora(&mut self, operand: u8) {
        self.logical_op(operand, |a, b| a | b);
    }

    impl_read_fn!(and_immediate, read_immediate(and));
    impl_read_fn!(and_zero_page, read_zero_page(and));
    impl_read_fn!(and_zero_page_x, read_zero_page_x(and));
    impl_read_fn!(and_absolute, read_absolute(and));
    impl_read_fn!(and_absolute_x, read_absolute_x(and));
    impl_read_fn!(and_absolute_y, read_absolute_y(and));
    impl_read_fn!(and_zero_page_indirect, read_zero_page_indirect_x(and));
    impl_read_fn!(and_zero_page_indirect_x, read_zero_page_indirect_x(and));
    impl_read_fn!(and_zero_page_indirect_y, read_zero_page_indirect_y(and));

    impl_read_fn!(eor_immediate, read_immediate(eor));
    impl_read_fn!(eor_zero_page, read_zero_page(eor));
    impl_read_fn!(eor_zero_page_x, read_zero_page_x(eor));
    impl_read_fn!(eor_absolute, read_absolute(eor));
    impl_read_fn!(eor_absolute_x, read_absolute_x(eor));
    impl_read_fn!(eor_absolute_y, read_absolute_y(eor));
    impl_read_fn!(eor_zero_page_indirect, read_zero_page_indirect_x(eor));
    impl_read_fn!(eor_zero_page_indirect_x, read_zero_page_indirect_x(eor));
    impl_read_fn!(eor_zero_page_indirect_y, read_zero_page_indirect_y(eor));

    impl_read_fn!(ora_immediate, read_immediate(ora));
    impl_read_fn!(ora_zero_page, read_zero_page(ora));
    impl_read_fn!(ora_zero_page_x, read_zero_page_x(ora));
    impl_read_fn!(ora_absolute, read_absolute(ora));
    impl_read_fn!(ora_absolute_x, read_absolute_x(ora));
    impl_read_fn!(ora_absolute_y, read_absolute_y(ora));
    impl_read_fn!(ora_zero_page_indirect, read_zero_page_indirect_x(ora));
    impl_read_fn!(ora_zero_page_indirect_x, read_zero_page_indirect_x(ora));
    impl_read_fn!(ora_zero_page_indirect_y, read_zero_page_indirect_y(ora));

    // BIT: Bit test
    fn bit(&mut self, operand: u8) {
        let result = self.cpu.registers.a & operand;

        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.overflow = operand.bit(6);
        self.cpu.registers.p.negative = operand.bit(7);
    }

    impl_read_fn!(bit_immediate, read_immediate(bit));
    impl_read_fn!(bit_zero_page, read_zero_page(bit));
    impl_read_fn!(bit_zero_page_x, read_zero_page_x(bit));
    impl_read_fn!(bit_absolute, read_absolute(bit));
    impl_read_fn!(bit_absolute_x, read_absolute_x(bit));

    #[inline(always)]
    fn compare(&mut self, register: ReadRegister, operand: u8) {
        let source = register.get(&self.cpu.registers);

        self.cpu.registers.p.negative = source.wrapping_sub(operand).bit(7);
        self.cpu.registers.p.zero = source == operand;
        self.cpu.registers.p.carry = source >= operand;
    }

    // CMP: Compare A with M
    fn cmp(&mut self, operand: u8) {
        self.compare(ReadRegister::A, operand);
    }

    // CPX: Compare X with M
    fn cpx(&mut self, operand: u8) {
        self.compare(ReadRegister::X, operand);
    }

    // CPY: Compare Y with M
    fn cpy(&mut self, operand: u8) {
        self.compare(ReadRegister::Y, operand);
    }

    impl_read_fn!(cmp_immediate, read_immediate(cmp));
    impl_read_fn!(cmp_zero_page, read_zero_page(cmp));
    impl_read_fn!(cmp_zero_page_x, read_zero_page_x(cmp));
    impl_read_fn!(cmp_absolute, read_absolute(cmp));
    impl_read_fn!(cmp_absolute_x, read_absolute_x(cmp));
    impl_read_fn!(cmp_absolute_y, read_absolute_y(cmp));
    impl_read_fn!(cmp_zero_page_indirect, read_zero_page_indirect(cmp));
    impl_read_fn!(cmp_zero_page_indirect_x, read_zero_page_indirect_x(cmp));
    impl_read_fn!(cmp_zero_page_indirect_y, read_zero_page_indirect_y(cmp));

    impl_read_fn!(cpx_immediate, read_immediate(cpx));
    impl_read_fn!(cpx_zero_page, read_zero_page(cpx));
    impl_read_fn!(cpx_absolute, read_absolute(cpx));

    impl_read_fn!(cpy_immediate, read_immediate(cpy));
    impl_read_fn!(cpy_zero_page, read_zero_page(cpy));
    impl_read_fn!(cpy_absolute, read_absolute(cpy));

    #[inline(always)]
    fn store_zero_page_indexed(&mut self, index: Option<IndexRegister>, register: ReadRegister) {
        let zero_page_addr = self.fetch_operand();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = zero_page_addr.wrapping_add(offset);
        let value = register.get(&self.cpu.registers);
        self.bus_write(self.map_zero_page(indexed_addr), value);
    }

    #[inline(always)]
    fn store_zero_page(&mut self, register: ReadRegister) {
        self.store_zero_page_indexed(None, register);
    }

    #[inline(always)]
    fn store_zero_page_x(&mut self, register: ReadRegister) {
        self.store_zero_page_indexed(Some(IndexRegister::X), register);
    }

    #[inline(always)]
    fn store_zero_page_y(&mut self, register: ReadRegister) {
        self.store_zero_page_indexed(Some(IndexRegister::Y), register);
    }

    #[inline(always)]
    fn store_absolute_indexed(&mut self, index: Option<IndexRegister>, register: ReadRegister) {
        let address = self.fetch_operand_u16();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = address.wrapping_add(offset.into());
        let value = register.get(&self.cpu.registers);
        self.bus_write(self.map_address(indexed_addr), value);
    }

    #[inline(always)]
    fn store_absolute(&mut self, register: ReadRegister) {
        self.store_absolute_indexed(None, register);
    }

    #[inline(always)]
    fn store_absolute_x(&mut self, register: ReadRegister) {
        self.store_absolute_indexed(Some(IndexRegister::X), register);
    }

    #[inline(always)]
    fn store_absolute_y(&mut self, register: ReadRegister) {
        self.store_absolute_indexed(Some(IndexRegister::Y), register);
    }

    #[inline(always)]
    fn store_indirect_indexed(&mut self, index: Option<IndexRegister>, register: ReadRegister) {
        let mut zero_page_addr = self.fetch_operand();
        self.bus_idle();

        if index == Some(IndexRegister::X) {
            // Zero page indexed indirect
            zero_page_addr = zero_page_addr.wrapping_add(self.cpu.registers.x);
        }

        let address_lsb = self.bus_read(self.map_zero_page(zero_page_addr));
        let address_msb = self.bus_read(self.map_zero_page(zero_page_addr.wrapping_add(1)));
        let mut address = u16::from_le_bytes([address_lsb, address_msb]);
        self.bus_idle();

        if index == Some(IndexRegister::Y) {
            // Zero page indirect indexed
            address = address.wrapping_add(self.cpu.registers.y.into());
        }

        let value = register.get(&self.cpu.registers);
        self.bus_write(self.map_address(address), value);
    }

    #[inline(always)]
    fn store_zero_page_indirect(&mut self, register: ReadRegister) {
        self.store_indirect_indexed(None, register);
    }

    #[inline(always)]
    fn store_zero_page_indirect_x(&mut self, register: ReadRegister) {
        self.store_indirect_indexed(Some(IndexRegister::X), register);
    }

    #[inline(always)]
    fn store_zero_page_indirect_y(&mut self, register: ReadRegister) {
        self.store_indirect_indexed(Some(IndexRegister::Y), register);
    }

    // STA: Store A
    impl_store_fn!(sta_zero_page, store_zero_page(A));
    impl_store_fn!(sta_zero_page_x, store_zero_page_x(A));
    impl_store_fn!(sta_absolute, store_absolute(A));
    impl_store_fn!(sta_absolute_x, store_absolute_x(A));
    impl_store_fn!(sta_absolute_y, store_absolute_y(A));
    impl_store_fn!(sta_zero_page_indirect, store_zero_page_indirect(A));
    impl_store_fn!(sta_zero_page_indirect_x, store_zero_page_indirect_x(A));
    impl_store_fn!(sta_zero_page_indirect_y, store_zero_page_indirect_y(A));

    // STX: Store X
    impl_store_fn!(stx_zero_page, store_zero_page(X));
    impl_store_fn!(stx_zero_page_y, store_zero_page_y(X));
    impl_store_fn!(stx_absolute, store_absolute(X));

    // STY: Store Y
    impl_store_fn!(sty_zero_page, store_zero_page(Y));
    impl_store_fn!(sty_zero_page_x, store_zero_page_x(Y));
    impl_store_fn!(sty_absolute, store_absolute(Y));

    // STZ: Store zero
    impl_store_fn!(stz_zero_page, store_zero_page(Z));
    impl_store_fn!(stz_zero_page_x, store_zero_page_x(Z));
    impl_store_fn!(stz_absolute, store_absolute(Z));
    impl_store_fn!(stz_absolute_x, store_absolute_x(Z));

    fn modify_accumulator(&mut self, op: impl FnOnce(&mut Self, u8) -> u8) {
        self.bus_idle();

        self.cpu.registers.a = op(self, self.cpu.registers.a);
    }

    #[inline(always)]
    fn modify_zero_page_indexed(
        &mut self,
        index: Option<IndexRegister>,
        op: impl FnOnce(&mut Self, u8) -> u8,
    ) {
        let zero_page_addr = self.fetch_operand();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let mapped_addr = self.map_zero_page(zero_page_addr.wrapping_add(offset));
        let operand = self.bus_read(mapped_addr);
        self.bus_idle();

        let result = op(self, operand);
        self.bus_write(mapped_addr, result);
    }

    fn modify_zero_page(&mut self, op: impl FnOnce(&mut Self, u8) -> u8) {
        self.modify_zero_page_indexed(None, op);
    }

    fn modify_zero_page_x(&mut self, op: impl FnOnce(&mut Self, u8) -> u8) {
        self.modify_zero_page_indexed(Some(IndexRegister::X), op);
    }

    #[inline(always)]
    fn modify_absolute_indexed(
        &mut self,
        index: Option<IndexRegister>,
        op: impl FnOnce(&mut Self, u8) -> u8,
    ) {
        let address = self.fetch_operand_u16();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let mapped_addr = self.map_address(address.wrapping_add(offset.into()));
        let operand = self.bus_read(mapped_addr);
        self.bus_idle();

        let result = op(self, operand);
        self.bus_write(mapped_addr, result);
    }

    fn modify_absolute(&mut self, op: impl FnOnce(&mut Self, u8) -> u8) {
        self.modify_absolute_indexed(None, op);
    }

    fn modify_absolute_x(&mut self, op: impl FnOnce(&mut Self, u8) -> u8) {
        self.modify_absolute_indexed(Some(IndexRegister::X), op);
    }

    // ASL: Shift left
    fn asl(&mut self, operand: u8) -> u8 {
        let result = operand << 1;
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
        self.cpu.registers.p.carry = operand.bit(7);

        result
    }

    impl_modify_fn!(asl_accumulator, modify_accumulator(asl));
    impl_modify_fn!(asl_zero_page, modify_zero_page(asl));
    impl_modify_fn!(asl_zero_page_x, modify_zero_page_x(asl));
    impl_modify_fn!(asl_absolute, modify_absolute(asl));
    impl_modify_fn!(asl_absolute_x, modify_absolute_x(asl));

    // LSR: Logical shift right
    fn lsr(&mut self, operand: u8) -> u8 {
        let result = operand >> 1;
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = false;
        self.cpu.registers.p.carry = operand.bit(0);

        result
    }

    impl_modify_fn!(lsr_accumulator, modify_accumulator(lsr));
    impl_modify_fn!(lsr_zero_page, modify_zero_page(lsr));
    impl_modify_fn!(lsr_zero_page_x, modify_zero_page_x(lsr));
    impl_modify_fn!(lsr_absolute, modify_absolute(lsr));
    impl_modify_fn!(lsr_absolute_x, modify_absolute_x(lsr));

    // ROL: Rotate left
    fn rol(&mut self, operand: u8) -> u8 {
        let result = (operand << 1) | u8::from(self.cpu.registers.p.carry);
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
        self.cpu.registers.p.carry = operand.bit(7);

        result
    }

    impl_modify_fn!(rol_accumulator, modify_accumulator(rol));
    impl_modify_fn!(rol_zero_page, modify_zero_page(rol));
    impl_modify_fn!(rol_zero_page_x, modify_zero_page_x(rol));
    impl_modify_fn!(rol_absolute, modify_absolute(rol));
    impl_modify_fn!(rol_absolute_x, modify_absolute_x(rol));

    // ROR: Rotate right
    fn ror(&mut self, operand: u8) -> u8 {
        let result = (operand >> 1) | (u8::from(self.cpu.registers.p.carry) << 7);
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
        self.cpu.registers.p.carry = operand.bit(0);

        result
    }

    impl_modify_fn!(ror_accumulator, modify_accumulator(ror));
    impl_modify_fn!(ror_zero_page, modify_zero_page(ror));
    impl_modify_fn!(ror_zero_page_x, modify_zero_page_x(ror));
    impl_modify_fn!(ror_absolute, modify_absolute(ror));
    impl_modify_fn!(ror_absolute_x, modify_absolute_x(ror));

    // INC: Increment
    fn inc(&mut self, operand: u8) -> u8 {
        let result = operand.wrapping_add(1);
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);

        result
    }

    impl_modify_fn!(inc_accumulator, modify_accumulator(inc));
    impl_modify_fn!(inc_zero_page, modify_zero_page(inc));
    impl_modify_fn!(inc_zero_page_x, modify_zero_page_x(inc));
    impl_modify_fn!(inc_absolute, modify_absolute(inc));
    impl_modify_fn!(inc_absolute_x, modify_absolute_x(inc));

    // DEC: Decrement
    fn dec(&mut self, operand: u8) -> u8 {
        let result = operand.wrapping_sub(1);
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);

        result
    }

    impl_modify_fn!(dec_accumulator, modify_accumulator(dec));
    impl_modify_fn!(dec_zero_page, modify_zero_page(dec));
    impl_modify_fn!(dec_zero_page_x, modify_zero_page_x(dec));
    impl_modify_fn!(dec_absolute, modify_absolute(dec));
    impl_modify_fn!(dec_absolute_x, modify_absolute_x(dec));

    // INX: Increment X
    fn inx(&mut self) {
        let result = self.cpu.registers.x.wrapping_add(1);
        self.cpu.registers.x = result;

        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
    }

    // INY: Increment Y
    fn iny(&mut self) {
        let result = self.cpu.registers.y.wrapping_add(1);
        self.cpu.registers.y = result;

        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
    }

    // DEX: Decrement X
    fn dex(&mut self) {
        let result = self.cpu.registers.x.wrapping_sub(1);
        self.cpu.registers.x = result;

        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
    }

    // DEY: Decrement Y
    fn dey(&mut self) {
        let result = self.cpu.registers.y.wrapping_sub(1);
        self.cpu.registers.y = result;

        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.negative = result.bit(7);
    }

    #[inline(always)]
    fn conditional_branch(&mut self, condition: bool) {
        let displacement = self.fetch_operand() as i8;

        if !condition {
            return;
        }

        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.pc = self.cpu.registers.pc.wrapping_add_signed(displacement.into());
    }

    // BRA: Branch always
    impl_branch!(bra);

    // BCC: Branch on carry clear
    // BCS: Branch on carry set
    // BEQ: Branch on equal
    // BMI: Branch on minus
    // BNE: Branch on not equal
    // BPL: Branch on plus
    // BVC: Branch on overflow clear
    // BVS: Branch on overflow set
    impl_branch!(bcc, carry == false);
    impl_branch!(bcs, carry == true);
    impl_branch!(beq, zero == true);
    impl_branch!(bmi, negative == true);
    impl_branch!(bne, zero == false);
    impl_branch!(bpl, negative == false);
    impl_branch!(bvc, overflow == false);
    impl_branch!(bvs, overflow == true);

    // JMP: Jump to new location
    fn jmp_absolute(&mut self) {
        self.cpu.registers.pc = self.fetch_operand_u16();

        self.bus_idle();
    }

    #[inline(always)]
    fn jmp_indirect_indexed(&mut self, index: Option<IndexRegister>) {
        let address = self.fetch_operand_u16();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = address.wrapping_add(offset.into());

        let pc_lsb = self.bus_read(self.map_address(indexed_addr));
        let pc_msb = self.bus_read(self.map_address(indexed_addr.wrapping_add(1)));
        self.cpu.registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        self.bus_idle();
    }

    // JMP: Jump to new location
    fn jmp_absolute_indirect(&mut self) {
        self.jmp_indirect_indexed(None);
    }

    // JMP: Jump to new location
    fn jmp_absolute_indirect_x(&mut self) {
        self.jmp_indirect_indexed(Some(IndexRegister::X));
    }

    // JSR: Jump to subroutine
    fn jsr(&mut self) {
        let pc_lsb = self.fetch_operand();
        self.bus_idle();

        self.push_stack_u16(self.cpu.registers.pc);

        let pc_msb = self.fetch_operand();
        self.cpu.registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        self.bus_idle();
    }

    // BSR: Branch to subroutine
    fn bsr(&mut self) {
        let displacement = self.fetch_operand() as i8;
        self.bus_idle();
        self.bus_idle();

        self.push_stack_u16(self.cpu.registers.pc.wrapping_sub(1));
        self.bus_idle();

        self.cpu.registers.pc = self.cpu.registers.pc.wrapping_add_signed(displacement.into());

        self.bus_idle();
    }

    // RTS: Return from subroutine
    fn rts(&mut self) {
        self.bus_idle();
        self.bus_idle();
        self.cpu.registers.pc = self.pull_stack_u16();
        self.bus_idle();

        self.fetch_operand(); // Advance PC
    }

    // RTI: Return from interrupt
    fn rti(&mut self) {
        self.bus_idle();
        self.cpu.registers.p = self.pull_stack().into();
        self.cpu.registers.pc = self.pull_stack_u16();
        self.bus_idle();

        self.bus_idle();
    }

    // PHA: Push A
    fn pha(&mut self) {
        self.bus_idle();

        self.push_stack(self.cpu.registers.a);
    }

    // PHP: Push P
    fn php(&mut self) {
        self.bus_idle();

        // PHP always pushes P with the B flag set
        self.push_stack(self.cpu.registers.p.to_u8_brk());
    }

    // PHX: Push X
    fn phx(&mut self) {
        self.bus_idle();

        self.push_stack(self.cpu.registers.x);
    }

    // PHY: Push Y
    fn phy(&mut self) {
        self.bus_idle();

        self.push_stack(self.cpu.registers.y);
    }

    // PLA: Pull A
    fn pla(&mut self) {
        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.a = self.pull_stack();
    }

    // PLP: Pull P
    fn plp(&mut self) {
        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.p = self.pull_stack().into();
    }

    // PLX: Pull X
    fn plx(&mut self) {
        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.x = self.pull_stack();
    }

    // PLY: Pull Y
    fn ply(&mut self) {
        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.y = self.pull_stack();
    }

    #[inline(always)]
    fn update_memory_bit<const SET: bool>(&mut self, i: u8) {
        let zero_page_addr = self.fetch_operand();
        self.bus_idle();

        let mapped_addr = self.map_zero_page(zero_page_addr);
        let value = self.bus_read(mapped_addr);
        self.bus_idle();
        self.bus_idle();

        let result = if SET { value | (1 << i) } else { value & !(1 << i) };
        self.bus_write(mapped_addr, result);
    }

    // RMBi: Reset memory bit
    fn rmbi(&mut self, opcode: u8) {
        let i = (opcode >> 4) & 7;
        self.update_memory_bit::<false>(i);
    }

    // SMBi: Set memory bit
    fn smbi(&mut self, opcode: u8) {
        let i = (opcode >> 4) & 7;
        self.update_memory_bit::<true>(i);
    }

    #[inline(always)]
    fn branch_on_bit<const SET: bool>(&mut self, i: u8) {
        let zero_page_addr = self.fetch_operand();
        let displacement = self.fetch_operand() as i8;
        self.bus_idle();

        let mapped_addr = self.map_zero_page(zero_page_addr);
        let value = self.bus_read(mapped_addr);

        self.bus_idle();

        if value.bit(i) != SET {
            return;
        }

        self.bus_idle();
        self.bus_idle();

        self.cpu.registers.pc = self.cpu.registers.pc.wrapping_add_signed(displacement.into());
    }

    // BBRi: Branch on bit reset
    fn bbri(&mut self, opcode: u8) {
        let i = (opcode >> 4) & 7;
        self.branch_on_bit::<false>(i);
    }

    // BBSi: Branch on bit set
    fn bbsi(&mut self, opcode: u8) {
        let i = (opcode >> 4) & 7;
        self.branch_on_bit::<true>(i);
    }

    // CLA: Clear A
    fn cla(&mut self) {
        self.bus_idle();

        self.cpu.registers.a = 0;
    }

    // CLX: Clear X
    fn clx(&mut self) {
        self.bus_idle();

        self.cpu.registers.x = 0;
    }

    // CLY: Clear Y
    fn cly(&mut self) {
        self.bus_idle();

        self.cpu.registers.y = 0;
    }

    // CLC: Clear C
    // CLD: Clear D
    // CLI: Clear I
    // CLV: Clear V
    // SEC: Set C
    // SED: Set D
    // SEI: Set I
    // SET: Set T
    impl_set_flag!(clc, carry = false);
    impl_set_flag!(cld, decimal = false);
    impl_set_flag!(cli, irq_disable = false);
    impl_set_flag!(clv, overflow = false);
    impl_set_flag!(sec, carry = true);
    impl_set_flag!(sed, decimal = true);
    impl_set_flag!(sei, irq_disable = true);
    impl_set_flag!(set, memory_op = true);

    // SAX: Swap A for X
    fn sax(&mut self) {
        self.bus_idle();
        self.bus_idle();

        mem::swap(&mut self.cpu.registers.a, &mut self.cpu.registers.x);
    }

    // SAX: Swap A for Y
    fn say(&mut self) {
        self.bus_idle();
        self.bus_idle();

        mem::swap(&mut self.cpu.registers.a, &mut self.cpu.registers.y);
    }

    // SXY: Swap X for Y
    fn sxy(&mut self) {
        self.bus_idle();
        self.bus_idle();

        mem::swap(&mut self.cpu.registers.x, &mut self.cpu.registers.y);
    }

    #[inline(always)]
    fn transfer(&mut self, from: ReadRegister, to: RwRegister) {
        self.bus_idle();

        let value = from.get(&self.cpu.registers);
        to.set(&mut self.cpu.registers, value);

        // TXS does not set flags
        if to != RwRegister::S {
            self.cpu.registers.p.zero = value == 0;
            self.cpu.registers.p.negative = value.bit(7);
        }
    }

    // TAX: Transfer A to X
    fn tax(&mut self) {
        self.transfer(ReadRegister::A, RwRegister::X);
    }

    // TAY: Transfer A to Y
    fn tay(&mut self) {
        self.transfer(ReadRegister::A, RwRegister::Y);
    }

    // TSX: Transfer S to X
    fn tsx(&mut self) {
        self.transfer(ReadRegister::S, RwRegister::X);
    }

    // TXA: Transfer X to A
    fn txa(&mut self) {
        self.transfer(ReadRegister::X, RwRegister::A);
    }

    // TXS: Transfer X to S
    fn txs(&mut self) {
        self.transfer(ReadRegister::X, RwRegister::S);
    }

    // TYA: Transfer Y to A
    fn tya(&mut self) {
        self.transfer(ReadRegister::Y, RwRegister::A);
    }

    // TRB: Test and reset bits
    fn trb(&mut self, operand: u8) -> u8 {
        let result = operand & !self.cpu.registers.a;
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.overflow = operand.bit(6);
        self.cpu.registers.p.negative = operand.bit(7);

        result
    }

    impl_modify_fn!(trb_zero_page, modify_zero_page(trb));
    impl_modify_fn!(trb_absolute, modify_absolute(trb));

    // TSB: Test and set bits
    fn tsb(&mut self, operand: u8) -> u8 {
        let result = operand | self.cpu.registers.a;
        self.cpu.registers.p.zero = result == 0;
        self.cpu.registers.p.overflow = operand.bit(6);
        self.cpu.registers.p.negative = operand.bit(7);

        result
    }

    impl_modify_fn!(tsb_zero_page, modify_zero_page(tsb));
    impl_modify_fn!(tsb_absolute, modify_absolute(tsb));

    // TST: Test memory
    fn tst(&mut self, memory_value: u8, operand: u8) {
        self.cpu.registers.p.zero = memory_value & operand == 0;
        self.cpu.registers.p.overflow = memory_value.bit(6);
        self.cpu.registers.p.negative = memory_value.bit(7);
    }

    #[inline(always)]
    fn tst_zero_page_indexed(&mut self, index: Option<IndexRegister>) {
        let operand = self.fetch_operand();
        let zero_page_addr = self.fetch_operand();
        self.bus_idle();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = zero_page_addr.wrapping_add(offset);
        let memory_value = self.bus_read(self.map_zero_page(indexed_addr));
        self.bus_idle();

        self.tst(memory_value, operand);
    }

    #[inline(always)]
    fn tst_absolute_indexed(&mut self, index: Option<IndexRegister>) {
        let operand = self.fetch_operand();
        let address = self.fetch_operand_u16();
        self.bus_idle();
        self.bus_idle();

        let offset = index.map_or(0, |index| index.get(&self.cpu.registers));
        let indexed_addr = address.wrapping_add(offset.into());
        let memory_value = self.bus_read(self.map_address(indexed_addr));
        self.bus_idle();

        self.tst(memory_value, operand);
    }

    fn tst_zero_page(&mut self) {
        self.tst_zero_page_indexed(None);
    }

    fn tst_zero_page_x(&mut self) {
        self.tst_zero_page_indexed(Some(IndexRegister::X));
    }

    fn tst_absolute(&mut self) {
        self.tst_absolute_indexed(None);
    }

    fn tst_absolute_x(&mut self) {
        self.tst_absolute_indexed(Some(IndexRegister::X));
    }

    // ST0/ST1/ST2: Store HuC6270 (VDC)
    fn sti<const I: usize>(&mut self) {
        assert!(I < 3);

        // ST0/ST1/ST2 always write to VDC physical addresses; MPRs are not used
        let vdc_address = match I {
            0 => 0x1FE000,
            1 => 0x1FE002,
            2 => 0x1FE003,
            _ => unreachable!("I must be less than 3"),
        };

        let data = self.fetch_operand();
        self.bus_idle();
        self.bus_write(vdc_address, data);
    }

    fn st0(&mut self) {
        self.sti::<0>();
    }

    fn st1(&mut self) {
        self.sti::<1>();
    }

    fn st2(&mut self) {
        self.sti::<2>();
    }

    // TAMi: Transfer A to MPR
    fn tam(&mut self) {
        self.bus_idle();
        self.bus_idle();
        self.bus_idle();

        let bits = self.fetch_operand();
        if bits != 0 {
            self.cpu.state.mpr_buffer = self.cpu.registers.a;

            // If multiple bits are set, A gets copied to each MPR whose bit is set
            for i in 0..8 {
                if bits.bit(i) {
                    self.cpu.registers.mpr[i as usize] = self.cpu.registers.a;
                }
            }
        }
    }

    // TMAi: Transfer MPR to A
    fn tma(&mut self) {
        self.bus_idle();
        self.bus_idle();

        let bits = self.fetch_operand();
        self.cpu.registers.a = match bits {
            0 => self.cpu.state.mpr_buffer,
            _ => {
                // If multiple bits are set, the results are "combined" in some way
                // Unclear exactly how; this is probably not accurate
                let mut value = 0xFF;
                for i in 0..8 {
                    if bits.bit(i) {
                        value &= self.cpu.registers.mpr[i as usize];
                    }
                }
                value
            }
        };

        self.cpu.state.mpr_buffer = self.cpu.registers.a;
    }

    // CSH: Clock speed high
    fn csh(&mut self) {
        self.bus_idle();
        self.bus.set_clock_speed(ClockSpeed::High);
        self.bus_idle();
    }

    // CSL: Clock speed low
    fn csl(&mut self) {
        self.bus_idle();
        self.bus.set_clock_speed(ClockSpeed::Low);
        self.bus_idle();
    }

    fn start_block_transfer(
        &mut self,
        source_step: BlockTransferStep,
        destination_step: BlockTransferStep,
    ) {
        self.bus_idle();

        self.push_stack(self.cpu.registers.y);
        self.push_stack(self.cpu.registers.a);
        self.push_stack(self.cpu.registers.x);

        self.bus_idle();

        let source = self.fetch_operand_u16();
        let destination = self.fetch_operand_u16();
        let length = self.fetch_operand_u16();

        self.bus_idle();

        self.cpu.state.block_transfer = Some(BlockTransferState {
            source,
            destination,
            length,
            source_step,
            destination_step,
            count: 0,
        });
    }

    fn progress_block_transfer(&mut self) {
        let Some(state) = &mut self.cpu.state.block_transfer else {
            panic!("progress_block_transfer() called when block transfer state is None");
        };

        self.bus.idle();

        let source_addr = self.cpu.registers.map_address(state.source);
        let value = match source_addr {
            0x1FE800..=0x1FF7FF => 0, // Block transfer cannot read from non-VDC I/O addresses
            _ => self.bus.read(source_addr),
        };

        self.bus.idle();

        let dest_addr = self.cpu.registers.map_address(state.destination);
        self.bus.write(dest_addr, value);

        self.bus.idle();
        self.bus.idle();

        state.source = state.source_step.apply(state.source, state.count);
        state.destination = state.destination_step.apply(state.destination, state.count);
        state.count = state.count.wrapping_add(1);

        let length_overflow;
        (state.length, length_overflow) = state.length.overflowing_sub(1);

        if length_overflow {
            self.bus.idle();

            self.cpu.registers.x = self.pull_stack();
            self.cpu.registers.a = self.pull_stack();
            self.cpu.registers.y = self.pull_stack();

            self.cpu.state.block_transfer = None;
        }
    }

    // TAI: Transfer block data (source alternate, dest increment)
    fn tai(&mut self) {
        self.start_block_transfer(BlockTransferStep::Alternate, BlockTransferStep::Increment);
    }

    // TDD: Transfer block data (source decrement, dest decrement)
    fn tdd(&mut self) {
        self.start_block_transfer(BlockTransferStep::Decrement, BlockTransferStep::Decrement);
    }

    // TIA: Transfer block data (source increment, dest alternate)
    fn tia(&mut self) {
        self.start_block_transfer(BlockTransferStep::Increment, BlockTransferStep::Alternate);
    }

    // TII: Transfer block data (source increment, dest increment)
    fn tii(&mut self) {
        self.start_block_transfer(BlockTransferStep::Increment, BlockTransferStep::Increment);
    }

    // TIN: Transfer block data (source increment, dest none)
    fn tin(&mut self) {
        self.start_block_transfer(BlockTransferStep::Increment, BlockTransferStep::None);
    }
}
