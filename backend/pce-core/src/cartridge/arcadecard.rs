use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_common::num::GetBit;
use jgenesis_common::{define_2_bit_enum, define_bit_enum};
use std::array;

const SUPER_SYSTEM_CARD_RAM_LEN: usize = super::SUPER_SYSTEM_CARD_RAM_LEN;
const ARCADE_CARD_RAM_LEN: usize = 2 * 1024 * 1024;

type ArcadeRam = [u8; ARCADE_CARD_RAM_LEN];

define_bit_enum!(IncrementMode, [Offset, Base]);
define_2_bit_enum!(AddOffsetTrigger, [None, WriteOffsetLow, WriteOffsetHigh, Write1A0A]);

#[derive(Debug, Clone, Default, Encode, Decode)]
struct RamPort {
    base: u32, // 24-bit address
    offset: u16,
    increment: u16,
    auto_increment: bool,
    increment_mode: IncrementMode,
    add_offset_on_access: bool,
    negative_offset: bool,
    add_offset_trigger: AddOffsetTrigger,
    control: u8, // Full byte value because some bits are R/W but apparently unused
}

impl RamPort {
    fn read_register(&mut self, address: u32, ram: &ArcadeRam) -> u8 {
        match address & 0xF {
            0x0 | 0x1 => self.read_ram(ram),
            0x2 => self.base.to_le_bytes()[0],
            0x3 => self.base.to_le_bytes()[1],
            0x4 => self.base.to_le_bytes()[2],
            0x5 => self.offset.to_le_bytes()[0],
            0x6 => self.offset.to_le_bytes()[1],
            0x7 => self.increment.to_le_bytes()[0],
            0x8 => self.increment.to_le_bytes()[1],
            0x9 => self.control,
            _ => {
                log::warn!("Invalid Arcade Card register read: {:04X}", address & 0x1FFF);
                0xFF
            }
        }
    }

    fn write_register(&mut self, address: u32, value: u8, ram: &mut ArcadeRam) {
        match address & 0xF {
            0x0 | 0x1 => self.write_ram(value, ram),
            0x2 => set_le_byte_u32::<0>(&mut self.base, value),
            0x3 => set_le_byte_u32::<1>(&mut self.base, value),
            0x4 => set_le_byte_u32::<2>(&mut self.base, value),
            0x5 => self.write_offset::<0>(value),
            0x6 => self.write_offset::<1>(value),
            0x7 => set_le_byte_u16::<0>(&mut self.increment, value),
            0x8 => set_le_byte_u16::<1>(&mut self.increment, value),
            0x9 => self.write_control(value),
            0xA => self.write_1a0a(),
            _ => {
                log::warn!(
                    "Invalid Arcade Card register write: {:04X} {value:02X}",
                    address & 0x1FFF
                );
            }
        }
    }

    fn read_ram(&mut self, ram: &ArcadeRam) -> u8 {
        let address = self.get_ram_address();
        ram[address]
    }

    fn write_ram(&mut self, value: u8, ram: &mut ArcadeRam) {
        let address = self.get_ram_address();
        ram[address] = value;
    }

    fn get_ram_address(&mut self) -> usize {
        let mut address = self.base;

        if self.add_offset_on_access {
            address = address.wrapping_add(self.effective_offset());
        }

        if self.auto_increment {
            match self.increment_mode {
                IncrementMode::Offset => {
                    self.offset = self.offset.wrapping_add(self.increment);
                }
                IncrementMode::Base => {
                    self.base = self.base.wrapping_add(self.increment.into()) & 0xFFFFFF;
                }
            }
        }

        (address as usize) & (ARCADE_CARD_RAM_LEN - 1)
    }

    fn effective_offset(&self) -> u32 {
        let high_byte = if self.negative_offset { 0xFF } else { 0x00 };
        u32::from(self.offset) | (high_byte << 16)
    }

    fn write_offset<const BYTE: usize>(&mut self, value: u8) {
        set_le_byte_u16::<BYTE>(&mut self.offset, value);

        if (BYTE == 0 && self.add_offset_trigger == AddOffsetTrigger::WriteOffsetLow)
            || (BYTE == 1 && self.add_offset_trigger == AddOffsetTrigger::WriteOffsetHigh)
        {
            self.add_offset_to_base();
        }
    }

    fn add_offset_to_base(&mut self) {
        self.base = self.base.wrapping_add(self.effective_offset()) & 0xFFFFFF;
    }

    fn write_control(&mut self, value: u8) {
        self.add_offset_trigger = AddOffsetTrigger::from_bits(value >> 5);
        self.increment_mode = IncrementMode::from_bit(value.bit(4));
        self.negative_offset = value.bit(3);
        self.add_offset_on_access = value.bit(1);
        self.auto_increment = value.bit(0);

        self.control = value;
    }

    fn write_1a0a(&mut self) {
        if self.add_offset_trigger == AddOffsetTrigger::Write1A0A {
            self.add_offset_to_base();
        }
    }
}

fn set_le_byte_u32<const BYTE: usize>(value: &mut u32, byte: u8) {
    let shift = 8 * BYTE;
    *value = (*value & !(0xFF << shift)) | (u32::from(byte) << shift);
}

