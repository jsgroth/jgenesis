//! GBA DMA transfer state
//!
//! The actual bus reads/writes are performed in [`crate::bus::Bus::try_progress_dma`]

use crate::cartridge::Cartridge;
use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::array;

const INITIAL_START_LATENCY: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum AddressIncrement {
    #[default]
    Increment = 0,
    Decrement = 1,
    Fixed = 2,
    IncrementReload = 3,
}

impl AddressIncrement {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Increment,
            1 => Self::Decrement,
            2 => Self::Fixed,
            3 => Self::IncrementReload,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn apply(self, address: u32, increment: u32) -> u32 {
        match self {
            Self::Increment | Self::IncrementReload => address.wrapping_add(increment),
            Self::Decrement => address.wrapping_sub(increment),
            Self::Fixed => address,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum TransferUnit {
    #[default]
    Halfword = 0,
    Word = 1,
}

impl TransferUnit {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Word } else { Self::Halfword }
    }

    fn address_increment(self) -> u32 {
        match self {
            Self::Halfword => 2,
            Self::Word => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum StartTiming {
    #[default]
    Immediate = 0,
    VBlank = 1,
    HBlank = 2,
    Special = 3,
}

impl StartTiming {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Immediate,
            1 => Self::VBlank,
            2 => Self::HBlank,
            3 => Self::Special,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct DmaChannel {
    idx: u8,
    last_read: u32,
    source_address: u32,
    source_addr_mask: u32,
    destination_address: u32,
    dest_addr_mask: u32,
    length: u16,
    length_mask: u16,
    // Control register fields
    source_increment: AddressIncrement,
    destination_increment: AddressIncrement,
    repeat: bool,
    unit: TransferUnit,
    start_timing: StartTiming,
    irq_enabled: bool,
    dma_enabled: bool,
    game_pak_drq_enabled: bool,
    // Internal latches used for in-progress DMA
    latched_source_address: u32,
    latched_destination_address: u32,
    latched_length: u16,
    dma_active: bool,
    start_latency: u64,
}

impl DmaChannel {
    fn new(idx: u8) -> Self {
        let length_mask = if idx != 3 {
            // DMA0-2 have 14-bit length
            0x3FFF
        } else {
            // DMA3 has 16-bit length
            0xFFFF
        };

        let (source_addr_mask, dest_addr_mask) = match idx {
            0 => {
                // DMA0 can only access $0000000-$7FFFFFF (can't access cartridge)
                (0x7FFFFFF, 0x7FFFFFF)
            }
            1 | 2 => {
                // DMA1-2 can read from $0000000-$FFFFFFF and write to $0000000-$7FFFFFF
                // (can't write to cartridge)
                (0xFFFFFFF, 0x7FFFFFF)
            }
            3 => {
                // DMA3 can access all valid addresses (except BIOS ROM)
                (0xFFFFFFF, 0xFFFFFFF)
            }
            _ => panic!("invalid DMA channel {idx}, must be 0-3"),
        };

        Self {
            idx,
            last_read: 0,
            source_address: 0,
            source_addr_mask,
            destination_address: 0,
            dest_addr_mask,
            length: 0,
            length_mask,
            source_increment: AddressIncrement::default(),
            destination_increment: AddressIncrement::default(),
            repeat: false,
            unit: TransferUnit::default(),
            start_timing: StartTiming::default(),
            irq_enabled: false,
            dma_enabled: false,
            game_pak_drq_enabled: false,
            latched_source_address: 0,
            latched_destination_address: 0,
            latched_length: 0,
            dma_active: false,
            start_latency: 0,
        }
    }

    // $40000B0: DMA0SAD_L (DMA0 source address low)
    // $40000BC: DMA1SAD_L
    // $40000C8: DMA2SAD_L
    // $40000D4: DMA3SAD_L
    fn write_source_low(&mut self, value: u16) {
        self.source_address = (self.source_address & !0xFFFF) | u32::from(value);

        log::trace!("DMA{}SAD_L write: {value:04X}", self.idx);
        log::trace!("  Source address: {:08X}", self.source_address);
    }

    // $40000B2: DMA0SAD_H (DMA0 source address high)
    // $40000BE: DMA1SAD_H
    // $40000CA: DMA2SAD_H
    // $40000D6: DMA3SAD_H
    fn write_source_high(&mut self, value: u16) {
        // Highest 4 bits are ignored
        self.source_address = (self.source_address & 0xFFFF) | (u32::from(value & 0x0FFF) << 16);

        log::trace!("DMA{}SAD_H write: {value:04X}", self.idx);
        log::trace!("  Source address: {:08X}", self.source_address);
    }

    // $40000B4: DMA0DAD_L (DMA0 destination address low)
    // $40000C0: DMA1DAD_L
    // $40000CC: DMA2DAD_L
    // $40000D8: DMA3DAD_L
    fn write_destination_low(&mut self, value: u16) {
        self.destination_address = (self.destination_address & !0xFFFF) | u32::from(value);

        log::trace!("DMA{}DAD_L write: {value:04X}", self.idx);
        log::trace!("  Destination address: {:08X}", self.destination_address);
    }

    // $40000B6: DMA0DAD_H (DMA0 destination address high)
    // $40000C2: DMA1DAD_H
    // $40000CE: DMA2DAD_H
    // $40000DA: DMA3DAD_H
    fn write_destination_high(&mut self, value: u16) {
        // Highest 4 bits are ignored
        self.destination_address =
            (self.destination_address & 0xFFFF) | (u32::from(value & 0x0FFF) << 16);

        log::trace!("DMA{}DAD_H write: {value:04X}", self.idx);
        log::trace!("  Destination address: {:08X}", self.destination_address);
    }

    // $40000B8: DMA0CNT_L (DMA0 control low / length)
    // $40000C4: DMA1CNT_L
    // $40000D0: DMA2CNT_L
    // $40000DC: DMA3CNT_L
    fn write_length(&mut self, value: u16) {
        self.length = value & self.length_mask;

        log::trace!("DMA{}CNT_L write: {value:04X}", self.idx);
        log::trace!("  Length: {:04X}", self.length);
    }

    // $40000BA: DMA0CNT_H (DMA0 control high)
    // $40000C6: DMA1CNT_H
    // $40000D2: DMA2CNT_H
    // $40000DE: DMA3CNT_H
    fn write_control(&mut self, value: u16) {
        self.destination_increment = AddressIncrement::from_bits(value >> 5);
        self.source_increment = AddressIncrement::from_bits(value >> 7);
        self.repeat = value.bit(9);
        self.unit = TransferUnit::from_bit(value.bit(10));
        self.game_pak_drq_enabled = self.idx == 3 && value.bit(11);
        self.start_timing = StartTiming::from_bits(value >> 12);
        self.irq_enabled = value.bit(14);

        let prev_dma_enabled = self.dma_enabled;
        self.dma_enabled = value.bit(15);

        if !prev_dma_enabled && self.dma_enabled {
            log::trace!("DMA{} newly enabled", self.idx);

            self.latched_source_address = self.source_address;
            self.latched_destination_address = self.destination_address;
            self.latched_length = self.effective_length();

            if self.start_timing == StartTiming::Immediate {
                self.dma_active = true;
                self.start_latency = INITIAL_START_LATENCY;
            }
        }

        log::trace!("DMA{}CNT_H write: {value:04X}", self.idx);
        log::trace!("  Destination increment: {:?}", self.destination_increment);
        log::trace!("  Source increment: {:?}", self.source_increment);
        log::trace!("  Repeat: {}", self.repeat);
        log::trace!("  Transfer unit: {:?}", self.unit);
        log::trace!("  Start timing: {:?}", self.start_timing);
        log::trace!("  IRQ enabled: {}", self.irq_enabled);
        log::trace!("  DMA enabled: {}", self.dma_enabled);
        log::trace!("  Game Pak DRQ enabled: {}", self.game_pak_drq_enabled);
    }

    // $40000BA: DMA0CNT_H (DMA0 control high)
    // $40000C6: DMA1CNT_H
    // $40000D2: DMA2CNT_H
    // $40000DE: DMA3CNT_H
    fn read_control(&self) -> u16 {
        ((self.destination_increment as u16) << 5)
            | ((self.source_increment as u16) << 7)
            | (u16::from(self.repeat) << 9)
            | ((self.unit as u16) << 10)
            | (u16::from(self.game_pak_drq_enabled) << 11)
            | ((self.start_timing as u16) << 12)
            | (u16::from(self.irq_enabled) << 14)
            | (u16::from(self.dma_enabled) << 15)
    }

    fn effective_length(&self) -> u16 {
        // Audio FIFO DMA is always 4 words
        match self.start_timing {
            StartTiming::Special if self.idx != 3 => 4,
            _ => self.length,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TransferSource {
    Memory { address: u32 },
    Value(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct DmaTransfer {
    pub channel: u8,
    pub source: TransferSource,
    pub destination: u32,
    pub unit: TransferUnit,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaState {
    channels: [DmaChannel; 4],
    cycles: u64,
    any_active: bool,
    any_start_latency: bool,
}

impl DmaState {
    pub fn new() -> Self {
        Self {
            channels: array::from_fn(|ch| DmaChannel::new(ch as u8)),
            cycles: 0,
            any_active: false,
            any_start_latency: false,
        }
    }

    pub fn sync(&mut self, cycles: u64) {
        if !self.any_start_latency {
            self.cycles = cycles;
            return;
        }

        let elapsed = cycles.saturating_sub(self.cycles);
        self.cycles = cycles;

        for channel in &mut self.channels {
            channel.start_latency = channel.start_latency.saturating_sub(elapsed);
        }

        self.any_start_latency = self.channels.iter().any(|channel| channel.start_latency != 0);
    }

    pub fn next_transfer(
        &mut self,
        interrupts: &mut InterruptRegisters,
        cycles: u64,
    ) -> Option<DmaTransfer> {
        if !self.any_active {
            return None;
        }

        for (i, channel) in self.channels.iter_mut().enumerate() {
            if !channel.dma_active || channel.start_latency != 0 {
                continue;
            }

            // Audio FIFO DMA is always word-size
            let audio_dma = channel.idx != 3 && channel.start_timing == StartTiming::Special;
            let unit = if audio_dma { TransferUnit::Word } else { channel.unit };
            let increment = unit.address_increment();

            let source_address = channel.latched_source_address & channel.source_addr_mask;
            let source = if source_address >= 0x2000000 {
                channel.latched_source_address =
                    channel.source_increment.apply(source_address, increment);
                TransferSource::Memory { address: source_address }
            } else {
                // When DMA reads an invalid address, source address does not increment and the
                // channel returns the last value that it read from a valid address
                TransferSource::Value(channel.last_read)
            };

            let destination = channel.latched_destination_address & channel.dest_addr_mask;
            if !audio_dma && destination >= 0x2000000 {
                // Destination address does not increment for audio FIFO DMA or invalid address writes
                channel.latched_destination_address =
                    channel.destination_increment.apply(destination, increment);
            }

            channel.latched_length = channel.latched_length.wrapping_sub(1) & channel.length_mask;
            if channel.latched_length == 0 {
                // Ignore repeat bit for immediate start timing
                // Several games depend on this (e.g. NFL Blitz 2002, Kong: The Animated Series)
                channel.dma_enabled =
                    channel.repeat && channel.start_timing != StartTiming::Immediate;
                channel.dma_active = false;
                channel.latched_length = channel.effective_length();

                if channel.destination_increment == AddressIncrement::IncrementReload {
                    channel.latched_destination_address = channel.destination_address;
                }

                if channel.irq_enabled {
                    interrupts.set_flag(InterruptType::DMA[i], cycles);
                }
            }

            let channel_idx = channel.idx;

            self.any_active = self.channels.iter().any(|channel| channel.dma_active);

            return Some(DmaTransfer { channel: channel_idx, source, destination, unit });
        }

        None
    }

    pub fn update_read_latch_halfword(&mut self, idx: u8, value: u16) {
        // Halfword reads duplicate the value in both low and high halfwords
        // Lufia: The Ruins of Lore depends on this
        let word = (u32::from(value) << 16) | u32::from(value);
        self.channels[idx as usize].last_read = word;
    }

    pub fn update_read_latch_word(&mut self, idx: u8, value: u32) {
        self.channels[idx as usize].last_read = value;
    }

    pub fn notify_vblank_start(&mut self) {
        self.activate_matching_channels(|channel| channel.start_timing == StartTiming::VBlank);
    }

    pub fn notify_hblank_start(&mut self) {
        self.activate_matching_channels(|channel| channel.start_timing == StartTiming::HBlank);
    }

    pub fn notify_video_capture(&mut self) {
        self.activate_matching_channels(|channel| {
            channel.idx == 3 && channel.start_timing == StartTiming::Special
        });
    }

    pub fn end_video_capture(&mut self) {
        if self.channels[3].dma_enabled && self.channels[3].start_timing == StartTiming::Special {
            self.channels[3].dma_enabled = false;
            self.channels[3].dma_active = false;
        }
    }

    pub fn video_capture_active(&self) -> bool {
        self.channels[3].dma_enabled && self.channels[3].start_timing == StartTiming::Special
    }

    pub fn notify_apu_fifo_a(&mut self) {
        self.activate_matching_channels(|channel| {
            channel.idx == 1 && channel.start_timing == StartTiming::Special
        });
    }

    pub fn notify_apu_fifo_b(&mut self) {
        self.activate_matching_channels(|channel| {
            channel.idx == 2 && channel.start_timing == StartTiming::Special
        });
    }

    fn activate_matching_channels(&mut self, predicate: impl Fn(&mut DmaChannel) -> bool) {
        for channel in &mut self.channels {
            if channel.dma_enabled && !channel.dma_active && predicate(channel) {
                channel.dma_active = true;
                channel.start_latency = INITIAL_START_LATENCY;

                self.any_active = true;
                self.any_start_latency = true;
            }
        }
    }

    pub fn read_register(&self, address: u32) -> Option<u16> {
        let value = match address {
            0x40000B8 | 0x40000C4 | 0x40000D0 | 0x40000DC => {
                // Low halfword of word-size control reads (length is not readable)
                0
            }
            0x40000BA => self.channels[0].read_control(),
            0x40000C6 => self.channels[1].read_control(),
            0x40000D2 => self.channels[2].read_control(),
            0x40000DE => self.channels[3].read_control(),
            _ => {
                log::debug!("Unexpected read from write-only DMA register {address:08X}");
                return None;
            }
        };

        Some(value)
    }

    pub fn write_register(&mut self, address: u32, value: u16, cartridge: &mut Cartridge) {
        debug_assert!((0x40000B0..0x40000E0).contains(&address));

        let dma_base_address = address - 0x40000B0;
        let channel = (dma_base_address / 0xC) as usize;
        let offset = dma_base_address % 0xC;

        match offset {
            0x0 => self.channels[channel].write_source_low(value),
            0x2 => self.channels[channel].write_source_high(value),
            0x4 => self.channels[channel].write_destination_low(value),
            0x6 => self.channels[channel].write_destination_high(value),
            0x8 => self.channels[channel].write_length(value),
            0xA => {
                if channel == 3 {
                    self.write_channel_3_control(value, cartridge);
                } else {
                    self.channels[channel].write_control(value);
                }
            }
            _ => {
                log::error!("Invalid DMA register address: {address:08X} {value:04X}");
            }
        }

        self.any_active = self.channels.iter().any(|channel| channel.dma_active);
        self.any_start_latency = self.channels.iter().any(|channel| channel.start_latency != 0);
    }

    // TODO I'm not sure any of this is correct
    pub fn write_register_byte(&mut self, address: u32, value: u8, cartridge: &mut Cartridge) {
        fn set_byte(mut halfword: u16, i: u32, byte: u8) -> u16 {
            if i & 1 == 0 {
                halfword.set_lsb(byte);
            } else {
                halfword.set_msb(byte);
            }
            halfword
        }

        debug_assert!((0x40000B0..0x40000E0).contains(&address));

        log::debug!("DMA byte write: {address:08X} {value:02X}");

        let dma_base_address = address - 0x40000B0;
        let channel = (dma_base_address / 0xC) as usize;
        let offset = dma_base_address % 0xC;

        match offset {
            0x0 => {
                let source_low = self.channels[channel].source_address as u16;
                let source_low = set_byte(source_low, address, value);
                self.channels[channel].write_source_low(source_low);
            }
            0x2 => {
                let source_high = (self.channels[channel].source_address >> 16) as u16;
                let source_high = set_byte(source_high, address, value);
                self.channels[channel].write_source_high(source_high);
            }
            0x4 => {
                let destination_low = self.channels[channel].destination_address as u16;
                let destination_low = set_byte(destination_low, address, value);
                self.channels[channel].write_destination_low(destination_low);
            }
            0x6 => {
                let destination_high = (self.channels[channel].destination_address >> 16) as u16;
                let destination_high = set_byte(destination_high, address, value);
                self.channels[channel].write_destination_high(destination_high);
            }
            0x8 => {
                let length = set_byte(self.channels[channel].length, address, value);
                self.channels[channel].write_length(length);
            }
            0xA => {
                let control = set_byte(self.channels[channel].read_control(), address, value);
                if channel == 3 {
                    self.write_channel_3_control(control, cartridge);
                } else {
                    self.channels[channel].write_control(control);
                }
            }
            _ => {
                log::error!("Invalid DMA register address: {address:08X} {value:02X}");
            }
        }
    }

    fn write_channel_3_control(&mut self, value: u16, cartridge: &mut Cartridge) {
        let prev_enabled = self.channels[3].dma_enabled;
        self.channels[3].write_control(value);

        if !prev_enabled
            && self.channels[3].dma_enabled
            && (0xD000000..0xE000000).contains(&self.channels[3].destination_address)
        {
            cartridge.notify_dma_to_rom(
                self.channels[3].destination_address,
                self.channels[3].length,
                self.channels[3].unit,
            );
        }
    }
}
