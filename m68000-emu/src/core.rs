mod instructions;

use crate::core::instructions::Instruction;
use crate::traits::BusInterface;
use jgenesis_traits::num::{GetBit, SignBit};

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "bincode", derive(bincode::Encode, bincode::Decode))]
struct ConditionCodes {
    carry: bool,
    overflow: bool,
    zero: bool,
    negative: bool,
    extend: bool,
}

impl From<u8> for ConditionCodes {
    fn from(value: u8) -> Self {
        Self {
            carry: value.bit(0),
            overflow: value.bit(1),
            zero: value.bit(2),
            negative: value.bit(3),
            extend: value.bit(4),
        }
    }
}

impl From<ConditionCodes> for u8 {
    fn from(value: ConditionCodes) -> Self {
        (u8::from(value.extend) << 4)
            | (u8::from(value.negative) << 3)
            | (u8::from(value.zero) << 2)
            | (u8::from(value.overflow) << 1)
            | u8::from(value.carry)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bincode", derive(bincode::Encode, bincode::Decode))]
struct Registers {
    data: [u32; 8],
    address: [u32; 7],
    usp: u32,
    ssp: u32,
    pc: u32,
    ccr: ConditionCodes,
    interrupt_priority_mask: u8,
    supervisor_mode: bool,
    trace_enabled: bool,
    address_error: bool,
    stopped: bool,
}

const DEFAULT_INTERRUPT_MASK: u8 = 7;

impl Registers {
    pub fn new() -> Self {
        Self {
            data: [0; 8],
            address: [0; 7],
            usp: 0,
            ssp: 0,
            pc: 0,
            ccr: 0.into(),
            interrupt_priority_mask: DEFAULT_INTERRUPT_MASK,
            supervisor_mode: true,
            trace_enabled: false,
            address_error: false,
            stopped: false,
        }
    }

    fn status_register(&self) -> u16 {
        let lsb: u8 = self.ccr.into();
        let msb = self.interrupt_priority_mask
            | (u8::from(self.supervisor_mode) << 5)
            | (u8::from(self.trace_enabled) << 7);

        u16::from_be_bytes([msb, lsb])
    }

    fn set_status_register(&mut self, value: u16) {
        let [msb, lsb] = value.to_be_bytes();

        self.interrupt_priority_mask = msb & 0x07;
        self.supervisor_mode = msb.bit(5);
        self.trace_enabled = msb.bit(7);

        self.ccr = lsb.into();
    }

    fn sp(&self) -> u32 {
        if self.supervisor_mode { self.ssp } else { self.usp }
    }

    fn set_sp(&mut self, sp: u32) {
        if self.supervisor_mode {
            self.ssp = sp;
        } else {
            self.usp = sp;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRegister(u8);

impl DataRegister {
    fn read_from(self, registers: &Registers) -> u32 {
        registers.data[self.0 as usize]
    }

    fn write_byte_to(self, registers: &mut Registers, value: u8) {
        let existing_value = registers.data[self.0 as usize];
        registers.data[self.0 as usize] = (existing_value & 0xFFFF_FF00) | u32::from(value);
    }

    fn write_word_to(self, registers: &mut Registers, value: u16) {
        let existing_value = registers.data[self.0 as usize];
        registers.data[self.0 as usize] = (existing_value & 0xFFFF_0000) | u32::from(value);
    }

    fn write_long_word_to(self, registers: &mut Registers, value: u32) {
        registers.data[self.0 as usize] = value;
    }
}

impl From<u8> for DataRegister {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressRegister(u8);

impl AddressRegister {
    fn is_stack_pointer(self) -> bool {
        self.0 == 7
    }

    fn read_from(self, registers: &Registers) -> u32 {
        match (self.0, registers.supervisor_mode) {
            (7, false) => registers.usp,
            (7, true) => registers.ssp,
            (register, _) => registers.address[register as usize],
        }
    }

    #[allow(clippy::unused_self)]
    fn write_byte_to(self, _registers: &mut Registers, _value: u8) {
        panic!("Writing a byte to an address register is not supported");
    }

    fn write_word_to(self, registers: &mut Registers, value: u16) {
        // Address register writes are always sign extended to 32 bits
        self.write_long_word_to(registers, value as i16 as u32);
    }

    fn write_long_word_to(self, registers: &mut Registers, value: u32) {
        match (self.0, registers.supervisor_mode) {
            (7, false) => {
                registers.usp = value;
            }
            (7, true) => {
                registers.ssp = value;
            }
            (register, _) => {
                registers.address[register as usize] = value;
            }
        }
    }
}

impl From<u8> for AddressRegister {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpSize {
    Byte,
    Word,
    LongWord,
}

impl OpSize {
    fn increment_step_for(self, register: AddressRegister) -> u32 {
        match self {
            Self::Byte => u8::increment_step_for(register),
            Self::Word => u16::increment_step_for(register),
            Self::LongWord => u32::increment_step_for(register),
        }
    }

    fn parse_from_opcode(opcode: u16) -> ExecuteResult<Self> {
        match opcode & 0x00C0 {
            0x0000 => Ok(Self::Byte),
            0x0040 => Ok(Self::Word),
            0x0080 => Ok(Self::LongWord),
            0x00C0 => Err(Exception::IllegalInstruction(opcode)),
            _ => unreachable!("value & 0x00C0 is always 0x0000/0x0040/0x0080/0x00C0"),
        }
    }
}

trait IncrementStep: Copy {
    fn increment_step_for(register: AddressRegister) -> u32;
}

impl IncrementStep for u8 {
    fn increment_step_for(register: AddressRegister) -> u32 {
        if register.is_stack_pointer() { 2 } else { 1 }
    }
}

impl IncrementStep for u16 {
    fn increment_step_for(_register: AddressRegister) -> u32 {
        2
    }
}

impl IncrementStep for u32 {
    fn increment_step_for(_register: AddressRegister) -> u32 {
        4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizedValue {
    Byte(u8),
    Word(u16),
    LongWord(u32),
}

impl SizedValue {
    fn from_size(value: u32, size: OpSize) -> Self {
        match size {
            OpSize::Byte => Self::Byte(value as u8),
            OpSize::Word => Self::Word(value as u16),
            OpSize::LongWord => Self::LongWord(value),
        }
    }

    fn is_zero(self) -> bool {
        match self {
            Self::Byte(value) => value == 0,
            Self::Word(value) => value == 0,
            Self::LongWord(value) => value == 0,
        }
    }
}

impl SignBit for SizedValue {
    fn sign_bit(self) -> bool {
        match self {
            Self::Byte(value) => value.sign_bit(),
            Self::Word(value) => value.sign_bit(),
            Self::LongWord(value) => value.sign_bit(),
        }
    }
}

impl From<SizedValue> for u32 {
    fn from(value: SizedValue) -> Self {
        match value {
            SizedValue::Byte(value) => value.into(),
            SizedValue::Word(value) => value.into(),
            SizedValue::LongWord(value) => value,
        }
    }
}

impl From<u8> for SizedValue {
    fn from(value: u8) -> Self {
        Self::Byte(value)
    }
}

impl From<u16> for SizedValue {
    fn from(value: u16) -> Self {
        Self::Word(value)
    }
}

impl From<u32> for SizedValue {
    fn from(value: u32) -> Self {
        Self::LongWord(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexRegister {
    Data(DataRegister),
    Address(AddressRegister),
}

impl IndexRegister {
    fn read_from(self, registers: &Registers, size: IndexSize) -> u32 {
        let raw_value = match self {
            Self::Data(register) => register.read_from(registers),
            Self::Address(register) => register.read_from(registers),
        };

        match size {
            IndexSize::SignExtendedWord => raw_value as i16 as u32,
            IndexSize::LongWord => raw_value,
        }
    }
}

fn parse_index(extension: u16) -> (IndexRegister, IndexSize) {
    let register_number = ((extension >> 12) & 0x07) as u8;
    let register = if extension.bit(15) {
        IndexRegister::Address(register_number.into())
    } else {
        IndexRegister::Data(register_number.into())
    };

    let size = if extension.bit(11) { IndexSize::LongWord } else { IndexSize::SignExtendedWord };

    (register, size)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexSize {
    SignExtendedWord,
    LongWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusOpType {
    Read,
    Write,
    Jump,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exception {
    AddressError(u32, BusOpType),
    PrivilegeViolation,
    IllegalInstruction(u16),
    DivisionByZero { cycles: u32 },
    Trap(u32),
    CheckRegister { cycles: u32 },
}

type ExecuteResult<T> = Result<T, Exception>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingMode {
    DataDirect(DataRegister),
    AddressDirect(AddressRegister),
    AddressIndirect(AddressRegister),
    AddressIndirectPostincrement(AddressRegister),
    AddressIndirectPredecrement(AddressRegister),
    AddressIndirectDisplacement(AddressRegister),
    AddressIndirectIndexed(AddressRegister),
    PcRelativeDisplacement,
    PcRelativeIndexed,
    AbsoluteShort,
    AbsoluteLong,
    Immediate,
    Quick(u8),
}

impl AddressingMode {
    fn parse_from(mode: u8, register: u8) -> ExecuteResult<Self> {
        match (mode & 0x07, register & 0x07) {
            (0x00, register) => Ok(Self::DataDirect(register.into())),
            (0x01, register) => Ok(Self::AddressDirect(register.into())),
            (0x02, register) => Ok(Self::AddressIndirect(register.into())),
            (0x03, register) => Ok(Self::AddressIndirectPostincrement(register.into())),
            (0x04, register) => Ok(Self::AddressIndirectPredecrement(register.into())),
            (0x05, register) => Ok(Self::AddressIndirectDisplacement(register.into())),
            (0x06, register) => Ok(Self::AddressIndirectIndexed(register.into())),
            (0x07, 0x00) => Ok(Self::AbsoluteShort),
            (0x07, 0x01) => Ok(Self::AbsoluteLong),
            (0x07, 0x02) => Ok(Self::PcRelativeDisplacement),
            (0x07, 0x03) => Ok(Self::PcRelativeIndexed),
            (0x07, 0x04) => Ok(Self::Immediate),
            (0x07, 0x05..=0x07) => {
                Err(Exception::IllegalInstruction(((mode << 3) | register).into()))
            }
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }

    fn parse_from_opcode(opcode: u16) -> ExecuteResult<Self> {
        let mode = (opcode >> 3) as u8;
        let register = opcode as u8;
        Self::parse_from(mode, register)
    }

    fn is_data_direct(self) -> bool {
        matches!(self, Self::DataDirect(..))
    }

    fn is_address_direct(self) -> bool {
        matches!(self, Self::AddressDirect(..))
    }

    fn is_memory(self) -> bool {
        matches!(
            self,
            Self::AddressIndirect(..)
                | Self::AddressIndirectPostincrement(..)
                | Self::AddressIndirectPredecrement(..)
                | Self::AddressIndirectDisplacement(..)
                | Self::AddressIndirectIndexed(..)
                | Self::PcRelativeDisplacement
                | Self::PcRelativeIndexed
                | Self::AbsoluteShort
                | Self::AbsoluteLong
        )
    }

    fn is_writable(self) -> bool {
        !matches!(
            self,
            Self::PcRelativeDisplacement
                | Self::PcRelativeIndexed
                | Self::Immediate
                | Self::Quick(..)
        )
    }

    fn address_calculation_cycles(self, size: OpSize) -> u32 {
        use AddressingMode::{
            AbsoluteLong, AbsoluteShort, AddressDirect, AddressIndirect,
            AddressIndirectDisplacement, AddressIndirectIndexed, AddressIndirectPostincrement,
            AddressIndirectPredecrement, DataDirect, Immediate, PcRelativeDisplacement,
            PcRelativeIndexed, Quick,
        };
        use OpSize::{Byte, LongWord, Word};

        match (self, size) {
            (DataDirect(..) | AddressDirect(..) | Quick(..), _) => 0,
            (AddressIndirect(..) | AddressIndirectPostincrement(..) | Immediate, Byte | Word) => 4,
            (AddressIndirectPredecrement(..), Byte | Word) => 6,
            (
                AddressIndirectDisplacement(..) | PcRelativeDisplacement | AbsoluteShort,
                Byte | Word,
            )
            | (AddressIndirect(..) | AddressIndirectPostincrement(..) | Immediate, LongWord) => 8,
            (AddressIndirectIndexed(..) | PcRelativeIndexed, Byte | Word)
            | (AddressIndirectPredecrement(..), LongWord) => 10,
            (AbsoluteLong, Byte | Word)
            | (
                AddressIndirectDisplacement(..) | PcRelativeDisplacement | AbsoluteShort,
                LongWord,
            ) => 12,
            (AddressIndirectIndexed(..) | PcRelativeIndexed, LongWord) => 14,
            (AbsoluteLong, LongWord) => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedAddress {
    DataRegister(DataRegister),
    AddressRegister(AddressRegister),
    Memory(u32),
    MemoryPostincrement { address: u32, register: AddressRegister, increment: u32 },
    Immediate(u32),
}

impl ResolvedAddress {
    fn apply_post(self, registers: &mut Registers) {
        if let ResolvedAddress::MemoryPostincrement { address, register, increment } = self {
            register.write_long_word_to(registers, address.wrapping_add(increment));
        }
    }
}

#[derive(Debug)]
struct InstructionExecutor<'registers, 'bus, B> {
    registers: &'registers mut Registers,
    bus: &'bus mut B,
    opcode: u16,
    instruction: Option<Instruction>,
}

const ADDRESS_ERROR_VECTOR: u32 = 3;
const ILLEGAL_OPCODE_VECTOR: u32 = 4;
const DIVIDE_BY_ZERO_VECTOR: u32 = 5;
const CHECK_REGISTER_VECTOR: u32 = 6;
const AUTO_VECTORED_INTERRUPT_BASE_ADDRESS: u32 = 0x60;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn new(registers: &'registers mut Registers, bus: &'bus mut B) -> Self {
        Self { registers, bus, opcode: 0, instruction: None }
    }

    // Read a word from the bus; returns an address error if address is odd
    fn read_bus_word(&mut self, address: u32) -> ExecuteResult<u16> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Read));
        }

        Ok(self.bus.read_word(address))
    }

    // Write a word to the bus; returns an address error if address is odd
    fn write_bus_word(&mut self, address: u32, value: u16) -> ExecuteResult<()> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Write));
        }

        self.bus.write_word(address, value);

        Ok(())
    }

    // Read a long word from the bus; returns an address error if address is odd
    fn read_bus_long_word(&mut self, address: u32) -> ExecuteResult<u32> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Read));
        }

        Ok(self.bus.read_long_word(address))
    }

    // Write a long word to the bus; returns an address error if address is odd
    fn write_bus_long_word(&mut self, address: u32, value: u32) -> ExecuteResult<()> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Write));
        }

        self.bus.write_long_word(address, value);

        Ok(())
    }

    // Fetch a word from the current PC and increment PC; returns an address error if PC is odd
    fn fetch_operand(&mut self) -> ExecuteResult<u16> {
        let operand = self.read_bus_word(self.registers.pc)?;
        self.registers.pc = self.registers.pc.wrapping_add(2);

        Ok(operand)
    }

    // Resolve the given addressing mode to a concrete register, memory location, or immediate value,
    // which may require fetching extension words
    fn resolve_address(
        &mut self,
        addressing_mode: AddressingMode,
        size: OpSize,
    ) -> ExecuteResult<ResolvedAddress> {
        log::trace!("Resolving addressing mode {addressing_mode:?}");

        let resolved_address = match addressing_mode {
            AddressingMode::DataDirect(register) => ResolvedAddress::DataRegister(register),
            AddressingMode::AddressDirect(register) => ResolvedAddress::AddressRegister(register),
            AddressingMode::AddressIndirect(register) => {
                ResolvedAddress::Memory(register.read_from(self.registers))
            }
            AddressingMode::AddressIndirectPredecrement(register) => {
                let increment = size.increment_step_for(register);
                let address = register.read_from(self.registers).wrapping_sub(increment);
                register.write_long_word_to(self.registers, address);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::AddressIndirectPostincrement(register) => {
                let increment = size.increment_step_for(register);
                let address = register.read_from(self.registers);
                ResolvedAddress::MemoryPostincrement { address, register, increment }
            }
            AddressingMode::AddressIndirectDisplacement(register) => {
                let extension = self.fetch_operand()?;
                let displacement = extension as i16;
                let address = register.read_from(self.registers).wrapping_add(displacement as u32);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::AddressIndirectIndexed(register) => {
                let extension = self.fetch_operand()?;
                let (index_register, index_size) = parse_index(extension);
                let index = index_register.read_from(self.registers, index_size);
                let displacement = extension as i8;

                let address = register
                    .read_from(self.registers)
                    .wrapping_add(index)
                    .wrapping_add(displacement as u32);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::PcRelativeDisplacement => {
                let pc = self.registers.pc;
                let extension = self.fetch_operand()?;
                let displacement = extension as i16;
                let address = pc.wrapping_add(displacement as u32);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::PcRelativeIndexed => {
                let pc = self.registers.pc;
                let extension = self.fetch_operand()?;
                let (index_register, index_size) = parse_index(extension);
                let index = index_register.read_from(self.registers, index_size);
                let displacement = extension as i8;

                let address = pc.wrapping_add(index).wrapping_add(displacement as u32);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::AbsoluteShort => {
                let extension = self.fetch_operand()?;
                let address = extension as i16 as u32;
                ResolvedAddress::Memory(address)
            }
            AddressingMode::AbsoluteLong => {
                let extension_0 = self.fetch_operand()?;
                let extension_1 = self.fetch_operand()?;
                let address = (u32::from(extension_0) << 16) | u32::from(extension_1);
                ResolvedAddress::Memory(address)
            }
            AddressingMode::Immediate => {
                let extension_0 = self.fetch_operand()?;
                match size {
                    OpSize::Byte => ResolvedAddress::Immediate((extension_0 as u8).into()),
                    OpSize::Word => ResolvedAddress::Immediate(extension_0.into()),
                    OpSize::LongWord => {
                        let extension_1 = self.fetch_operand()?;
                        let value = (u32::from(extension_0) << 16) | u32::from(extension_1);
                        ResolvedAddress::Immediate(value)
                    }
                }
            }
            AddressingMode::Quick(value) => ResolvedAddress::Immediate(value.into()),
        };

        log::trace!("Resolved to {resolved_address:08X?}");

        Ok(resolved_address)
    }

    // Resolve the given address and, if it is a postincrement address, apply the increment
    fn resolve_address_with_post(
        &mut self,
        addressing_mode: AddressingMode,
        size: OpSize,
    ) -> ExecuteResult<ResolvedAddress> {
        let resolved = self.resolve_address(addressing_mode, size)?;
        resolved.apply_post(self.registers);
        Ok(resolved)
    }

    fn read_byte_resolved(&mut self, resolved_address: ResolvedAddress) -> u8 {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => register.read_from(self.registers) as u8,
            ResolvedAddress::AddressRegister(register) => register.read_from(self.registers) as u8,
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => self.bus.read_byte(address),
            ResolvedAddress::Immediate(value) => value as u8,
        }
    }

    // Read a word from the given location; will return an address error if the location is an odd memory address
    fn read_word_resolved(&mut self, resolved_address: ResolvedAddress) -> ExecuteResult<u16> {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => {
                Ok(register.read_from(self.registers) as u16)
            }
            ResolvedAddress::AddressRegister(register) => {
                Ok(register.read_from(self.registers) as u16)
            }
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => self.read_bus_word(address),
            ResolvedAddress::Immediate(value) => Ok(value as u16),
        }
    }

    // Read a long word from the given location; will return an address error if the location is an odd memory address
    fn read_long_word_resolved(&mut self, resolved_address: ResolvedAddress) -> ExecuteResult<u32> {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => Ok(register.read_from(self.registers)),
            ResolvedAddress::AddressRegister(register) => Ok(register.read_from(self.registers)),
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => {
                self.read_bus_long_word(address)
            }
            ResolvedAddress::Immediate(value) => Ok(value),
        }
    }

    fn read_resolved(
        &mut self,
        resolved_address: ResolvedAddress,
        size: OpSize,
    ) -> ExecuteResult<SizedValue> {
        match size {
            OpSize::Byte => Ok(SizedValue::Byte(self.read_byte_resolved(resolved_address))),
            OpSize::Word => self.read_word_resolved(resolved_address).map(SizedValue::Word),
            OpSize::LongWord => {
                self.read_long_word_resolved(resolved_address).map(SizedValue::LongWord)
            }
        }
    }

    fn read_byte(&mut self, source: AddressingMode) -> ExecuteResult<u8> {
        let resolved_address = self.resolve_address(source, OpSize::Byte)?;
        resolved_address.apply_post(self.registers);
        let value = self.read_byte_resolved(resolved_address);
        Ok(value)
    }

    fn read_word(&mut self, source: AddressingMode) -> ExecuteResult<u16> {
        let resolved_address = self.resolve_address(source, OpSize::Word)?;
        resolved_address.apply_post(self.registers);
        let value = self.read_word_resolved(resolved_address)?;
        Ok(value)
    }

    fn read_long_word(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let resolved_address = self.resolve_address(source, OpSize::LongWord)?;
        resolved_address.apply_post(self.registers);
        let value = self.read_long_word_resolved(resolved_address)?;
        Ok(value)
    }

    fn read(&mut self, source: AddressingMode, size: OpSize) -> ExecuteResult<SizedValue> {
        match size {
            OpSize::Byte => self.read_byte(source).map(SizedValue::Byte),
            OpSize::Word => self.read_word(source).map(SizedValue::Word),
            OpSize::LongWord => self.read_long_word(source).map(SizedValue::LongWord),
        }
    }

    fn write_byte_resolved(&mut self, resolved_address: ResolvedAddress, value: u8) {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => {
                register.write_byte_to(self.registers, value);
            }
            ResolvedAddress::AddressRegister(register) => {
                register.write_byte_to(self.registers, value);
            }
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => {
                self.bus.write_byte(address, value);
            }
            ResolvedAddress::Immediate(..) => panic!("cannot write to immediate addressing mode"),
        }
    }

    fn write_word_resolved(
        &mut self,
        resolved_address: ResolvedAddress,
        value: u16,
    ) -> ExecuteResult<()> {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => {
                register.write_word_to(self.registers, value);
            }
            ResolvedAddress::AddressRegister(register) => {
                register.write_word_to(self.registers, value);
            }
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => {
                self.write_bus_word(address, value)?;
            }
            ResolvedAddress::Immediate(..) => panic!("cannot write to immediate addressing mode"),
        }

        Ok(())
    }

    fn write_long_word_resolved(
        &mut self,
        resolved_address: ResolvedAddress,
        value: u32,
    ) -> ExecuteResult<()> {
        match resolved_address {
            ResolvedAddress::DataRegister(register) => {
                register.write_long_word_to(self.registers, value);
            }
            ResolvedAddress::AddressRegister(register) => {
                register.write_long_word_to(self.registers, value);
            }
            ResolvedAddress::Memory(address)
            | ResolvedAddress::MemoryPostincrement { address, .. } => {
                self.write_bus_long_word(address, value)?;
            }
            ResolvedAddress::Immediate(..) => panic!("cannot write to immediate addressing mode"),
        }

        Ok(())
    }

    fn write_resolved(
        &mut self,
        resolved_address: ResolvedAddress,
        value: SizedValue,
    ) -> ExecuteResult<()> {
        match value {
            SizedValue::Byte(value) => {
                self.write_byte_resolved(resolved_address, value);
                Ok(())
            }
            SizedValue::Word(value) => self.write_word_resolved(resolved_address, value),
            SizedValue::LongWord(value) => self.write_long_word_resolved(resolved_address, value),
        }
    }

    fn write_byte(&mut self, dest: AddressingMode, value: u8) -> ExecuteResult<()> {
        let resolved_address = self.resolve_address(dest, OpSize::Byte)?;
        self.write_byte_resolved(resolved_address, value);
        resolved_address.apply_post(self.registers);

        Ok(())
    }

    fn write_word(&mut self, dest: AddressingMode, value: u16) -> ExecuteResult<()> {
        let resolved_address = self.resolve_address(dest, OpSize::Word)?;
        self.write_word_resolved(resolved_address, value)?;
        resolved_address.apply_post(self.registers);

        Ok(())
    }

    fn write_long_word(&mut self, dest: AddressingMode, value: u32) -> ExecuteResult<()> {
        let resolved_address = self.resolve_address(dest, OpSize::LongWord)?;
        self.write_long_word_resolved(resolved_address, value)?;
        resolved_address.apply_post(self.registers);

        Ok(())
    }

    fn write(&mut self, dest: AddressingMode, value: SizedValue) -> ExecuteResult<()> {
        match (value, dest) {
            (SizedValue::Byte(value), _) => self.write_byte(dest, value),
            (SizedValue::Word(value), _) => self.write_word(dest, value),
            (
                SizedValue::LongWord(value),
                AddressingMode::AddressIndirectPredecrement(register),
            ) => {
                let high_word = (value >> 16) as u16;
                let low_word = value as u16;

                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, low_word)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, high_word)
            }
            (SizedValue::LongWord(value), _) => self.write_long_word(dest, value),
        }
    }