fn set_le_byte_u16<const BYTE: usize>(value: &mut u16, byte: u8) {
    let shift = 8 * BYTE;
    *value = (*value & !(0xFF << shift)) | (u16::from(byte) << shift);
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Shifter {
    value: u32,
    shift: u8,
    rotate: u8,
}

impl Shifter {
    fn read(&self, address: u32) -> u8 {
        match address & 0xF {
            0x0..=0x3 => self.value.to_le_bytes()[(address & 3) as usize],
            0x4 => self.shift,
            0x5 => self.rotate,
            _ => {
                log::warn!("Invalid Arcade Card shifter read: {:04X}", address & 0x1FFF);
                0xFF
            }
        }
    }

    fn write(&mut self, address: u32, value: u8) {
        match address & 0xF {
            0x0 => set_le_byte_u32::<0>(&mut self.value, value),
            0x1 => set_le_byte_u32::<1>(&mut self.value, value),
            0x2 => set_le_byte_u32::<2>(&mut self.value, value),
            0x3 => set_le_byte_u32::<3>(&mut self.value, value),
            0x4 => self.write_shift(value),
            0x5 => self.write_rotate(value),
            _ => {
                log::warn!(
                    "Invalid Arcade Card shifter write: {:04X} {value:02X}",
                    address & 0x1FFF
                );
            }
        }
    }

    fn write_shift(&mut self, value: u8) {
        self.shift = value;

        if !value.bit(3) {
            self.value <<= value & 7;
        } else {
            self.value >>= 8 - (value & 7);
        }
    }

    fn write_rotate(&mut self, value: u8) {
        self.rotate = value;

        if !value.bit(3) {
            self.value = self.value.rotate_left((value & 7).into());
        } else {
            self.value = self.value.rotate_right((8 - (value & 7)).into());
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ArcadeCard {
    super_ram: BoxedByteArray<SUPER_SYSTEM_CARD_RAM_LEN>,
    arcade_ram: BoxedByteArray<ARCADE_CARD_RAM_LEN>,
    ram_ports: [RamPort; 4],
    shifter: Shifter,
}

impl ArcadeCard {
    pub fn new() -> Self {
        Self {
            super_ram: BoxedByteArray::new_random(),
            arcade_ram: BoxedByteArray::new_random(),
            ram_ports: array::from_fn(|_| RamPort::default()),
            shifter: Shifter::default(),
        }
    }

    // Pages $00-$7F
    pub fn read(&mut self, address: u32, rom: &[u8]) -> u8 {
        match address {
            0x000000..=0x07FFFF => super::read_rom_safely(rom, address),
            0x080000..=0x087FFF => {
                let port_idx = ((address >> 13) & 3) as usize;
                self.ram_ports[port_idx].read_ram(self.arcade_ram.as_ref())
            }
            0x088000..=0x0CFFFF => 0xFF, // Unused?
            0x0D0000..=0x0FFFFF => self.super_ram[(address - 0x0D0000) as usize],
            _ => panic!("Invalid cartridge address {address:06X}"),
        }
    }

    // Pages $00-$7F
    pub fn write(&mut self, address: u32, value: u8) {
        match address {
            0x080000..=0x087FFF => {
                let port_idx = ((address >> 13) & 3) as usize;
                self.ram_ports[port_idx].write_ram(value, self.arcade_ram.as_mut());
            }
            0x0D0000..=0x0FFFFF => {
                self.super_ram[(address - 0x0D0000) as usize] = value;
            }
            _ => {}
        }
    }

    // $1800-$1BFF in page $FF
    pub fn read_cd_register(&mut self, address: u32) -> Option<u8> {
        let address = address & 0x1FFF;
        if !(0x1A00..=0x1AFF).contains(&address) {
            return None;
        }

        Some(match address {
            0x1A00..=0x1A7F => {
                let port_idx = ((address >> 4) & 3) as usize;
                self.ram_ports[port_idx].read_register(address, self.arcade_ram.as_ref())
            }
            0x1AE0..=0x1AE5 => self.shifter.read(address),
            0x1AFD..=0x1AFF => {
                // Some sort of version ID, games use this for Arcade Card detection
                [0x00, 0x10, 0x51][(address - 0x1AFD) as usize]
            }
            _ => {
                log::warn!("Invalid Arcade Card read: {address:04X}");
                0xFF
            }
        })
    }

    // $1800-$1BFF in page $FF
    pub fn write_cd_register(&mut self, address: u32, value: u8) {
        let address = address & 0x1FFF;
        if !(0x1A00..=0x1AFF).contains(&address) {
            return;
        }

        match address {
            0x1A00..=0x1A7F => {
                let port_idx = ((address >> 4) & 3) as usize;
                self.ram_ports[port_idx].write_register(address, value, self.arcade_ram.as_mut());
            }
            0x1AE0..=0x1AE5 => self.shifter.write(address, value),
            _ => {
                log::warn!("Invalid Arcade Card write: {address:04X} {value:02X}");
            }
        }
    }

    pub fn debug_super_ram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.super_ram.as_mut_slice())
    }

    pub fn debug_arcade_ram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.arcade_ram.as_mut_slice())
    }
}
