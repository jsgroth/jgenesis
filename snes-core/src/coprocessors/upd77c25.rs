//! NEC uPD77C25 CPU, used in the following SNES coprocessor chips:
//!   * DSP-1 (19 games, including Super Mario Kart and Pilotwings)
//!   * DSP-2 (1 game, Dungeon Master)
//!   * DSP-3 (1 game, SD Gundam GX)
//!   * DSP-4 (1 game, Top Gear 3000)
//!   * ST010 (1 game, F1 ROC II: Race of Champions)
//!   * ST011 (1 game, Hayazashi Nidan Morita Shogi)

mod instructions;

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

const DSP_PROGRAM_ROM_LEN_OPCODES: usize = 2048;
const DSP_RAM_LEN_WORDS: usize = 256;

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
}

impl Registers {
    fn new(variant: Upd77c25Variant) -> Self {
        let stack_len = match variant {
            Upd77c25Variant::Dsp => 4,
        };

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
        }
    }

    fn snes_read_data(&mut self) -> u8 {
        match self.sr.dr_control {
            DataRegisterBits::Eight => {
                self.sr.request_for_master = false;

                (self.dr >> 8) as u8
            }
            DataRegisterBits::Sixteen => {
                if self.sr.dr_busy {
                    self.sr.dr_busy = false;
                    self.sr.request_for_master = false;

                    (self.dr >> 8) as u8
                } else {
                    self.sr.dr_busy = true;

                    self.dr as u8
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

                    self.dr = (self.dr & 0x00FF) | (u16::from(value) << 8);
                } else {
                    self.sr.dr_busy = true;

                    self.dr = (self.dr & 0xFF00) | u16::from(value);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upd77c25Variant {
    // DSP-1 / DSP-2 / DSP-3 / DSP-4
    Dsp,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Upd77c25 {
    program_rom: Box<[u32]>,
    data_rom: Box<[u16]>,
    ram: Box<[u16]>,
    registers: Registers,
    idling: bool,
    master_cycles_elapsed: u64,
}

impl Upd77c25 {
    pub fn new(rom: &[u8], variant: Upd77c25Variant) -> Self {
        let (program_rom, data_rom) = convert_rom(rom);
        // TODO ST010/ST011
        let ram = vec![0; DSP_RAM_LEN_WORDS];

        Self {
            program_rom: program_rom.into_boxed_slice(),
            data_rom: data_rom.into_boxed_slice(),
            ram: ram.into_boxed_slice(),
            registers: Registers::new(variant),
            idling: false,
            master_cycles_elapsed: 0,
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

    pub fn read_status(&self) -> u8 {
        self.registers.sr.into()
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        self.master_cycles_elapsed += master_cycles_elapsed;

        // TODO more accurate timing?
        while self.master_cycles_elapsed >= 2 {
            instructions::execute(self);
            self.master_cycles_elapsed -= 2;
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
fn convert_rom(rom: &[u8]) -> (Vec<u32>, Vec<u16>) {
    let endianness = detect_program_rom_endianness(rom);
    log::info!("Detected ROM endian-ness: {endianness:?}");

    // TODO ST010/ST011
    let program_rom = convert_program_rom(&rom[..3 * DSP_PROGRAM_ROM_LEN_OPCODES], endianness);
    let data_rom = convert_data_rom(&rom[3 * DSP_PROGRAM_ROM_LEN_OPCODES..], endianness);
    (program_rom, data_rom)
}

// Convert program ROM from bytes to 24-bit opcodes
fn convert_program_rom(program_rom: &[u8], endianness: Endianness) -> Vec<u32> {
    program_rom.chunks_exact(3).map(|chunk| endianness.chunk_to_u32(chunk)).collect()
}

// Convert data ROM from bytes to 16-bit words
fn convert_data_rom(data_rom: &[u8], endianness: Endianness) -> Vec<u16> {
    data_rom.chunks_exact(2).map(|chunk| endianness.chunk_to_u16(chunk)).collect()
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