    fn push_stack_u16(&mut self, value: u16) -> ExecuteResult<()> {
        let sp = self.registers.sp().wrapping_sub(2);
        self.registers.set_sp(sp);

        self.write_bus_word(sp, value)?;

        Ok(())
    }

    fn push_stack_u32(&mut self, value: u32) -> ExecuteResult<()> {
        let high_word = (value >> 16) as u16;
        let low_word = value as u16;

        let sp = self.registers.sp().wrapping_sub(4);
        self.registers.set_sp(sp);

        self.write_bus_word(sp, high_word)?;
        self.write_bus_word(sp.wrapping_add(2), low_word)?;

        Ok(())
    }

    fn pop_stack_u16(&mut self) -> ExecuteResult<u16> {
        let sp = self.registers.sp();
        let value = self.read_bus_word(sp)?;

        self.registers.set_sp(sp.wrapping_add(2));

        Ok(value)
    }

    fn pop_stack_u32(&mut self) -> ExecuteResult<u32> {
        let sp = self.registers.sp();
        let value = self.read_bus_long_word(sp)?;

        self.registers.set_sp(sp.wrapping_add(4));

        Ok(value)
    }

    fn handle_address_error(&mut self, address: u32, op_type: BusOpType) -> ExecuteResult<()> {
        let sr = self.registers.status_register();
        let supervisor_mode = self.registers.supervisor_mode;

        self.registers.trace_enabled = false;
        self.registers.supervisor_mode = true;

        let dest = self.instruction.and_then(Instruction::dest_addressing_mode);
        let source = self.instruction.and_then(Instruction::source_addressing_mode);

        let pc = match (op_type, dest, source) {
            (BusOpType::Write, Some(AddressingMode::AddressIndirectPredecrement(..)), Some(_)) => {
                self.registers.pc
            }
            (
                BusOpType::Write,
                Some(AddressingMode::AbsoluteLong),
                Some(
                    AddressingMode::AddressIndirect(..)
                    | AddressingMode::AddressIndirectPostincrement(..)
                    | AddressingMode::AddressIndirectPredecrement(..)
                    | AddressingMode::AddressIndirectDisplacement(..)
                    | AddressingMode::AddressIndirectIndexed(..)
                    | AddressingMode::PcRelativeDisplacement
                    | AddressingMode::PcRelativeIndexed
                    | AddressingMode::AbsoluteShort
                    | AddressingMode::AbsoluteLong,
                ),
            ) => self.registers.pc.wrapping_sub(4),
            _ => self.registers.pc.wrapping_sub(2),
        };

        log::trace!("Address error PC: {pc:08X}");
        self.push_stack_u32(pc)?;
        log::trace!("Address error SR: {sr:08X}");
        self.push_stack_u16(sr)?;
        log::trace!("Address error opcode: {:08X}", self.opcode);
        self.push_stack_u16(self.opcode)?;
        self.push_stack_u32(address)?;

        let rw_bit = (op_type == BusOpType::Read || op_type == BusOpType::Jump)
            ^ matches!(self.instruction, Some(Instruction::MoveFromSr(..)));
        let status_code = match op_type {
            BusOpType::Jump => {
                if supervisor_mode {
                    0x0E
                } else {
                    0x0A
                }
            }
            _ => 0x05,
        };
        let status_word = (self.opcode & 0xFFE0) | (u16::from(rw_bit) << 4) | status_code;
        log::trace!("Pushing status word: {status_word:08X}");
        self.push_stack_u16(status_word)?;

        let vector = self.bus.read_long_word(ADDRESS_ERROR_VECTOR * 4);
        self.registers.pc = vector;

        Ok(())
    }

