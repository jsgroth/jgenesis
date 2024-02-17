mod dma;

use crate::sa1::mmc::Sa1Mmc;
use crate::sa1::timer::Sa1Timer;
use crate::sa1::Iram;
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, SignBit, U16Ext, U24Ext};
use std::ops::Range;
use std::{array, cmp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum InterruptVectorSource {
    #[default]
    Rom,
    IoPorts,
}

impl InterruptVectorSource {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::IoPorts } else { Self::Rom }
    }

    fn to_bit(self) -> bool {
        self == Self::IoPorts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaSourceDevice {
    #[default]
    Rom,
    Iram,
    Bwram,
}

impl DmaSourceDevice {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::Rom,
            0x01 => Self::Bwram,
            0x02 => Self::Iram,
            0x03 => {
                log::warn!("SA-1 set unsupported DMA source 3; defaulting to ROM");
                Self::Rom
            }
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaDestinationDevice {
    #[default]
    Iram,
    Bwram,
}

impl DmaDestinationDevice {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Bwram } else { Self::Iram }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaType {
    #[default]
    Normal,
    CharacterConversion,
}

impl DmaType {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::CharacterConversion } else { Self::Normal }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaPriority {
    #[default]
    Cpu,
    Dma,
}

impl DmaPriority {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Dma } else { Self::Cpu }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CharacterConversionType {
    One,
    #[default]
    Two,
}

impl CharacterConversionType {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::One } else { Self::Two }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CharacterConversionColorBits {
    Two,
    Four,
    #[default]
    Eight,
}

impl CharacterConversionColorBits {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::Eight,
            0x01 => Self::Four,
            0x02 | 0x03 => Self::Two,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }

    fn bit_mask(self) -> u8 {
        match self {
            Self::Two => 0x03,
            Self::Four => 0x0F,
            Self::Eight => 0xFF,
        }
    }

    fn tile_size(self) -> u32 {
        match self {
            Self::Two => 16,
            Self::Four => 32,
            Self::Eight => 64,
        }
    }

    fn bitplanes(self) -> u32 {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum DmaState {
    Idle,
    NormalCopying,
    NormalWaitCycle,
    CharacterConversion2 { buffer_idx: u8, rows_copied: u8 },
    CharacterConversion1Initial { cycles_remaining: u8 },
    CharacterConversion1Active { buffer_idx: u8, dma_bytes_remaining: u8, next_tile_number: u16 },
}

impl Default for DmaState {
    fn default() -> Self {
        Self::Idle
    }
}

impl DmaState {
    fn is_character_conversion(self) -> bool {
        matches!(
            self,
            Self::CharacterConversion2 { .. }
                | Self::CharacterConversion1Initial { .. }
                | Self::CharacterConversion1Active { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ArithmeticOp {
    #[default]
    Multiply,
    Divide,
    MultiplyAccumulate,
}

impl ArithmeticOp {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::Multiply,
            0x01 => Self::Divide,
            0x02 | 0x03 => Self::MultiplyAccumulate,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sa1Registers {
    // CCNT / SA-1 CPU Control
    pub sa1_irq_from_snes: bool,
    pub sa1_nmi: bool,
    pub sa1_reset: bool,
    pub sa1_wait: bool,
    pub message_to_sa1: u8,
    // SCNT / SNES CPU Control
    pub snes_irq_from_sa1: bool,
    pub snes_irq_vector_source: InterruptVectorSource,
    pub snes_nmi_vector_source: InterruptVectorSource,
    pub message_to_snes: u8,
    // SIE / SNES CPU Interrupt Enable
    pub snes_irq_from_sa1_enabled: bool,
    pub snes_irq_from_dma_enabled: bool,
    // CIE / SA-1 CPU Interrupt Enable
    pub sa1_irq_from_snes_enabled: bool,
    pub timer_irq_enabled: bool,
    pub dma_irq_enabled: bool,
    pub sa1_nmi_enabled: bool,
    // CRV / SA-1 CPU RESET Vector
    pub sa1_reset_vector: u16,
    // CNV / SA-1 CPU NMI Vector
    pub sa1_nmi_vector: u16,
    // CIV / SA-1 CPU IRQ Vector
    pub sa1_irq_vector: u16,
    // SNV / SNES CPU NMI Vector
    pub snes_nmi_vector: u16,
    // SIV / SNES CPU IRQ Vector
    pub snes_irq_vector: u16,
    // SBWE/CBWE / BW-RAM Writes Enabled
    pub bwram_writes_enabled: bool,
    // BWPA / BW-RAM Write Protected Area
    pub bwram_write_protection_size: u32,
    // SIWP / SNES I-RAM Write Protection
    pub snes_iram_writes_enabled: [bool; 8],
    // CIWP / SA-1 I-RAM Write Protection
    pub sa1_iram_writes_enabled: [bool; 8],
    // DCNT / DMA Control
    pub dma_source: DmaSourceDevice,
    pub dma_destination: DmaDestinationDevice,
    pub dma_type: DmaType,
    pub character_conversion_type: CharacterConversionType,
    pub dma_priority: DmaPriority,
    pub dma_enabled: bool,
    // CDMA / Character Conversion DMA Parameters
    pub ccdma_color_depth: CharacterConversionColorBits,
    pub virtual_vram_width_tiles: u8,
    // SDA / DMA Source Device Start Address
    pub dma_source_address: u32,
    // DDA / DMA Destination Device Start Address
    pub dma_destination_address: u32,
    // DTC / DMA Terminal Counter
    pub dma_terminal_counter: u16,
    // BRF / Bitmap Register File
    pub bitmap_pixels: [u8; 16],
    // MCNT / Arithmetic Control
    pub arithmetic_op: ArithmeticOp,
    // MA / Arithmetic Parameter A
    pub arithmetic_param_a: u16,
    // MB / Arithmetic Parameter B
    pub arithmetic_param_b: u16,
    // MR / Arithmetic Result
    pub arithmetic_result: u64,
    // OF / Arithmetic Overflow Flag
    pub arithmetic_overflow: bool,
    // VDA / Variable-Length Bit Data ROM Start Address
    pub varlen_bit_start_address: u32,
    // VDP / Variable-Length Bit Data Read Port
    pub varlen_bit_data: u32,
    pub varlen_bits_remaining: u8,
    // TODO varlen auto-increment? not used by any games
    // Miscellaneous internal state
    pub dma_state: DmaState,
    pub ccdma_transfer_in_progress: bool,
    pub character_conversion_irq: bool,
    pub sa1_dma_irq: bool,
}

impl Sa1Registers {
    pub fn new() -> Self {
        Self {
            sa1_irq_from_snes: false,
            sa1_nmi: false,
            sa1_reset: true,
            sa1_wait: false,
            message_to_sa1: 0,
            snes_irq_from_sa1: false,
            snes_irq_vector_source: InterruptVectorSource::default(),
            snes_nmi_vector_source: InterruptVectorSource::default(),
            message_to_snes: 0,
            snes_irq_from_sa1_enabled: false,
            snes_irq_from_dma_enabled: false,
            sa1_irq_from_snes_enabled: false,
            timer_irq_enabled: false,
            dma_irq_enabled: false,
            sa1_nmi_enabled: false,
            sa1_reset_vector: 0,
            sa1_nmi_vector: 0,
            sa1_irq_vector: 0,
            snes_nmi_vector: 0,
            snes_irq_vector: 0,
            bwram_writes_enabled: false,
            bwram_write_protection_size: 1 << 23,
            snes_iram_writes_enabled: [false; 8],
            sa1_iram_writes_enabled: [false; 8],
            dma_source: DmaSourceDevice::default(),
            dma_destination: DmaDestinationDevice::default(),
            dma_type: DmaType::default(),
            character_conversion_type: CharacterConversionType::default(),
            dma_priority: DmaPriority::default(),
            dma_enabled: false,
            ccdma_color_depth: CharacterConversionColorBits::default(),
            virtual_vram_width_tiles: 1,
            dma_source_address: 0,
            dma_destination_address: 0,
            dma_terminal_counter: 0,
            bitmap_pixels: [0; 16],
            arithmetic_op: ArithmeticOp::default(),
            arithmetic_param_a: 0,
            arithmetic_param_b: 0,
            arithmetic_result: 0,
            arithmetic_overflow: false,
            varlen_bit_start_address: 0,
            varlen_bit_data: 0,
            varlen_bits_remaining: 0,
            dma_state: DmaState::default(),
            ccdma_transfer_in_progress: false,
            character_conversion_irq: false,
            sa1_dma_irq: false,
        }
    }

    pub fn snes_read(&self, address: u32) -> Option<u8> {
        // Only $2300 (SFR) is readable by SNES CPU
        // $230E is supposed to be a version code register, but hardware tests discovered that it
        // is actually open bus
        (address & 0xFFFF == 0x2300).then(|| self.read_sfr())
    }

    pub fn sa1_read(&self, address: u32, timer: &mut Sa1Timer) -> u8 {
        log::trace!("SA-1 register read: {:04X}", address & 0xFFFF);

        match address & 0xFFFF {
            0x2301 => self.read_cfr(timer),
            0x2302 => timer.read_hcr_low(),
            0x2303 => timer.read_hcr_high(),
            0x2304 => timer.read_vcr_low(),
            0x2305 => timer.read_vcr_high(),
            0x2306..=0x230A => self.read_mr(address),
            0x230B => self.read_of(),
            0x230C => self.read_vdp_low(),
            0x230D => self.read_vdp_high(),
            _ => 0,
        }
    }

    pub fn snes_write(&mut self, address: u32, value: u8, mmc: &mut Sa1Mmc) {
        log::trace!("SNES register write: {address:06X} {value:02X}");

        match address & 0xFFFF {
            0x2200 => self.write_ccnt(value),
            0x2201 => self.write_sie(value),
            0x2202 => self.write_sic(value),
            0x2203 => self.write_crv_low(value),
            0x2204 => self.write_crv_high(value),
            0x2205 => self.write_cnv_low(value),
            0x2206 => self.write_cnv_high(value),
            0x2207 => self.write_civ_low(value),
            0x2208 => self.write_civ_high(value),
            0x2220 => mmc.write_cxb(value),
            0x2221 => mmc.write_dxb(value),
            0x2222 => mmc.write_exb(value),
            0x2223 => mmc.write_fxb(value),
            0x2224 => mmc.write_bmaps(value),
            0x2226 => self.write_sbwe(value),
            0x2228 => self.write_bwpa(value),
            0x2229 => self.write_siwp(value),
            0x2231 => self.write_cdma(value),
            0x2232 => self.write_sda_low(value),
            0x2233 => self.write_sda_mid(value),
            0x2234 => self.write_sda_high(value),
            0x2235 => self.write_dda_low(value),
            0x2236 => self.write_dda_mid(value),
            0x2237 => self.write_dda_high(value),
            _ => {}
        }
    }

    pub fn sa1_write(
        &mut self,
        address: u32,
        value: u8,
        timer: &mut Sa1Timer,
        mmc: &mut Sa1Mmc,
        rom: &[u8],
        iram: &mut Iram,
    ) {
        log::trace!("SA-1 register write: {address:06X} {value:02X}");

        match address & 0xFFFF {
            0x2209 => self.write_scnt(value),
            0x220A => self.write_cie(value),
            0x220B => self.write_cic(value, timer),
            0x220C => self.write_snv_low(value),
            0x220D => self.write_snv_high(value),
            0x220E => self.write_siv_low(value),
            0x220F => self.write_siv_high(value),
            0x2210 => timer.write_tmc(value),
            0x2211 => timer.reset(),
            0x2212 => timer.write_hcnt_low(value),
            0x2213 => timer.write_hcnt_high(value),
            0x2214 => timer.write_vcnt_low(value),
            0x2215 => timer.write_vcnt_high(value),
            0x2225 => mmc.write_bmap(value),
            0x2227 => self.write_cbwe(value),
            0x222A => self.write_ciwp(value),
            0x2230 => self.write_dcnt(value),
            0x2231 => self.write_cdma(value),
            0x2232 => self.write_sda_low(value),
            0x2233 => self.write_sda_mid(value),
            0x2234 => self.write_sda_high(value),
            0x2235 => self.write_dda_low(value),
            0x2236 => self.write_dda_mid(value),
            0x2237 => self.write_dda_high(value),
            0x2238 => self.write_dtc_low(value),
            0x2239 => self.write_dtc_high(value),
            0x223F => mmc.write_bbf(value),
            0x2240..=0x224F => self.write_brf(address, value, iram),
            0x2250 => self.write_mcnt(value),
            0x2251 => self.write_ma_low(value),
            0x2252 => self.write_ma_high(value),
            0x2253 => self.write_mb_low(value),
            0x2254 => self.write_mb_high(value),
            0x2258 => self.write_vbd(value, mmc, rom),
            0x2259 => self.write_vda_low(value),
            0x225A => self.write_vda_mid(value),
            0x225B => self.write_vda_high(value, mmc, rom),
            _ => {}
        }
    }

    fn read_sfr(&self) -> u8 {
        (u8::from(self.snes_irq_from_sa1) << 7)
            | (u8::from(self.snes_irq_vector_source.to_bit()) << 6)
            | (u8::from(self.character_conversion_irq) << 5)
            | (u8::from(self.snes_nmi_vector_source.to_bit()) << 4)
            | self.message_to_snes
    }

    fn read_cfr(&self, timer: &Sa1Timer) -> u8 {
        (u8::from(self.sa1_irq_from_snes) << 7)
            | (u8::from(timer.irq_pending) << 6)
            | (u8::from(self.sa1_dma_irq) << 5)
            | (u8::from(self.sa1_nmi) << 4)
            | self.message_to_sa1
    }

    fn read_mr(&self, address: u32) -> u8 {
        // $2306 is bits 0-7, $2307 is bits 8-15, $2308 is bits 16-23, etc.
        let shift = 8 * ((address & 0xF) - 0x6);
        (self.arithmetic_result >> shift) as u8
    }

    fn read_of(&self) -> u8 {
        u8::from(self.arithmetic_overflow) << 7
    }

    fn read_vdp_low(&self) -> u8 {
        (self.varlen_bit_data as u16).lsb()
    }

    fn read_vdp_high(&self) -> u8 {
        (self.varlen_bit_data as u16).msb()
    }

    fn write_ccnt(&mut self, value: u8) {
        if value.bit(7) {
            self.sa1_irq_from_snes = true;
            log::trace!("  Generating SA-1 IRQ from SNES");
        }

        self.sa1_wait = value.bit(6);
        self.sa1_reset = value.bit(5);

        if value.bit(4) {
            self.sa1_nmi = true;
            log::trace!("  Generating SA-1 NMI");
        }

        self.message_to_sa1 = value & 0x0F;

        log::trace!("  SA-1 wait: {}", self.sa1_wait);
        log::trace!("  SA-1 reset: {}", self.sa1_reset);
        log::trace!("  Message to SA-1: {:X}", self.message_to_sa1);
    }

    fn write_sie(&mut self, value: u8) {
        self.snes_irq_from_sa1_enabled = value.bit(7);
        self.snes_irq_from_dma_enabled = value.bit(5);

        log::trace!("  SNES IRQs from SA-1 enabled: {}", self.snes_irq_from_sa1_enabled);
        log::trace!(
            "  SNES character conversion DMA IRQs enabled: {}",
            self.snes_irq_from_dma_enabled
        );
    }

    fn write_sic(&mut self, value: u8) {
        if value.bit(7) {
            self.snes_irq_from_sa1 = false;
            log::trace!("  SNES IRQ from SA-1 cleared");
        }
        if value.bit(5) {
            self.character_conversion_irq = false;
            log::trace!("  SNES character conversion DMA IRQ cleared");
        }
    }

    fn write_cie(&mut self, value: u8) {
        self.sa1_irq_from_snes_enabled = value.bit(7);
        self.timer_irq_enabled = value.bit(6);
        self.dma_irq_enabled = value.bit(5);
        self.sa1_nmi_enabled = value.bit(4);

        log::trace!("  SA-1 IRQs from SNES enabled: {}", self.sa1_irq_from_snes_enabled);
        log::trace!("  SA-1 timer IRQs enabled: {}", self.timer_irq_enabled);
        log::trace!("  SA-1 DMA IRQs enabled: {}", self.dma_irq_enabled);
        log::trace!("  SA-1 NMIs enabled: {}", self.sa1_nmi_enabled);
    }

    fn write_cic(&mut self, value: u8, timer: &mut Sa1Timer) {
        if value.bit(7) {
            self.sa1_irq_from_snes = false;
            log::trace!("  Cleared SA-1 IRQ from SNES");
        }

        if value.bit(6) {
            timer.irq_pending = false;
            log::trace!("  Cleared SA-1 timer IRQ");
        }

        if value.bit(5) {
            self.sa1_dma_irq = false;
            log::trace!("  Cleared SA-1 DMA IRQ");
        }

        if value.bit(4) {
            self.sa1_nmi = false;
            log::trace!("  Cleared SA-1 NMI");
        }
    }

    fn write_crv_low(&mut self, value: u8) {
        self.sa1_reset_vector.set_lsb(value);

        log::trace!("  SA-1 RESET vector: {:04X}", self.sa1_reset_vector);
    }

    fn write_crv_high(&mut self, value: u8) {
        self.sa1_reset_vector.set_msb(value);

        log::trace!("  SA-1 RESET vector: {:04X}", self.sa1_reset_vector);
    }

    fn write_cnv_low(&mut self, value: u8) {
        self.sa1_nmi_vector.set_lsb(value);

        log::trace!("  SA-1 NMI vector: {:04X}", self.sa1_nmi_vector);
    }

    fn write_cnv_high(&mut self, value: u8) {
        self.sa1_nmi_vector.set_msb(value);

        log::trace!("  SA-1 NMI vector: {:04X}", self.sa1_nmi_vector);
    }

    fn write_civ_low(&mut self, value: u8) {
        self.sa1_irq_vector.set_lsb(value);

        log::trace!("  SA-1 IRQ vector: {:04X}", self.sa1_irq_vector);
    }

    fn write_civ_high(&mut self, value: u8) {
        self.sa1_irq_vector.set_msb(value);

        log::trace!("  SA-1 IRQ vector: {:04X}", self.sa1_irq_vector);
    }

    fn write_snv_low(&mut self, value: u8) {
        self.snes_nmi_vector.set_lsb(value);

        log::trace!("  SNES NMI vector: {:04X}", self.snes_nmi_vector);
    }

    fn write_snv_high(&mut self, value: u8) {
        self.snes_nmi_vector.set_msb(value);

        log::trace!("  SNES NMI vector: {:04X}", self.snes_nmi_vector);
    }

    fn write_siv_low(&mut self, value: u8) {
        self.snes_irq_vector.set_lsb(value);

        log::trace!("  SNES IRQ vector: {:04X}", self.snes_irq_vector);
    }

    fn write_siv_high(&mut self, value: u8) {
        self.snes_irq_vector.set_msb(value);

        log::trace!("  SNES IRQ vector: {:04X}", self.snes_irq_vector);
    }

    fn write_scnt(&mut self, value: u8) {
        if value.bit(7) {
            self.snes_irq_from_sa1 = true;
            log::trace!("  Generated SNES IRQ from SA-1");
        }

        self.snes_irq_vector_source = InterruptVectorSource::from_bit(value.bit(6));
        self.snes_nmi_vector_source = InterruptVectorSource::from_bit(value.bit(4));
        self.message_to_snes = value & 0x0F;

        log::trace!("  SNES IRQ vector source: {:?}", self.snes_irq_vector_source);
        log::trace!("  SNES NMI vector source: {:?}", self.snes_nmi_vector_source);
        log::trace!("  Message to SNES: {:X}", self.message_to_snes);
    }

    fn write_sbwe(&mut self, value: u8) {
        self.bwram_writes_enabled = value.bit(7);

        log::trace!("  BW-RAM writes enabled: {}", self.bwram_writes_enabled);
    }

    fn write_cbwe(&mut self, value: u8) {
        self.bwram_writes_enabled = value.bit(7);

        log::trace!("  BW-RAM writes enabled: {}", self.bwram_writes_enabled);
    }

    fn write_bwpa(&mut self, value: u8) {
        // Write protected area size is 256 * 2^N bytes
        self.bwram_write_protection_size = 1 << (8 + (value & 0x0F));

        log::trace!("  BW-RAM write protection size: {:X}", self.bwram_write_protection_size);
    }

    fn write_siwp(&mut self, value: u8) {
        self.snes_iram_writes_enabled = array::from_fn(|i| value.bit(i as u8));

        log::trace!("  SNES I-RAM writes enabled: {value:02X}");
    }

    fn write_ciwp(&mut self, value: u8) {
        self.sa1_iram_writes_enabled = array::from_fn(|i| value.bit(i as u8));

        log::trace!("  SA-1 I-RAM writes enabled: {value:02X}");
    }

    fn write_dcnt(&mut self, value: u8) {
        self.dma_source = DmaSourceDevice::from_byte(value);
        self.dma_destination = DmaDestinationDevice::from_bit(value.bit(2));
        self.character_conversion_type = CharacterConversionType::from_bit(value.bit(4));
        self.dma_type = DmaType::from_bit(value.bit(5));
        self.dma_priority = DmaPriority::from_bit(value.bit(6));
        self.dma_enabled = value.bit(7);

        log::trace!("  DMA source: {:?}", self.dma_source);
        log::trace!("  DMA destination: {:?}", self.dma_destination);
        log::trace!("  DMA character conversion type: {:?}", self.character_conversion_type);
        log::trace!("  DMA type: {:?}", self.dma_type);
        log::trace!("  DMA enabled: {}", self.dma_enabled);
    }

    fn write_cdma(&mut self, value: u8) {
        self.ccdma_color_depth = CharacterConversionColorBits::from_byte(value);
        self.virtual_vram_width_tiles = cmp::min(32, 1 << ((value >> 2) & 0x07));

        if value.bit(7) && self.dma_state.is_character_conversion() {
            log::trace!("  Terminating character conversion DMA");
            self.dma_state = DmaState::Idle;
        }

        log::trace!("  Character conversion DMA color depth bits: {:?}", self.ccdma_color_depth);
        log::trace!("  Virtual VRAM width in tiles: {}", self.virtual_vram_width_tiles);
    }

    fn write_sda_low(&mut self, value: u8) {
        self.dma_source_address.set_low_byte(value);

        log::trace!("  DMA source address: {:06X}", self.dma_source_address);
    }

    fn write_sda_mid(&mut self, value: u8) {
        self.dma_source_address.set_mid_byte(value);

        log::trace!("  DMA source address: {:06X}", self.dma_source_address);
    }

    fn write_sda_high(&mut self, value: u8) {
        self.dma_source_address.set_high_byte(value);

        log::trace!("  DMA source address: {:06X}", self.dma_source_address);
    }

    fn write_dda_low(&mut self, value: u8) {
        self.dma_destination_address.set_low_byte(value);

        log::trace!("  DMA destination address: {:06X}", self.dma_destination_address);
    }

    fn write_dda_mid(&mut self, value: u8) {
        self.dma_destination_address.set_mid_byte(value);

        log::trace!("  DMA destination address: {:06X}", self.dma_destination_address);

        match (self.dma_enabled, self.dma_type, self.dma_destination) {
            (true, DmaType::Normal, DmaDestinationDevice::Iram) => {
                log::trace!("  Starting SA-1 DMA to I-RAM");
                self.dma_state = DmaState::NormalCopying;
            }
            (true, DmaType::CharacterConversion, _) => {
                log::trace!("  Starting character conversion DMA");
                self.dma_state = match self.character_conversion_type {
                    CharacterConversionType::Two => {
                        DmaState::CharacterConversion2 { buffer_idx: 0, rows_copied: 0 }
                    }
                    CharacterConversionType::One => DmaState::CharacterConversion1Initial {
                        cycles_remaining: self.ccdma_color_depth.tile_size() as u8,
                    },
                };
            }
            _ => {}
        }
    }

    fn write_dda_high(&mut self, value: u8) {
        self.dma_destination_address.set_high_byte(value);

        log::trace!("  DMA destination address: {:06X}", self.dma_destination_address);

        if self.dma_enabled
            && self.dma_type == DmaType::Normal
            && self.dma_destination == DmaDestinationDevice::Bwram
        {
            log::trace!("  Starting SA-1 DMA to BW-RAM");
            self.dma_state = DmaState::NormalCopying;
        }
    }

    fn write_dtc_low(&mut self, value: u8) {
        self.dma_terminal_counter.set_lsb(value);

        log::trace!("  DMA terminal counter: {:04X}", self.dma_terminal_counter);
    }

    fn write_dtc_high(&mut self, value: u8) {
        self.dma_terminal_counter.set_msb(value);

        log::trace!("  DMA terminal counter: {:04X}", self.dma_terminal_counter);
    }

    fn write_brf(&mut self, address: u32, value: u8, iram: &mut Iram) {
        // BRF registers are $2240-$224F; lowest 4 bits of address are the register index
        let idx = (address & 0xF) as usize;
        self.bitmap_pixels[idx] = value & self.ccdma_color_depth.bit_mask();

        log::trace!("  Bitmap register file #{idx}: {value:02X}");

        if idx & 0x7 == 0x7 {
            // Perform character conversion any time register 7 or 15 is written
            if let DmaState::CharacterConversion2 { buffer_idx, rows_copied } = self.dma_state {
                self.character_conversion_2(idx & 0x8, buffer_idx, rows_copied, iram);
            }
        }
    }

    fn write_mcnt(&mut self, value: u8) {
        self.arithmetic_op = ArithmeticOp::from_byte(value);

        // Setting multiply-accumulate clears result
        if self.arithmetic_op == ArithmeticOp::MultiplyAccumulate {
            self.arithmetic_result = 0;
            self.arithmetic_overflow = false;
        }

        log::trace!("  Arithmetic mode: {:?}", self.arithmetic_op);
    }

    fn write_ma_low(&mut self, value: u8) {
        self.arithmetic_param_a.set_lsb(value);

        log::trace!("  Arithmetic parameter A: {:04X}", self.arithmetic_param_a);
    }

    fn write_ma_high(&mut self, value: u8) {
        self.arithmetic_param_a.set_msb(value);

        log::trace!("  Arithmetic parameter A: {:04X}", self.arithmetic_param_a);
    }

    fn write_mb_low(&mut self, value: u8) {
        self.arithmetic_param_b.set_lsb(value);

        log::trace!("  Arithmetic parameter B: {:04X}", self.arithmetic_param_b);
    }

    fn write_mb_high(&mut self, value: u8) {
        self.arithmetic_param_b.set_msb(value);

        // Writing MB high byte begins arithmetic operation
        self.perform_arithmetic_op();

        log::trace!("  Arithmetic parameter B: {:04X}", self.arithmetic_param_b);
    }

    fn write_vbd(&mut self, value: u8, mmc: &Sa1Mmc, rom: &[u8]) {
        if self.varlen_bits_remaining == 0 {
            // Variable-length bit data reading not initialized; do nothing
            return;
        }

        let shift = if value & 0x0F == 0 { 16 } else { value & 0x0F };
        self.varlen_bit_data >>= shift;
        self.varlen_bits_remaining -= shift;

        if self.varlen_bits_remaining < 16 {
            // Read next word
            let word = mmc.map_rom_address(self.varlen_bit_start_address).map_or(0, |rom_addr| {
                let lsb = rom.get(rom_addr as usize).copied().unwrap_or(0);
                let msb = rom.get((rom_addr + 1) as usize).copied().unwrap_or(0);
                u16::from_le_bytes([lsb, msb])
            });
            let word: u32 = word.into();

            self.varlen_bit_data |= word << self.varlen_bits_remaining;
            self.varlen_bit_start_address = (self.varlen_bit_start_address + 2) & 0xFFFFFF;
            self.varlen_bits_remaining += 16;
        }

        log::trace!("  Variable-length bit data shift: {shift}");
    }

    fn write_vda_low(&mut self, value: u8) {
        self.varlen_bit_start_address.set_low_byte(value);

        log::trace!(
            "  Variable-length bit data ROM start address: {:06X}",
            self.varlen_bit_start_address
        );
    }

    fn write_vda_mid(&mut self, value: u8) {
        self.varlen_bit_start_address.set_mid_byte(value);

        log::trace!(
            "  Variable-length bit data ROM start address: {:06X}",
            self.varlen_bit_start_address
        );
    }

    fn write_vda_high(&mut self, value: u8, mmc: &Sa1Mmc, rom: &[u8]) {
        self.varlen_bit_start_address.set_high_byte(value);

        // Writing MSB starts the variable-length bit data read
        if let Some(rom_addr) = mmc.map_rom_address(self.varlen_bit_start_address) {
            let lsb = rom[rom_addr as usize];
            let msb = rom.get((rom_addr + 1) as usize).copied().unwrap_or(0);
            self.varlen_bit_data = u16::from_le_bytes([lsb, msb]).into();
            self.varlen_bit_start_address = (self.varlen_bit_start_address + 2) & 0xFFFFFF;
            self.varlen_bits_remaining = 16;
        }

        log::trace!(
            "  Variable-length bit data ROM start address: {:06X}",
            self.varlen_bit_start_address
        );
    }

    pub fn can_write_bwram(&self, bwram_addr: u32) -> bool {
        self.bwram_writes_enabled || bwram_addr >= self.bwram_write_protection_size
    }

    pub fn tick_dma(&mut self, mmc: &Sa1Mmc, rom: &[u8], iram: &mut Iram, bwram: &mut [u8]) {
        // Progress normal DMA or character conversion DMA type 1
        match self.dma_state {
            DmaState::NormalCopying => {
                self.progress_normal_dma(mmc, rom, iram, bwram);
            }
            DmaState::NormalWaitCycle => {
                self.dma_state = DmaState::NormalCopying;
            }
            DmaState::CharacterConversion1Initial { cycles_remaining } => {
                if cycles_remaining == 1 {
                    self.start_ccdma_type_1(iram, bwram);
                } else {
                    self.dma_state = DmaState::CharacterConversion1Initial {
                        cycles_remaining: cycles_remaining - 1,
                    };
                }
            }
            DmaState::Idle
            | DmaState::CharacterConversion2 { .. }
            | DmaState::CharacterConversion1Active { .. } => {}
        }
    }

    pub fn notify_snes_dma_start(&mut self, source_address: u32) {
        // TODO check exact source address?
        if matches!(self.dma_state, DmaState::CharacterConversion1Active { .. })
            && (0x400000..0x500000).contains(&source_address)
        {
            self.ccdma_transfer_in_progress = true;
        }
    }

    pub fn notify_snes_dma_end(&mut self) {
        self.ccdma_transfer_in_progress = false;
    }

    fn perform_arithmetic_op(&mut self) {
        const I40_RANGE: Range<i64> = -(1 << 39)..1 << 39;
        const I40_MASK: u64 = (1 << 40) - 1;

        match self.arithmetic_op {
            ArithmeticOp::Multiply => {
                // Signed 16-bit x Signed 16-bit -> Signed 32-bit
                self.arithmetic_result =
                    (multiply(self.arithmetic_param_a, self.arithmetic_param_b) as u64) & I40_MASK;
            }
            ArithmeticOp::Divide => {
                // Signed 16-bit / Unsigned 16-bit -> Signed 16-bit Quotient, Unsigned 16-bit Remainder
                let (quotient, remainder) =
                    divide(self.arithmetic_param_a, self.arithmetic_param_b);
                let quotient: u64 = (quotient as u16).into();
                let remainder: u64 = remainder.into();

                self.arithmetic_result = quotient | (remainder << 16);

                // Division apparently clears parameter A in addition to B
                self.arithmetic_param_a = 0;
            }
            ArithmeticOp::MultiplyAccumulate => {
                // Signed 16-bit x Signed 16-bit -> Signed 32-bit
                // Accumulates into a signed 40-bit sum
                let product = multiply(self.arithmetic_param_a, self.arithmetic_param_b);
                let sum = (((self.arithmetic_result as i64) << 24) >> 24) + product;

                self.arithmetic_result = (sum as u64) & I40_MASK;
                self.arithmetic_overflow = !I40_RANGE.contains(&sum);
            }
        }

        // All ops apparently clear parameter B
        self.arithmetic_param_b = 0;
    }

    pub fn reset(&mut self, timer: &mut Sa1Timer, mmc: &mut Sa1Mmc) {
        self.write_ccnt(0x20);
        self.write_sie(0x00);
        self.write_sic(0x00);
        self.write_scnt(0x00);
        self.write_cie(0x00);
        self.write_cic(0x00, timer);
        timer.write_tmc(0x00);
        self.write_sbwe(0x00);
        self.write_cbwe(0x00);
        self.write_bwpa(0xFF);
        self.write_siwp(0x00);
        self.write_ciwp(0x00);
        self.write_dcnt(0x00);
        self.write_cdma(0x80);
        self.write_mcnt(0x00);

        mmc.write_cxb(0x00);
        mmc.write_dxb(0x01);
        mmc.write_exb(0x02);
        mmc.write_fxb(0x03);
        mmc.write_bmaps(0x00);
        mmc.write_bmap(0x00);
        mmc.write_bbf(0x00);
    }

    pub fn cpu_halted(&self) -> bool {
        matches!(self.dma_state, DmaState::NormalCopying | DmaState::NormalWaitCycle)
            && (self.dma_priority == DmaPriority::Dma || self.dma_source == DmaSourceDevice::Rom)
    }
}

fn multiply(a: u16, b: u16) -> i64 {
    let a: i64 = (a as i16).into();
    let b: i64 = (b as i16).into();
    a * b
}

fn divide(a: u16, b: u16) -> (i16, u16) {
    if b == 0 {
        // Divide by zero
        return if a.sign_bit() { (1, (!a).wrapping_add(1)) } else { (-1, a) };
    }

    // Signed dividend, unsigned divisor
    let a: i32 = (a as i16).into();
    let b: i32 = b.into();

    // Signed quotient, unsigned remainder
    let quotient = a.div_euclid(b);
    let remainder = a.rem_euclid(b);

    (quotient as i16, remainder as u16)
}
