mod load;

use crate::traits::{BusInterface, GetBit, SignBit};

#[derive(Debug, Clone, Copy)]
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
}

impl Registers {
    pub fn new() -> Self {
        Self {
            data: [0; 8],
            address: [0; 7],
            usp: 0,
            ssp: 0,
            pc: 0,
            ccr: 0.into(),
            interrupt_priority_mask: 0,
            supervisor_mode: true,
            trace_enabled: false,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DataRegister(u8);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddressRegister(u8);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpSize {
    Byte,
    Word,
    LongWord,
}

impl OpSize {
    #[cfg(test)]
    const ALL: [Self; 3] = [Self::Byte, Self::Word, Self::LongWord];
}

trait IncrementStep: Copy {
    fn increment_step_for(register: AddressRegister) -> u32;
}

impl IncrementStep for u8 {
    fn increment_step_for(register: AddressRegister) -> u32 {
        if register.is_stack_pointer() {
            2
        } else {
            1
        }
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
        IndexRegister::Address(AddressRegister(register_number))
    } else {
        IndexRegister::Data(DataRegister(register_number))
    };

    let size = if extension.bit(11) {
        IndexSize::LongWord
    } else {
        IndexSize::SignExtendedWord
    };

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exception {
    AddressError(u32, BusOpType),
    IllegalInstruction,
}

type ExecuteResult<T> = Result<T, Exception>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddressingMode {
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
}

impl AddressingMode {
    fn parse_from(mode: u8, register: u8) -> ExecuteResult<Self> {
        match (mode & 0x07, register & 0x07) {
            (0x00, register) => Ok(Self::DataDirect(DataRegister(register))),
            (0x01, register) => Ok(Self::AddressDirect(AddressRegister(register))),
            (0x02, register) => Ok(Self::AddressIndirect(AddressRegister(register))),
            (0x03, register) => Ok(Self::AddressIndirectPostincrement(AddressRegister(
                register,
            ))),
            (0x04, register) => Ok(Self::AddressIndirectPredecrement(AddressRegister(register))),
            (0x05, register) => Ok(Self::AddressIndirectDisplacement(AddressRegister(register))),
            (0x06, register) => Ok(Self::AddressIndirectIndexed(AddressRegister(register))),
            (0x07, 0x00) => Ok(Self::AbsoluteShort),
            (0x07, 0x01) => Ok(Self::AbsoluteLong),
            (0x07, 0x02) => Ok(Self::PcRelativeDisplacement),
            (0x07, 0x03) => Ok(Self::PcRelativeIndexed),
            (0x07, 0x04) => Ok(Self::Immediate),
            (0x07, 0x05..=0x07) => Err(Exception::IllegalInstruction),
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }

    fn parse_from_opcode(opcode: u16) -> ExecuteResult<Self> {
        let mode = (opcode >> 3) as u8;
        let register = opcode as u8;
        Self::parse_from(mode, register)
    }

    fn is_address_direct(self) -> bool {
        matches!(self, Self::AddressDirect(..))
    }

    fn is_writable(self) -> bool {
        !matches!(
            self,
            Self::PcRelativeDisplacement | Self::PcRelativeIndexed | Self::Immediate
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Instruction {
    Move {
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    },
}

impl Instruction {
    #[allow(clippy::unnecessary_wraps)]
    fn source_addressing_mode(self) -> Option<AddressingMode> {
        match self {
            Self::Move { source, .. } => Some(source),
        }
    }

    fn dest_addressing_mode(self) -> AddressingMode {
        match self {
            Self::Move { dest, .. } => dest,
        }
    }
}

#[derive(Debug)]
struct InstructionExecutor<'registers, 'bus, B> {
    registers: &'registers mut Registers,
    bus: &'bus mut B,
    opcode: u16,
    dest: Option<AddressingMode>,
    source: Option<AddressingMode>,
}

macro_rules! impl_read_immediate {
    (u8, $self:expr) => {{
        let extension = $self.fetch_operand()?;
        Ok(extension as u8)
    }};
    (u16, $self:expr) => {
        $self.fetch_operand()
    };
    (u32, $self:expr) => {{
        let extension0 = $self.fetch_operand()?;
        let extension1 = $self.fetch_operand()?;
        Ok((u32::from(extension0) << 16) | u32::from(extension1))
    }};
}

macro_rules! impl_read_method {
    ($name:ident, $t:tt, $bus_read_method:ident) => {
        fn $name(&mut self, source: AddressingMode) -> ExecuteResult<$t> {
            match source {
                AddressingMode::DataDirect(register) => {
                    Ok(register.read_from(self.registers) as $t)
                }
                AddressingMode::AddressDirect(register) => {
                    Ok(register.read_from(self.registers) as $t)
                }
                AddressingMode::AddressIndirect(register) => {
                    let address = register.read_from(self.registers);
                    self.$bus_read_method(address)
                }
                AddressingMode::AddressIndirectPostincrement(register) => {
                    let address = register.read_from(self.registers);
                    let increment = <$t>::increment_step_for(register);
                    register.write_long_word_to(self.registers, address.wrapping_add(increment));

                    self.$bus_read_method(address)
                }
                AddressingMode::AddressIndirectPredecrement(register) => {
                    let increment = <$t>::increment_step_for(register);
                    let address = register.read_from(self.registers).wrapping_sub(increment);
                    register.write_long_word_to(self.registers, address);
                    self.$bus_read_method(address)
                }
                AddressingMode::AddressIndirectDisplacement(register) => {
                    let extension = self.fetch_operand()?;
                    let displacement = extension as i16;
                    let address = register
                        .read_from(self.registers)
                        .wrapping_add(displacement as u32);
                    self.$bus_read_method(address)
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
                    self.$bus_read_method(address)
                }
                AddressingMode::PcRelativeDisplacement => {
                    let pc = self.registers.pc;
                    let extension = self.fetch_operand()?;
                    let displacement = extension as i16;

                    let address = pc.wrapping_add(displacement as u32);
                    self.$bus_read_method(address)
                }
                AddressingMode::PcRelativeIndexed => {
                    let pc = self.registers.pc;
                    let extension = self.fetch_operand()?;
                    let (index_register, index_size) = parse_index(extension);
                    let index = index_register.read_from(self.registers, index_size);
                    let displacement = extension as i8;

                    let address = pc.wrapping_add(index).wrapping_add(displacement as u32);
                    self.$bus_read_method(address)
                }
                AddressingMode::AbsoluteShort => {
                    let extension = self.fetch_operand()?;
                    let address = extension as i16 as u32;
                    self.$bus_read_method(address)
                }
                AddressingMode::AbsoluteLong => {
                    let extension0 = self.fetch_operand()?;
                    let extension1 = self.fetch_operand()?;
                    let address = (u32::from(extension0) << 16) | u32::from(extension1);
                    self.$bus_read_method(address)
                }
                AddressingMode::Immediate => impl_read_immediate!($t, self),
            }
        }
    };
}

macro_rules! impl_predecrement_write {
    (u8, $self:expr, $register:expr, $value:expr) => {
        let increment = u8::increment_step_for($register);
        let address = $register.read_from($self.registers).wrapping_sub(increment);
        $register.write_long_word_to($self.registers, address);
        $self.bus.write_byte(address, $value);
    };
    (u16, $self:expr, $register:expr, $value:expr) => {
        let address = $register.read_from($self.registers).wrapping_sub(2);
        $register.write_long_word_to($self.registers, address);
        $self.write_bus_word(address, $value)?;
    };
    (u32, $self:expr, $register:expr, $value:expr) => {{
        let high_word = ($value >> 16) as u16;
        let low_word = $value as u16;

        let address = $register.read_from($self.registers).wrapping_sub(2);
        $register.write_long_word_to($self.registers, address);
        $self.write_bus_word(address, low_word)?;

        let address = address.wrapping_sub(2);
        $register.write_long_word_to($self.registers, address);
        $self.write_bus_word(address, high_word)?;
    }};
}

macro_rules! impl_write_method {
    ($name:ident, $t:tt, $register_write_method:ident, $bus_write_method:ident) => {
        fn $name(&mut self, dest: AddressingMode, value: $t) -> ExecuteResult<()> {
            match dest {
                AddressingMode::DataDirect(register) => {
                    register.$register_write_method(self.registers, value);
                }
                AddressingMode::AddressDirect(register) => {
                    register.$register_write_method(self.registers, value);
                }
                AddressingMode::AddressIndirect(register) => {
                    let address = register.read_from(self.registers);
                    self.$bus_write_method(address, value)?;
                }
                AddressingMode::AddressIndirectPostincrement(register) => {
                    let address = register.read_from(self.registers);
                    self.$bus_write_method(address, value)?;

                    let increment = <$t>::increment_step_for(register);
                    register.write_long_word_to(self.registers, address.wrapping_add(increment));
                }
                AddressingMode::AddressIndirectPredecrement(register) => {
                    impl_predecrement_write!($t, self, register, value);
                }
                AddressingMode::AddressIndirectDisplacement(register) => {
                    let extension = self.fetch_operand()?;
                    let displacement = extension as i16;
                    let address = register
                        .read_from(self.registers)
                        .wrapping_add(displacement as u32);
                    self.$bus_write_method(address, value)?
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
                    self.$bus_write_method(address, value)?;
                }
                AddressingMode::AbsoluteShort => {
                    let extension = self.fetch_operand()?;
                    let address = extension as i16 as u32;
                    self.$bus_write_method(address, value)?;
                }
                AddressingMode::AbsoluteLong => {
                    let extension0 = self.fetch_operand()?;
                    let extension1 = self.fetch_operand()?;
                    let address = (u32::from(extension0) << 16) | u32::from(extension1);
                    self.$bus_write_method(address, value)?;
                }
                AddressingMode::PcRelativeDisplacement
                | AddressingMode::PcRelativeIndexed
                | AddressingMode::Immediate => panic!("cannot write with addressing mode {dest:?}"),
            }

            Ok(())
        }
    };
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn new(registers: &'registers mut Registers, bus: &'bus mut B) -> Self {
        Self {
            registers,
            bus,
            opcode: 0,
            dest: None,
            source: None,
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn read_bus_byte(&mut self, address: u32) -> ExecuteResult<u8> {
        Ok(self.bus.read_byte(address))
    }

    #[allow(clippy::unnecessary_wraps)]
    fn write_bus_byte(&mut self, address: u32, value: u8) -> ExecuteResult<()> {
        self.bus.write_byte(address, value);

        Ok(())
    }

    fn read_bus_word(&mut self, address: u32) -> ExecuteResult<u16> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Read));
        }

        Ok(self.bus.read_word(address))
    }

    fn write_bus_word(&mut self, address: u32, value: u16) -> ExecuteResult<()> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Write));
        }

        self.bus.write_word(address, value);

        Ok(())
    }

    fn read_bus_long_word(&mut self, address: u32) -> ExecuteResult<u32> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Read));
        }

        Ok(self.bus.read_long_word(address))
    }

    fn write_bus_long_word(&mut self, address: u32, value: u32) -> ExecuteResult<()> {
        if address % 2 != 0 {
            return Err(Exception::AddressError(address, BusOpType::Write));
        }

        self.bus.write_long_word(address, value);

        Ok(())
    }

    fn fetch_operand(&mut self) -> ExecuteResult<u16> {
        let operand = self.read_bus_word(self.registers.pc)?;
        self.registers.pc = self.registers.pc.wrapping_add(2);

        Ok(operand)
    }

    impl_read_method!(read_byte, u8, read_bus_byte);
    impl_read_method!(read_word, u16, read_bus_word);
    impl_read_method!(read_long_word, u32, read_bus_long_word);

    fn read(&mut self, source: AddressingMode, size: OpSize) -> ExecuteResult<SizedValue> {
        match size {
            OpSize::Byte => self.read_byte(source).map(SizedValue::Byte),
            OpSize::Word => self.read_word(source).map(SizedValue::Word),
            OpSize::LongWord => self.read_long_word(source).map(SizedValue::LongWord),
        }
    }

    impl_write_method!(write_byte, u8, write_byte_to, write_bus_byte);
    impl_write_method!(write_word, u16, write_word_to, write_bus_word);
    impl_write_method!(
        write_long_word,
        u32,
        write_long_word_to,
        write_bus_long_word
    );

    fn write(&mut self, dest: AddressingMode, value: SizedValue) -> ExecuteResult<()> {
        match value {
            SizedValue::Byte(value) => self.write_byte(dest, value),
            SizedValue::Word(value) => self.write_word(dest, value),
            SizedValue::LongWord(value) => self.write_long_word(dest, value),
        }
    }

    fn push_stack_u16(&mut self, value: u16) -> ExecuteResult<()> {
        let sp = if self.registers.supervisor_mode {
            &mut self.registers.ssp
        } else {
            &mut self.registers.usp
        };

        *sp = sp.wrapping_sub(2);
        let address = *sp;
        self.write_bus_word(address, value)
    }

    fn push_stack_u32(&mut self, value: u32) -> ExecuteResult<()> {
        let high_word = (value >> 16) as u16;
        let low_word = value as u16;

        self.push_stack_u16(low_word)?;
        self.push_stack_u16(high_word)?;

        Ok(())
    }

    fn handle_address_error(&mut self, address: u32, op_type: BusOpType) -> ExecuteResult<()> {
        let sr = self.registers.status_register();
        self.registers.trace_enabled = false;
        self.registers.supervisor_mode = true;

        let pc = match (op_type, self.dest, self.source) {
            (BusOpType::Write, Some(AddressingMode::AddressIndirectPredecrement(..)), _) => {
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

        self.push_stack_u32(pc)?;
        self.push_stack_u16(sr)?;
        self.push_stack_u16(self.opcode)?;
        self.push_stack_u32(address)?;

        let status_word =
            (self.opcode & 0xFFE0) | (u16::from(op_type == BusOpType::Read) << 4) | 0x0005;
        self.push_stack_u16(status_word)?;

        let vector = self.bus.read_long_word(12);
        self.registers.pc = vector;

        Ok(())
    }

    fn do_execute(&mut self) -> ExecuteResult<()> {
        let opcode = self.fetch_operand()?;
        self.opcode = opcode;

        let instruction = decode_opcode(opcode)?;
        self.dest = Some(instruction.dest_addressing_mode());
        self.source = instruction.source_addressing_mode();
        log::trace!("Decoded instruction: {instruction:?}");

        match instruction {
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
        }
    }

    fn execute(mut self) {
        match self.do_execute() {
            Ok(()) => {}
            Err(Exception::AddressError(address, op_type)) => {
                log::trace!(
                    "Encountered address error; address={address:08X}, op_type={op_type:?}"
                );
                if self.handle_address_error(address, op_type).is_err() {
                    todo!("halt CPU")
                }
            }
            Err(Exception::IllegalInstruction) => todo!(),
        }
    }
}

fn decode_opcode(opcode: u16) -> ExecuteResult<Instruction> {
    match opcode & 0xF000 {
        0x1000 | 0x2000 | 0x3000 => {
            let size = match opcode & 0xF000 {
                0x1000 => OpSize::Byte,
                0x2000 => OpSize::LongWord,
                0x3000 => OpSize::Word,
                _ => unreachable!("nested match expressions"),
            };

            let source = AddressingMode::parse_from_opcode(opcode)?;

            let dest_mode = (opcode >> 6) as u8;
            let dest_register = (opcode >> 9) as u8;
            let dest = AddressingMode::parse_from(dest_mode, dest_register)?;

            if !dest.is_writable() || (dest.is_address_direct() && size == OpSize::Byte) {
                return Err(Exception::IllegalInstruction);
            }

            Ok(Instruction::Move { size, source, dest })
        }
        _ => Err(Exception::IllegalInstruction),
    }
}

pub struct M68000 {
    registers: Registers,
}

impl M68000 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: Registers::new(),
        }
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

    pub fn execute_instruction<B: BusInterface>(&mut self, bus: &mut B) {
        InstructionExecutor::new(&mut self.registers, bus).execute();
    }
}

impl Default for M68000 {
    fn default() -> Self {
        Self::new()
    }
}