    fn handle_trap(&mut self, vector: u32, pc: u32) -> ExecuteResult<()> {
        let sr = self.registers.status_register();
        self.registers.trace_enabled = false;
        self.registers.supervisor_mode = true;

        self.push_stack_u32(pc)?;
        self.push_stack_u16(sr)?;

        self.registers.pc = self.bus.read_long_word(vector * 4);

        Ok(())
    }

    fn handle_auto_vectored_interrupt(&mut self, interrupt_level: u8) -> ExecuteResult<u32> {
        let sr = self.registers.status_register();
        self.registers.trace_enabled = false;
        self.registers.supervisor_mode = true;
        self.registers.interrupt_priority_mask = interrupt_level;

        self.push_stack_u32(self.registers.pc)?;
        self.push_stack_u16(sr)?;

        let vector_addr = AUTO_VECTORED_INTERRUPT_BASE_ADDRESS + 4 * u32::from(interrupt_level);
        self.registers.pc = self.bus.read_long_word(vector_addr);

        // TODO this is maybe not correct
        Ok(46)
    }

    fn execute(mut self) -> u32 {
        self.registers.address_error = false;

        // TODO properly handle non-maskable level 7 interrupts?
        let interrupt_level = self.bus.interrupt_level() & 0x07;
        if interrupt_level > self.registers.interrupt_priority_mask {
            log::trace!("Handling interrupt of level {interrupt_level}");
            self.bus.acknowledge_interrupt();
            self.registers.stopped = false;
            return self
                .handle_auto_vectored_interrupt(interrupt_level)
                .unwrap_or_else(|_err| todo!("address error during interrupt service routine"));
        }

        if self.registers.stopped {
            return 4;
        }

        match self.do_execute() {
            Ok(cycles) => cycles,
            Err(Exception::AddressError(address, op_type)) => {
                self.registers.address_error = true;

                log::trace!(
                    "Encountered address error; address={address:08X}, op_type={op_type:?}"
                );
                if self.handle_address_error(address, op_type).is_err() {
                    todo!("halt CPU")
                }

                // Not completely accurate but close enough; this shouldn't occur in real software
                50
            }
            Err(Exception::PrivilegeViolation) => todo!("privilege violation"),
            Err(Exception::IllegalInstruction(opcode)) => {
                log::error!("Illegal opcode executed: {opcode:04X} / {opcode:016b}");
                if self
                    .handle_trap(ILLEGAL_OPCODE_VECTOR, self.registers.pc.wrapping_sub(2))
                    .is_err()
                {
                    todo!("???")
                }

                // TODO this shouldn't happen in real software
                34
            }
            Err(Exception::DivisionByZero { cycles }) => {
                if self
                    .handle_trap(DIVIDE_BY_ZERO_VECTOR, self.registers.pc.wrapping_sub(4))
                    .is_err()
                {
                    todo!("???")
                }

                38 + cycles
            }
            Err(Exception::Trap(vector)) => {
                if self.handle_trap(vector, self.registers.pc).is_err() {
                    todo!("???")
                }

                34
            }
            Err(Exception::CheckRegister { cycles }) => {
                if self.handle_trap(CHECK_REGISTER_VECTOR, self.registers.pc).is_err() {
                    todo!("???")
                }

                30 + cycles
            }
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bincode", derive(bincode::Encode, bincode::Decode))]
pub struct M68000 {
    registers: Registers,
    halted: bool,
}

impl M68000 {
    #[must_use]
    pub fn new() -> Self {
        Self { registers: Registers::new(), halted: false }
    }

    pub fn reset<B: BusInterface>(&mut self, bus: &mut B) {
        // Reset the upper word of the status register
        self.registers.supervisor_mode = true;
        self.registers.trace_enabled = false;
        self.registers.interrupt_priority_mask = DEFAULT_INTERRUPT_MASK;

        self.registers.stopped = false;

        // Read SSP from $000000 and PC from $000004
        self.registers.ssp = bus.read_long_word(0);
        self.registers.pc = bus.read_long_word(4);
    }

    #[must_use]
    pub fn data_registers(&self) -> [u32; 8] {
        self.registers.data
    }

    pub fn set_data_registers(&mut self, registers: [u32; 8]) {
        self.registers.data = registers;
    }

    #[must_use]
    pub fn address_registers(&self) -> [u32; 7] {
        self.registers.address
    }

    #[must_use]
    pub fn user_stack_pointer(&self) -> u32 {
        self.registers.usp
    }

    #[must_use]
    pub fn supervisor_stack_pointer(&self) -> u32 {
        self.registers.ssp
    }

    pub fn set_supervisor_stack_pointer(&mut self, ssp: u32) {
        self.registers.ssp = ssp;
    }

    pub fn set_address_registers(&mut self, registers: [u32; 7], usp: u32, ssp: u32) {
        self.registers.address = registers;
        self.registers.usp = usp;
        self.registers.ssp = ssp;
    }

    #[must_use]
    pub fn status_register(&self) -> u16 {
        self.registers.status_register()
    }

    pub fn set_status_register(&mut self, status_register: u16) {
        self.registers.set_status_register(status_register);
    }

    #[must_use]
    pub fn pc(&self) -> u32 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, pc: u32) {
        self.registers.pc = pc;
    }

    #[must_use]
    pub fn address_error(&self) -> bool {
        self.registers.address_error
    }

    #[must_use]
    pub fn halted(&self) -> bool {
        self.halted
    }

    pub fn set_halted(&mut self, halted: bool) {
        self.halted = halted;
    }

    pub fn execute_instruction<B: BusInterface>(&mut self, bus: &mut B) -> u32 {
        if self.halted {
            return 4;
        }

        InstructionExecutor::new(&mut self.registers, bus).execute()
    }
}

impl Default for M68000 {
    fn default() -> Self {
        Self::new()
    }
}
