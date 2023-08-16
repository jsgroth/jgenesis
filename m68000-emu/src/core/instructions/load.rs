use crate::core::instructions::{Direction, UspDirection};
use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    Instruction, InstructionExecutor, OpSize, Registers, ResolvedAddress,
};
use crate::traits::{BusInterface, GetBit, SignBit};

macro_rules! impl_exg {
    ($name:ident, $tx:ty, $ty:ty) => {
        pub(super) fn $name(&mut self, rx: $tx, ry: $ty) -> u32 {
            let x_val = rx.read_from(self.registers);
            let y_val = ry.read_from(self.registers);

            rx.write_long_word_to(self.registers, y_val);
            ry.write_long_word_to(self.registers, x_val);

            0
        }
    };
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn move_(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let value = self.read(source, size)?;

        if !dest.is_address_direct() {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value.is_zero(),
                negative: value.sign_bit(),
                ..self.registers.ccr
            };
        }

        self.write(dest, value)?;

        Ok(0)
    }

    pub(super) fn move_from_sr(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        self.write_word_resolved(dest_resolved, self.registers.status_register())?;

        Ok(0)
    }

    pub(super) fn move_to_ccr(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let value = self.read_word(source)?;

        self.registers.ccr = (value as u8).into();

        Ok(0)
    }

    pub(super) fn move_to_sr(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let value = self.read_word(source)?;

        self.registers.set_status_register(value);

        Ok(0)
    }

    pub(super) fn moveq(&mut self, data: i8, register: DataRegister) -> u32 {
        register.write_long_word_to(self.registers, data as u32);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: data == 0,
            negative: data < 0,
            ..self.registers.ccr
        };

        0
    }

    pub(super) fn move_usp(&mut self, direction: UspDirection, register: AddressRegister) -> u32 {
        match direction {
            UspDirection::RegisterToUsp => {
                let value = register.read_from(self.registers);
                self.registers.usp = value;
            }
            UspDirection::UspToRegister => {
                register.write_long_word_to(self.registers, self.registers.usp);
            }
        }

        0
    }

    pub(super) fn movem(
        &mut self,
        size: OpSize,
        addressing_mode: AddressingMode,
        direction: Direction,
    ) -> ExecuteResult<u32> {
        let extension = self.fetch_operand()?;
        if let AddressingMode::AddressIndirectPredecrement(register) = addressing_mode {
            return self.movem_predecrement(size, register, extension);
        }

        let resolved_address = self.resolve_address(addressing_mode, size)?;

        let iter = MultipleRegisterIter::new(extension);
        let (mut address, postinc_register) = match resolved_address {
            ResolvedAddress::Memory(address) => (address, None),
            ResolvedAddress::MemoryPostincrement {
                address, register, ..
            } => (address, Some(register)),
            _ => panic!("MOVEM only supports addressing modes that resolve to a memory address"),
        };

        match direction {
            Direction::RegisterToMemory => {
                for register in iter {
                    let value = register.read_from(self.registers);
                    match size {
                        OpSize::Word => {
                            self.write_bus_word(address, value as u16)?;
                            address = address.wrapping_add(2);
                        }
                        OpSize::LongWord => {
                            self.write_bus_long_word(address, value)?;
                            address = address.wrapping_add(4);
                        }
                        OpSize::Byte => panic!("MOVEM does not support size byte"),
                    }
                }
            }
            Direction::MemoryToRegister => {
                for register in iter {
                    match size {
                        OpSize::Word => {
                            if let Some(postinc_register) = postinc_register {
                                postinc_register
                                    .write_long_word_to(self.registers, address.wrapping_add(2));
                            }

                            let value = self.read_bus_word(address)? as i16 as u32;
                            match register {
                                Register::Address(register)
                                    if Some(register) == postinc_register => {}
                                _ => {
                                    register.write_to(self.registers, value);
                                }
                            }

                            address = address.wrapping_add(2);
                        }
                        OpSize::LongWord => {
                            if let Some(postinc_register) = postinc_register {
                                postinc_register
                                    .write_long_word_to(self.registers, address.wrapping_add(2));
                            }

                            let value = self.read_bus_long_word(address)?;
                            match register {
                                Register::Address(register)
                                    if Some(register) == postinc_register => {}
                                _ => {
                                    register.write_to(self.registers, value);
                                }
                            }

                            address = address.wrapping_add(4);
                        }
                        OpSize::Byte => panic!("MOVEM does not support size byte"),
                    };
                }

                if let Some(postinc_register) = postinc_register {
                    postinc_register.write_long_word_to(self.registers, address);
                }
            }
        }

        Ok(0)
    }

    fn movem_predecrement(
        &mut self,
        size: OpSize,
        predec_register: AddressRegister,
        extension: u16,
    ) -> ExecuteResult<u32> {
        let iter = MultipleRegisterIter::new_reverse(extension);
        let mut address = predec_register.read_from(self.registers);

        for register in iter {
            match size {
                OpSize::Word => {
                    address = address.wrapping_sub(2);
                    let value = register.read_from(self.registers) as u16;
                    self.write_bus_word(address, value)?;
                }
                OpSize::LongWord => {
                    let value = register.read_from(self.registers);
                    let high_word = (value >> 16) as u16;
                    let low_word = value as u16;

                    address = address.wrapping_sub(2);
                    self.write_bus_word(address, low_word)?;

                    address = address.wrapping_sub(2);
                    self.write_bus_word(address, high_word)?;
                }
                OpSize::Byte => panic!("MOVEM does not support size byte"),
            }
        }

        predec_register.write_long_word_to(self.registers, address);

        Ok(0)
    }

    pub(super) fn movep(
        &mut self,
        size: OpSize,
        d_register: DataRegister,
        a_register: AddressRegister,
        direction: Direction,
    ) -> ExecuteResult<u32> {
        let extension = self.fetch_operand()?;
        let displacement = extension as i16;
        let address = a_register
            .read_from(self.registers)
            .wrapping_add(displacement as u32);

        match (size, direction) {
            (OpSize::Word, Direction::RegisterToMemory) => {
                let value = d_register.read_from(self.registers);
                let [msb, lsb] = (value as u16).to_be_bytes();
                self.bus.write_byte(address, msb);
                self.bus.write_byte(address.wrapping_add(2), lsb);
            }
            (OpSize::Word, Direction::MemoryToRegister) => {
                let msb = self.bus.read_byte(address);
                let lsb = self.bus.read_byte(address.wrapping_add(2));
                d_register.write_word_to(self.registers, u16::from_be_bytes([msb, lsb]));
            }
            (OpSize::LongWord, Direction::RegisterToMemory) => {
                let value = d_register.read_from(self.registers);
                let mut address = address;
                for byte in value.to_be_bytes() {
                    self.bus.write_byte(address, byte);
                    address = address.wrapping_add(2);
                }
            }
            (OpSize::LongWord, Direction::MemoryToRegister) => {
                let b3 = self.bus.read_byte(address);
                let b2 = self.bus.read_byte(address.wrapping_add(2));
                let b1 = self.bus.read_byte(address.wrapping_add(4));
                let b0 = self.bus.read_byte(address.wrapping_add(6));
                let value = u32::from_be_bytes([b3, b2, b1, b0]);
                d_register.write_long_word_to(self.registers, value);
            }
            (OpSize::Byte, _) => panic!("MOVEP does not support size byte"),
        }

        Ok(0)
    }

    impl_exg!(exg_data, DataRegister, DataRegister);
    impl_exg!(exg_address, AddressRegister, AddressRegister);
    impl_exg!(exg_data_address, DataRegister, AddressRegister);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Register {
    Data(DataRegister),
    Address(AddressRegister),
}

