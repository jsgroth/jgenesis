//! NEC µPD77C25 and µPD96050 CPUs, used in the following SNES coprocessor chips:
//!   * DSP-1 (~16 games, including Super Mario Kart and Pilotwings)
//!   * DSP-2 (1 game, Dungeon Master)
//!   * DSP-3 (1 game, SD Gundam GX)
//!   * DSP-4 (1 game, Top Gear 3000)
//!   * ST010 (1 game, F1 ROC II: Race of Champions)
//!   * ST011 (1 game, Hayazashi Nidan Morita Shougi)

mod instructions;

use crate::common;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::{GetBit, U16Ext};

pub const ST01X_RAM_LEN_BYTES: usize = Upd77c25Variant::St011.ram_len_words() << 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Upd77c25Variant {
    // DSP-1 / DSP-2 / DSP-3 / DSP-4
    Dsp,
    St010,
    St011,
}

impl Upd77c25Variant {
    const fn program_rom_len_opcodes(self) -> usize {
        match self {
            Self::Dsp => 1 << 11,
            Self::St010 | Self::St011 => 1 << 14,
        }
    }

    const fn data_rom_len_words(self) -> usize {
        match self {
            Self::Dsp => 1 << 10,
            Self::St010 | Self::St011 => 1 << 11,
        }
    }

    const fn ram_len_words(self) -> usize {
        match self {
            Self::Dsp => 1 << 8,
            Self::St010 | Self::St011 => 1 << 11,
        }
    }

    const fn stack_len(self) -> u8 {
        match self {
            Self::Dsp => 4,
            Self::St010 | Self::St011 => 8,
        }
    }

