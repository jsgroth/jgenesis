use crate::core::instructions::{Direction, UspDirection};
use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    InstructionExecutor, OpSize, Registers, ResolvedAddress,
};
use crate::traits::BusInterface;
use jgenesis_common::num::{GetBit, SignBit};

macro_rules! impl_move {
    ($name:ident, $read_method:ident, $write_method:ident, $size:expr) => {
        pub(super) fn $name(
            &mut self,
            source: AddressingMode,
            dest: AddressingMode,
        ) -> ExecuteResult<u32> {
            let value = self.$read_method(source)?;

            if !dest.is_address_direct() {
                self.registers.ccr = ConditionCodes {
                    carry: false,
                    overflow: false,
                    zero: value == 0,
                    negative: value.sign_bit(),
                    ..self.registers.ccr
                };
            }

            self.$write_method(dest, value)?;

            // -(An) destinations take 2 fewer cycles than they do in other operations
            let base_cycles = match dest {
                AddressingMode::AddressIndirectPredecrement(..) => 2,
                _ => 4,
            };

            Ok(base_cycles
                + source.address_calculation_cycles($size)
                + dest.address_calculation_cycles($size))
        }
    };
}

macro_rules! impl_exg {
    ($name:ident, $tx:ty, $ty:ty) => {
        pub(super) fn $name(&mut self, rx: $tx, ry: $ty) -> u32 {
            let x_val = rx.read_from(self.registers);
            let y_val = ry.read_from(self.registers);

            rx.write_long_word_to(self.registers, y_val);
            ry.write_long_word_to(self.registers, x_val);

            6
        }
    };
}

impl<B: BusInterface> InstructionExecutor<'_, '_, B> {
    #[inline]
    fn write_long_word_for_move(&mut self, dest: AddressingMode, value: u32) -> ExecuteResult<()> {
        match dest {
            AddressingMode::AddressIndirectPredecrement(register) => {
                let high_word = (value >> 16) as u16;
                let low_word = value as u16;

                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, low_word)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                self.write_bus_word(address, high_word)?;

                Ok(())
            }
            _ => self.write_long_word(dest, value),
        }
    }

    impl_move!(move_byte, read_byte, write_byte, OpSize::Byte);
    impl_move!(move_word, read_word, write_word, OpSize::Word);
    impl_move!(move_long_word, read_long_word, write_long_word_for_move, OpSize::LongWord);

    pub(super) fn move_from_sr(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        self.write_word_resolved(dest_resolved, self.registers.status_register())?;

        Ok(if dest.is_data_direct() {
            6
        } else {
            8 + dest.address_calculation_cycles(OpSize::Word)
        })
    }

    pub(super) fn move_to_ccr(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        let value = self.read_word(source)?;

        self.registers.ccr = (value as u8).into();

        Ok(12 + source.address_calculation_cycles(OpSize::Word))
    }

    pub(super) fn move_to_sr(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
        if !self.registers.supervisor_mode {
            return Err(Exception::PrivilegeViolation);
        }

        let value = self.read_word(source)?;

        self.registers.set_status_register(value);

        Ok(12 + source.address_calculation_cycles(OpSize::Word))
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

        4
    }

    pub(super) fn move_usp(
        &mut self,
        direction: UspDirection,
        register: AddressRegister,
    ) -> ExecuteResult<u32> {
        if !self.registers.supervisor_mode {
            return Err(Exception::PrivilegeViolation);
        }

        match direction {
            UspDirection::RegisterToUsp => {
                let value = register.read_from(self.registers);
                self.registers.usp = value;
            }
            UspDirection::UspToRegister => {
                register.write_long_word_to(self.registers, self.registers.usp);
            }
        }

        Ok(4)
    }

    pub(super) fn movem(
        &mut self,
        size: OpSize,
        addressing_mode: AddressingMode,
        direction: Direction,
    ) -> ExecuteResult<u32> {
        let extension = self.fetch_operand()?;
        log::trace!("Extension {extension:04X}");
        if let AddressingMode::AddressIndirectPredecrement(register) = addressing_mode {
            return self.movem_predecrement(size, register, extension);
        }

        let resolved_address = self.resolve_address(addressing_mode, size)?;

        let iter = MultipleRegisterIter::new(extension);
        let (mut address, postinc_register) = match resolved_address {
            ResolvedAddress::Memory(address) => (address, None),
            ResolvedAddress::MemoryPostincrement { address, register, .. } => {
                (address, Some(register))
            }
            _ => panic!("MOVEM only supports addressing modes that resolve to a memory address"),
        };

        let mut count = 0;
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

                    count += 1;
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
                    }

                    count += 1;
                }

                if let Some(postinc_register) = postinc_register {
                    postinc_register.write_long_word_to(self.registers, address);
                }
            }
        }

        let count_cycles = match size {
            OpSize::Word => 4 * count,
            OpSize::LongWord => 8 * count,
            OpSize::Byte => panic!("MOVEM does not support size byte"),
        };
        Ok(match direction {
            Direction::MemoryToRegister => {
                8 + addressing_mode.address_calculation_cycles(OpSize::Word) + count_cycles
            }
            Direction::RegisterToMemory => {
                4 + addressing_mode.address_calculation_cycles(OpSize::Word) + count_cycles
            }
        })
    }

    fn movem_predecrement(
        &mut self,
        size: OpSize,
        predec_register: AddressRegister,
        extension: u16,
    ) -> ExecuteResult<u32> {
        let iter = MultipleRegisterIter::new_reverse(extension);
        let mut address = predec_register.read_from(self.registers);

        let mut count = 0;
        for register in iter {
            log::trace!("  Copy {register:?}");
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

            count += 1;
        }

        predec_register.write_long_word_to(self.registers, address);

        Ok(8 + match size {
            OpSize::Word => 4 * count,
            OpSize::LongWord => 8 * count,
            OpSize::Byte => panic!("MOVEM does not support size byte"),
        })
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
        let address = a_register.read_from(self.registers).wrapping_add(displacement as u32);

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

        Ok(match size {
            OpSize::Word => 16,
            OpSize::LongWord => 24,
            OpSize::Byte => panic!("MOVEP does not support size byte"),
        })
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
        Self { mask: extension, i: 0, reverse: false }
    }

    fn new_reverse(extension: u16) -> Self {
        Self { mask: extension, i: 0, reverse: true }
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
    if i < 8 { Register::Data(DataRegister(i)) } else { Register::Address(AddressRegister(i - 8)) }
}
