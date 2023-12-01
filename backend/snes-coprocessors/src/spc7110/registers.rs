use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext, U24Ext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DirectRomStep {
    #[default]
    One,
    Step,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DirectRomStepTarget {
    #[default]
    Base,
    Offset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DirectRomSpecialAction {
    #[default]
    None,
    Add8Bit,
    Add16BitAfterWrite,
    Add16BitAfterRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct DirectDataRomMode {
    step: DirectRomStep,
    step_target: DirectRomStepTarget,
    offset_enabled: bool,
    sign_extend_step: bool,
    sign_extend_offset: bool,
    special_action: DirectRomSpecialAction,
}

impl From<u8> for DirectDataRomMode {
    fn from(value: u8) -> Self {
        Self {
            step: if value.bit(0) { DirectRomStep::Step } else { DirectRomStep::One },
            step_target: if value.bit(4) {
                DirectRomStepTarget::Offset
            } else {
                DirectRomStepTarget::Base
            },
            offset_enabled: value.bit(1),
            sign_extend_step: value.bit(2),
            sign_extend_offset: value.bit(3),
            special_action: match (value >> 5) & 0x03 {
                0x00 => DirectRomSpecialAction::None,
                0x01 => DirectRomSpecialAction::Add8Bit,
                0x02 => DirectRomSpecialAction::Add16BitAfterWrite,
                0x03 => DirectRomSpecialAction::Add16BitAfterRead,
                _ => unreachable!("value & 0x03 is always <= 0x03"),
            },
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct MathRegisters {
    pub dividend: u32,
    pub multiplier: u16,
    pub divisor: u16,
    pub result: u32,
    pub remainder: u16,
    pub signed: bool,
}

impl MathRegisters {
    fn execute_multiplication(&mut self) {
        let product = if self.signed {
            (self.dividend as i16 as u32).wrapping_mul(self.multiplier as i16 as u32)
        } else {
            u32::from(self.dividend as u16) * u32::from(self.multiplier)
        };
        self.result = product;
    }

    fn execute_division(&mut self) {
        if self.divisor == 0 {
            // Divide by zero
            self.result = 0;
            self.remainder = self.dividend as u16;
            return;
        }

        let (quotient, remainder) = if self.signed {
            let dividend = self.dividend as i32;
            let divisor: i32 = (self.divisor as i16).into();

            ((dividend / divisor) as u32, (dividend % divisor) as u16)
        } else {
            let divisor: u32 = self.divisor.into();
            (self.dividend / divisor, (self.dividend % divisor) as u16)
        };
        self.result = quotient;
        self.remainder = remainder;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Registers {
    pub sram_enabled: bool,
    pub rom_bank_d: u8,
    pub rom_bank_e: u8,
    pub rom_bank_f: u8,
    pub direct_data_rom_initialized: bool,
    pub direct_data_rom_base: u32,
    pub direct_data_rom_offset: u16,
    pub direct_data_rom_step: u16,
    pub direct_data_rom_mode: DirectDataRomMode,
    pub direct_data_rom_mode_byte: u8,
    pub r4814_written: bool,
    pub r4815_written: bool,
    pub math: MathRegisters,
    // Functionality unknown; treat as read/write register
    pub sram_bank: u8,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            sram_enabled: false,
            rom_bank_d: 0x00,
            rom_bank_e: 0x01,
            rom_bank_f: 0x02,
            direct_data_rom_initialized: false,
            direct_data_rom_base: 0,
            direct_data_rom_offset: 0,
            direct_data_rom_step: 0,
            direct_data_rom_mode: DirectDataRomMode::default(),
            direct_data_rom_mode_byte: 0,
            r4814_written: false,
            r4815_written: false,
            math: MathRegisters::default(),
            sram_bank: 0,
        }
    }

    pub fn read_sram_enabled(&self) -> u8 {
        u8::from(self.sram_enabled) << 7
    }

    pub fn write_sram_enabled(&mut self, value: u8) {
        self.sram_enabled = value.bit(7);
    }

    pub fn read_direct_data_rom_r4810(&mut self, data_rom: &[u8]) -> u8 {
        if !self.direct_data_rom_initialized {
            return 0;
        }

        // Register $4810: Read from either ROM[Base] or ROM[Base+Offset] depending on mode, and then
        // increment either Base or Offset depending on mode
        let mode = self.direct_data_rom_mode;
        let rom_addr = if mode.offset_enabled {
            let offset = extend_u16(self.direct_data_rom_offset, mode.sign_extend_offset);
            self.direct_data_rom_base.wrapping_add(offset) & 0xFFFFFF
        } else {
            self.direct_data_rom_base
        };
        let byte = data_rom.get(rom_addr as usize).copied().unwrap_or(0);

        let step = match mode.step {
            DirectRomStep::One => 1,
            DirectRomStep::Step => extend_u16(self.direct_data_rom_step, mode.sign_extend_step),
        };

        match mode.step_target {
            DirectRomStepTarget::Base => {
                self.direct_data_rom_base = self.direct_data_rom_base.wrapping_add(step) & 0xFFFFFF;
            }
            DirectRomStepTarget::Offset => {
                self.direct_data_rom_offset = self.direct_data_rom_offset.wrapping_add(step as u16);
            }
        }

        byte
    }

    pub fn read_direct_data_rom_r481a(&mut self, data_rom: &[u8]) -> u8 {
        if !self.direct_data_rom_initialized {
            return 0;
        }

        // Register $481A: Read from ROM[Base+Offset], and potentially set Base = Base+Offset depending
        // on mode
        let offset =
            extend_u16(self.direct_data_rom_offset, self.direct_data_rom_mode.sign_extend_offset);
        let rom_addr = self.direct_data_rom_base.wrapping_add(offset) & 0xFFFFFF;
        let byte = data_rom.get(rom_addr as usize).copied().unwrap_or(0);

        if self.direct_data_rom_mode.special_action == DirectRomSpecialAction::Add16BitAfterRead {
            self.direct_data_rom_base = rom_addr;
        }

        byte
    }

    pub fn write_direct_data_rom_base_low(&mut self, value: u8) {
        self.direct_data_rom_base.set_low_byte(value);
    }

    pub fn write_direct_data_rom_base_mid(&mut self, value: u8) {
        self.direct_data_rom_base.set_mid_byte(value);
    }

    pub fn write_direct_data_rom_base_high(&mut self, value: u8) {
        self.direct_data_rom_base.set_high_byte(value);

        // Direct data ROM reads are "initialized" after the first write to this register
        // Pre-initialization reads from $4810 and $481A always return 0
        self.direct_data_rom_initialized = true;
    }

    pub fn write_direct_data_rom_offset_low(&mut self, value: u8) {
        self.direct_data_rom_offset.set_lsb(value);
        self.r4814_written = true;

        if self.r4814_written && self.r4815_written {
            self.apply_mode_write();
        }
    }

    pub fn write_direct_data_rom_offset_high(&mut self, value: u8) {
        self.direct_data_rom_offset.set_msb(value);
        self.r4815_written = true;

        if self.r4814_written && self.r4815_written {
            self.apply_mode_write();
        }
    }

    fn apply_mode_write(&mut self) {
        let mode = self.direct_data_rom_mode_byte.into();
        self.direct_data_rom_mode = mode;
        self.r4814_written = false;
        self.r4815_written = false;

        // 2 of the 3 special actions apply after $4814 and $4815 are written
        match mode.special_action {
            DirectRomSpecialAction::Add8Bit => {
                let offset = extend_u8(self.direct_data_rom_offset.lsb(), mode.sign_extend_offset);
                self.direct_data_rom_base =
                    self.direct_data_rom_base.wrapping_add(offset) & 0xFFFFFF;
            }
            DirectRomSpecialAction::Add16BitAfterWrite => {
                let offset = extend_u16(self.direct_data_rom_offset, mode.sign_extend_offset);
                self.direct_data_rom_base =
                    self.direct_data_rom_base.wrapping_add(offset) & 0xFFFFFF;
            }
            DirectRomSpecialAction::None | DirectRomSpecialAction::Add16BitAfterRead => {}
        }
    }

    pub fn write_direct_data_rom_step_low(&mut self, value: u8) {
        self.direct_data_rom_step.set_lsb(value);
    }

    pub fn write_direct_data_rom_step_high(&mut self, value: u8) {
        self.direct_data_rom_step.set_msb(value);
    }

    pub fn write_direct_data_rom_mode(&mut self, value: u8) {
        // Direct data ROM mode changes are not applied immediately, only after $4814 and $4815 are
        // written (direct data ROM offset)
        self.direct_data_rom_mode_byte = value;
        self.r4814_written = false;
        self.r4815_written = false;

        // Writing to this register seems to reset offset? Momotarou Dentetsu Happy has glitchy
        // audio without doing this
        self.direct_data_rom_offset = 0;
    }

    pub fn read_dividend(&self, address: u32) -> u8 {
        // Dividend is at $4820-$4823
        let shift = 8 * (address & 0x3);
        (self.math.dividend >> shift) as u8
    }

    pub fn write_dividend(&mut self, address: u32, value: u8) {
        // Divdend is at $4820-$4823
        let shift = 8 * (address & 0x3);
        self.math.dividend = (self.math.dividend & !(0xFF << shift)) | (u32::from(value) << shift);
    }

    pub fn write_multiplier_low(&mut self, value: u8) {
        self.math.multiplier.set_lsb(value);
    }

    pub fn write_multiplier_high(&mut self, value: u8) {
        self.math.multiplier.set_msb(value);

        // Writing multiplier MSB initiates multiplication
        self.math.execute_multiplication();
    }

    pub fn write_divisor_low(&mut self, value: u8) {
        self.math.divisor.set_lsb(value);
    }

    pub fn write_divisor_high(&mut self, value: u8) {
        self.math.divisor.set_msb(value);

        // Writing divisor MSB initiates division
        self.math.execute_division();
    }

    pub fn read_math_result(&self, address: u32) -> u8 {
        // Result is at $4828-$482B
        let shift = 8 * (address & 0x3);
        (self.math.result >> shift) as u8
    }

    pub fn write_math_mode(&mut self, value: u8) {
        self.math.signed = value.bit(0);
    }
}

fn extend_u8(value: u8, sign_extend: bool) -> u32 {
    if sign_extend { (value as i8 as u32) & 0xFFFFFF } else { value.into() }
}

fn extend_u16(value: u16, sign_extend: bool) -> u32 {
    if sign_extend { (value as i16 as u32) & 0xFFFFFF } else { value.into() }
}