    const fn clock_speed(self) -> u64 {
        match self {
            Self::Dsp => 8_000_000,
            Self::St010 => 10_000_000,
            Self::St011 => 15_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct FlagsRegister {
    z: bool,
    c: bool,
    s0: bool,
    s1: bool,
    ov0: bool,
    ov1: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum DataRegisterBits {
    Eight,
    #[default]
    Sixteen,
}

impl DataRegisterBits {
    fn to_bit(self) -> bool {
        self == Self::Eight
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::Eight } else { Self::Sixteen }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct StatusRegister {
    request_for_master: bool,
    user_flag_0: bool,
    user_flag_1: bool,
    dr_busy: bool,
    dr_control: DataRegisterBits,
}

impl StatusRegister {
    fn write(&mut self, value: u16) {
        self.user_flag_1 = value.bit(14);
        self.user_flag_0 = value.bit(13);
        self.dr_control = DataRegisterBits::from_bit(value.bit(10));
        log::trace!("DR control set to {:?}", self.dr_control);
    }
}

impl From<StatusRegister> for u8 {
    fn from(value: StatusRegister) -> Self {
        (u8::from(value.request_for_master) << 7)
            | (u8::from(value.user_flag_1) << 6)
            | (u8::from(value.user_flag_0) << 5)
            | (u8::from(value.dr_busy) << 4)
            | (u8::from(value.dr_control.to_bit()) << 2)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    dp: u16,
    rp: u16,
    pc: u16,
    stack: [u16; 8],
    stack_idx: u8,
    stack_len: u8,
    k: i16,
    l: i16,
    accumulator_a: i16,
    accumulator_b: i16,
    flags_a: FlagsRegister,
    flags_b: FlagsRegister,
    tr: i16,
    trb: i16,
    sr: StatusRegister,
    dr: u16,
    so: u16,
}

impl Registers {
    fn new(variant: Upd77c25Variant) -> Self {
        let stack_len = variant.stack_len();

        Self {
            dp: 0,
            rp: 0x3FF,
            pc: 0,
            stack: [0; 8],
            stack_idx: 0,
            stack_len,
            k: 0,
            l: 0,
            accumulator_a: 0,
            accumulator_b: 0,
            flags_a: FlagsRegister::default(),
            flags_b: FlagsRegister::default(),
            tr: 0,
            trb: 0,
            sr: StatusRegister::default(),
            dr: 0,
            so: 0,
        }
    }

    fn snes_read_data(&mut self) -> u8 {
        match self.sr.dr_control {
            DataRegisterBits::Eight => {
                self.sr.request_for_master = false;

                self.dr.lsb()
            }
            DataRegisterBits::Sixteen => {
                if self.sr.dr_busy {
                    self.sr.dr_busy = false;
                    self.sr.request_for_master = false;

                    self.dr.msb()
                } else {
                    self.sr.dr_busy = true;

                    self.dr.lsb()
                }
            }
        }
    }

    fn snes_write_data(&mut self, value: u8) {
        match self.sr.dr_control {
            DataRegisterBits::Eight => {
                self.sr.request_for_master = false;

                self.dr = value.into();
            }
            DataRegisterBits::Sixteen => {
                if self.sr.dr_busy {
                    self.sr.dr_busy = false;
                    self.sr.request_for_master = false;

                    self.dr.set_msb(value);
                } else {
                    self.sr.dr_busy = true;

                    self.dr.set_lsb(value);
                }
            }
        }
    }

    fn upd_write_data(&mut self, value: u16) {
        log::trace!("Wrote {value:04X} to DR");

        self.dr = value;
        self.sr.request_for_master = true;
    }

    fn reset(&mut self) {
        self.pc = 0;
        self.flags_a = FlagsRegister::default();
        self.flags_b = FlagsRegister::default();
        self.sr = StatusRegister::default();
        self.rp = 0x3FF;
    }

    fn kl(&self) -> i32 {
        i32::from(self.k) * i32::from(self.l)
    }

    fn push_stack(&mut self, pc: u16) {
        self.stack[self.stack_idx as usize] = pc;
        self.stack_idx = (self.stack_idx + 1) & (self.stack_len - 1);
    }

    fn pop_stack(&mut self) -> u16 {
        log::trace!("Returning, current stack IDX is {}", self.stack_idx);

        self.stack_idx = self.stack_idx.wrapping_sub(1) & (self.stack_len - 1);
        self.stack[self.stack_idx as usize]
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Upd77c25 {
    program_rom: Box<[u32]>,
    data_rom: Box<[u16]>,
    ram: Box<[u16]>,
    registers: Registers,
    idling: bool,
    pc_mask: u16,
    dp_mask: u16,
    rp_mask: u16,
    variant: Upd77c25Variant,
    clock_speed: u64,
    snes_mclk_speed: u64,
    master_cycles_product: u64,
}

impl Upd77c25 {
    #[must_use]
    pub fn new(rom: &[u8], variant: Upd77c25Variant, sram: &[u8], timing_mode: TimingMode) -> Self {
        let (program_rom, data_rom) = convert_rom(rom, variant);

        let ram = match variant {
            Upd77c25Variant::Dsp => vec![0; variant.ram_len_words()],
            Upd77c25Variant::St010 | Upd77c25Variant::St011 => {
                convert_to_u16(sram, Endianness::Little)
            }
        };

        let snes_mclk_speed = match timing_mode {
            TimingMode::Ntsc => common::NTSC_MASTER_CLOCK_FREQUENCY,
            TimingMode::Pal => common::PAL_MASTER_CLOCK_FREQUENCY,
        };

        Self {
            program_rom: program_rom.into_boxed_slice(),
            data_rom: data_rom.into_boxed_slice(),
            ram: ram.into_boxed_slice(),
            registers: Registers::new(variant),
            idling: false,
            pc_mask: (variant.program_rom_len_opcodes() - 1) as u16,
            dp_mask: (variant.ram_len_words() - 1) as u16,
            rp_mask: (variant.data_rom_len_words() - 1) as u16,
            variant,
            clock_speed: variant.clock_speed(),
            snes_mclk_speed,
            master_cycles_product: 0,
        }
    }

    pub fn read_data(&mut self) -> u8 {
        let value = self.registers.snes_read_data();

        if !self.registers.sr.request_for_master {
            self.idling = false;
        }

        log::trace!("Data read: {value:02X}");

        value
    }

    pub fn write_data(&mut self, value: u8) {
        self.registers.snes_write_data(value);

        log::trace!("Data write: {value:02X}");

        if !self.registers.sr.request_for_master {
            self.idling = false;
        }
    }

    #[inline]
    #[must_use]
    pub fn read_status(&self) -> u8 {
        self.registers.sr.into()
    }

    #[inline]
    #[must_use]
    pub fn read_ram(&self, address: u32) -> u8 {
        let word = self.ram[((address >> 1) & 0x7FF) as usize];
        if !address.bit(0) { word.lsb() } else { word.msb() }
    }

    #[inline]
    pub fn write_ram(&mut self, address: u32, value: u8) {
        let word_addr = ((address >> 1) & 0x7FF) as usize;
        if !address.bit(0) {
            self.ram[word_addr].set_lsb(value);
        } else {
            self.ram[word_addr].set_msb(value);
        };
    }

    #[inline]
    #[must_use]
    pub fn sram(&self) -> &[u8] {
        bytemuck::cast_slice(self.ram.as_ref())
    }

    #[inline]
    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        if self.idling {
            return;
        }

        self.master_cycles_product += master_cycles_elapsed * self.clock_speed;
        while self.master_cycles_product >= self.snes_mclk_speed {
            instructions::execute(self);
            self.master_cycles_product -= self.snes_mclk_speed;
        }
    }

    pub fn reset(&mut self) {
        self.registers.reset();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endianness {
    Little,
    Big,
}

impl Endianness {
    fn chunk_to_u16(self, chunk: &[u8]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
            Self::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
        }
    }

    fn chunk_to_u32(self, chunk: &[u8]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            Self::Big => u32::from_be_bytes([0, chunk[0], chunk[1], chunk[2]]),
        }
    }
}

// Parse the ROM into program ROM and data ROM
fn convert_rom(rom: &[u8], variant: Upd77c25Variant) -> (Vec<u32>, Vec<u16>) {
    let endianness = detect_program_rom_endianness(rom);
    log::info!("Detected ROM endian-ness: {endianness:?}");

    let program_rom_len = 3 * variant.program_rom_len_opcodes();

    let program_rom = convert_program_rom(&rom[..program_rom_len], endianness);
    let data_rom = convert_to_u16(&rom[program_rom_len..], endianness);
    (program_rom, data_rom)
}

// Convert program ROM from bytes to 24-bit opcodes
fn convert_program_rom(program_rom: &[u8], endianness: Endianness) -> Vec<u32> {
    program_rom.chunks_exact(3).map(|chunk| endianness.chunk_to_u32(chunk)).collect()
}

// Convert data ROM from bytes to 16-bit words
fn convert_to_u16(bytes: &[u8], endianness: Endianness) -> Vec<u16> {
    bytes.chunks_exact(2).map(|chunk| endianness.chunk_to_u16(chunk)).collect()
}

// All program ROMs used for this chip contain the opcode $97C00x in the first 4 opcodes, where
// x is 4 times the opcode number
fn detect_program_rom_endianness(program_rom: &[u8]) -> Endianness {
    for (i, chunk) in program_rom.chunks_exact(3).enumerate().take(4) {
        if chunk == [(i << 2) as u8, 0xC0, 0x97] {
            return Endianness::Little;
        }

        if chunk == [0x97, 0xC0, (i << 2) as u8] {
            return Endianness::Big;
        }
    }

    log::warn!("Unable to detect uPD77C25 endian-ness; defaulting to little-endian");

    Endianness::Little
}