impl Register {
    fn read_from(self, registers: &Registers) -> u32 {
        match self {
            Self::Data(register) => register.read_from(registers),
            Self::Address(register) => register.read_from(registers),
        }
    }

    fn write_to(self, registers: &mut Registers, value: u32) {
        match self {
            Self::Data(register) => {
                register.write_long_word_to(registers, value);
            }
            Self::Address(register) => {
                register.write_long_word_to(registers, value);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct MultipleRegisterIter {
    mask: u16,
    i: u8,
    reverse: bool,
}

impl MultipleRegisterIter {
    fn new(extension: u16) -> Self {
        Self {
            mask: extension,
            i: 0,
            reverse: false,
        }
    }

    fn new_reverse(extension: u16) -> Self {
        Self {
            mask: extension,
            i: 0,
            reverse: true,
        }
    }
}

impl Iterator for MultipleRegisterIter {
    type Item = Register;

    fn next(&mut self) -> Option<Self::Item> {
        while self.i < 16 {
            let i = self.i;
            let bit = self.mask.bit(i);
            self.i += 1;

            if bit {
                let i = if self.reverse { 15 - i } else { i };
                return Some(to_register(i));
            }
        }

        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(16))
    }
}

fn to_register(i: u8) -> Register {
    if i < 8 {
        Register::Data(DataRegister(i))
    } else {
        Register::Address(AddressRegister(i - 8))
    }
}

pub(super) fn decode_move(opcode: u16) -> ExecuteResult<Instruction> {
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
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Move { size, source, dest })
}

pub(super) fn decode_movq(opcode: u16) -> ExecuteResult<Instruction> {
    if opcode.bit(8) {
        Err(Exception::IllegalInstruction(opcode))
    } else {
        // MOVEQ
        let data = opcode as i8;
        let register = ((opcode >> 9) & 0x07) as u8;
        Ok(Instruction::MoveQuick(data, register.into()))
    }
}

pub(super) fn decode_move_from_sr(opcode: u16) -> ExecuteResult<Instruction> {
    let dest = AddressingMode::parse_from_opcode(opcode)?;

    if !dest.is_writable() || dest.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveFromSr(dest))
}

pub(super) fn decode_move_to_ccr(opcode: u16) -> ExecuteResult<Instruction> {
    let source = AddressingMode::parse_from_opcode(opcode)?;

    if source.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveToCcr(source))
}

pub(super) fn decode_move_to_sr(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    if !supervisor_mode {
        return Err(Exception::PrivilegeViolation);
    }

    let source = AddressingMode::parse_from_opcode(opcode)?;

    if source.is_address_direct() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveToSr(source))
}

pub(super) fn decode_move_usp(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
    if !supervisor_mode {
        return Err(Exception::PrivilegeViolation);
    }

    let register = (opcode & 0x07) as u8;
    let direction = if opcode.bit(3) {
        UspDirection::UspToRegister
    } else {
        UspDirection::RegisterToUsp
    };

    Ok(Instruction::MoveUsp(direction, register.into()))
}

pub(super) fn decode_movem(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let size = if opcode.bit(6) {
        OpSize::LongWord
    } else {
        OpSize::Word
    };
    let direction = if opcode.bit(10) {
        Direction::MemoryToRegister
    } else {
        Direction::RegisterToMemory
    };

    if matches!(
        addressing_mode,
        AddressingMode::DataDirect(..)
            | AddressingMode::AddressDirect(..)
            | AddressingMode::Immediate
    ) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    if direction == Direction::MemoryToRegister
        && matches!(
            addressing_mode,
            AddressingMode::AddressIndirectPredecrement(..)
        )
    {
        return Err(Exception::IllegalInstruction(opcode));
    }

    if direction == Direction::RegisterToMemory
        && (!addressing_mode.is_writable()
            || matches!(
                addressing_mode,
                AddressingMode::AddressIndirectPostincrement(..)
            ))
    {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::MoveMultiple(size, addressing_mode, direction))
}

pub(super) fn decode_movep(opcode: u16) -> Instruction {
    let a_register = (opcode & 0x07) as u8;
    let d_register = ((opcode >> 9) & 0x07) as u8;

    let size = if opcode.bit(6) {
        OpSize::LongWord
    } else {
        OpSize::Word
    };
    let direction = if opcode.bit(7) {
        Direction::RegisterToMemory
    } else {
        Direction::MemoryToRegister
    };

    Instruction::MovePeripheral(size, d_register.into(), a_register.into(), direction)
}

pub(super) fn decode_exg(opcode: u16) -> ExecuteResult<Instruction> {
    let ry = (opcode & 0x07) as u8;
    let rx = ((opcode >> 9) & 0x07) as u8;

    match opcode & 0b1100_1000 {
        0b0100_0000 => Ok(Instruction::ExchangeData(rx.into(), ry.into())),
        0b0100_1000 => Ok(Instruction::ExchangeAddress(rx.into(), ry.into())),
        0b1000_1000 => Ok(Instruction::ExchangeDataAddress(
            DataRegister(rx),
            AddressRegister(ry),
        )),
        _ => Err(Exception::IllegalInstruction(opcode)),
    }
}
