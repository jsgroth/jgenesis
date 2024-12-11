//! GBA control registers and DMA

use crate::apu;
use crate::apu::Apu;
use crate::bus::Bus;
use arm7tdmi_emu::bus::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum InterruptType {
    VBlank,
    HBlank,
    VCounterMatch,
    Timer0,
    Timer1,
    Timer2,
    Timer3,
    Serial,
    Dma0,
    Dma1,
    Dma2,
    Dma3,
    Keypad,
    GamePak,
}

impl InterruptType {
    pub const fn to_bit(self) -> u16 {
        match self {
            Self::VBlank => 1 << 0,
            Self::HBlank => 1 << 1,
            Self::VCounterMatch => 1 << 2,
            Self::Timer0 => 1 << 3,
            Self::Timer1 => 1 << 4,
            Self::Timer2 => 1 << 5,
            Self::Timer3 => 1 << 6,
            Self::Serial => 1 << 7,
            Self::Dma0 => 1 << 8,
            Self::Dma1 => 1 << 9,
            Self::Dma2 => 1 << 10,
            Self::Dma3 => 1 << 11,
            Self::Keypad => 1 << 12,
            Self::GamePak => 1 << 13,
        }
    }

    pub fn timer(timer_idx: usize) -> Self {
        [Self::Timer0, Self::Timer1, Self::Timer2, Self::Timer3][timer_idx]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaAddressStep {
    #[default]
    Increment = 0,
    Decrement = 1,
    Fixed = 2,
    IncrementReload = 3,
}

impl DmaAddressStep {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Increment,
            1 => Self::Decrement,
            2 => Self::Fixed,
            3 => Self::IncrementReload,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn apply(self, address: u32, step: u32) -> u32 {
        match self {
            Self::Increment | Self::IncrementReload => address.wrapping_add(step),
            Self::Decrement => address.wrapping_sub(step),
            Self::Fixed => address,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaTransferUnit {
    #[default]
    Halfword = 0,
    Word = 1,
}

impl DmaTransferUnit {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Word } else { Self::Halfword }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaStartTrigger {
    #[default]
    Immediate = 0,
    VBlank = 1,
    HBlank = 2,
    // Sound FIFO DRQ for DMA1 and DMA2, video capture DRQ for DMA3
    Special = 3,
}

impl DmaStartTrigger {
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
pub struct DmaChannel {
    pub idx: u32,
    pub irq: InterruptType,
    // DMAxSAD: DMA source address
    pub source_address: u32,
    // DMAxDAD: DMA destination address
    pub dest_address: u32,
    // DMAxCNT_L: DMA length
    pub length: u16,
    // DMAxCNT_H: DMA control
    pub dest_address_step: DmaAddressStep,
    pub source_address_step: DmaAddressStep,
    pub repeat: bool,
    pub unit: DmaTransferUnit,
    pub game_pak_drq: bool,
    pub start_trigger: DmaStartTrigger,
    pub irq_enabled: bool,
    pub enabled: bool,
    // Internal registers
    pub internal_source_address: u32,
    pub internal_dest_address: u32,
    pub internal_length: u32,
}

impl DmaChannel {
    pub fn new(idx: u32) -> Self {
        let interrupt = match idx {
            0 => InterruptType::Dma0,
            1 => InterruptType::Dma1,
            2 => InterruptType::Dma2,
            3 => InterruptType::Dma3,
            _ => panic!("Invalid DMA channel index: {idx}"),
        };

        Self {
            idx,
            irq: interrupt,
            source_address: 0,
            dest_address: 0,
            length: 0x4000,
            dest_address_step: DmaAddressStep::default(),
            source_address_step: DmaAddressStep::default(),
            repeat: false,
            unit: DmaTransferUnit::default(),
            game_pak_drq: false,
            start_trigger: DmaStartTrigger::default(),
            irq_enabled: false,
            enabled: false,
            internal_source_address: 0,
            internal_dest_address: 0,
            internal_length: 0x4000,
        }
    }

    // $040000B0: DMA0SAD_L (DMA0 source address low)
    // $040000BC: DMA1SAD_L
    // $040000C8: DMA2SAD_L
    // $040000D4: DMA3SAD_L
    pub fn write_sad_low(&mut self, value: u16) {
        self.source_address = (self.source_address & 0xFFFF0000) | u32::from(value);
        log::trace!("DMA{}SAD_L write: {value:04X}", self.idx);
        log::trace!("  Source address: {:08X}", self.source_address);
    }

    // $040000B2: DMA0SAD_H (DMA0 source address high)
    // $040000BE: DMA1SAD_H
    // $040000CA: DMA2SAD_H
    // $040000D6: DMA3SAD_H
    pub fn write_sad_high(&mut self, value: u16) {
        self.source_address = (self.source_address & 0x0000FFFF) | (u32::from(value) << 16);
        log::trace!("DMA{}SAD_H write: {value:04X}", self.idx);
        log::trace!("  Source address: {:08X}", self.source_address);
    }

    // $040000B4: DMA0DAD_L (DMA0 destination address low)
    // $040000C0: DMA1DAD_L
    // $040000CC: DMA2DAD_L
    // $040000D8: DMA3DAD_L
    pub fn write_dad_low(&mut self, value: u16) {
        self.dest_address = (self.dest_address & 0xFFFF0000) | u32::from(value);
        log::trace!("DMA{}DAD_L write: {value:04X}", self.idx);
        log::trace!("  Destination address: {:08X}", self.dest_address);
    }

    // $040000B6: DMA0DAD_H (DMA0 destination address high)
    // $040000C2: DMA1DAD_H
    // $040000CE: DMA2DAD_H
    // $040000DA: DMA3DAD_H
    pub fn write_dad_high(&mut self, value: u16) {
        self.dest_address = (self.dest_address & 0x0000FFFF) | (u32::from(value) << 16);
        log::trace!("DMA{}DAD_H write: {value:04X}", self.idx);
        log::trace!("  Destination address: {:08X}", self.dest_address);
    }

    // $040000B8: DMA0CNT_L (DMA0 word count)
    // $040000C4: DMA1CNT_L
    // $040000D0: DMA2CNT_L
    // $040000DC: DMA3CNT_L
    pub fn write_cnt_low(&mut self, value: u16) {
        // Value is 14 bits, and 0 is treated as 0x4000
        self.length = value & 0x3FFF;
        if self.length == 0 {
            self.length = 0x4000;
        }
        log::trace!("DMA{}CNT_L write: {value:04X}", self.idx);
        log::trace!("  Word length: {}", self.length);
    }

    // $040000BA: DMA0CNT_H (DMA0 control)
    // $040000C6: DMA1CNT_H
    // $040000D2: DMA2CNT_H
    // $040000DE: DMA3CNT_H
    pub fn read_cnt_high(&self) -> u16 {
        log::trace!("Read DMA{}CNT_H", self.idx);

        ((self.dest_address_step as u16) << 5)
            | ((self.source_address_step as u16) << 7)
            | (u16::from(self.repeat) << 9)
            | ((self.unit as u16) << 10)
            | (u16::from(self.game_pak_drq) << 11)
            | ((self.start_trigger as u16) << 12)
            | (u16::from(self.irq_enabled) << 14)
            | (u16::from(self.enabled) << 15)
    }

    // $040000BA: DMA0CNT_H (DMA0 control)
    // $040000C6: DMA1CNT_H
    // $040000D2: DMA2CNT_H
    // $040000DE: DMA3CNT_H
    pub fn write_cnt_high(&mut self, value: u16, state: &mut DmaState) {
        self.dest_address_step = DmaAddressStep::from_bits(value >> 5);
        self.source_address_step = DmaAddressStep::from_bits(value >> 7);
        self.repeat = value.bit(9);
        self.unit = DmaTransferUnit::from_bit(value.bit(10));
        self.game_pak_drq = value.bit(11);
        self.start_trigger = DmaStartTrigger::from_bits(value >> 12);
        self.irq_enabled = value.bit(14);

        let enabled = value.bit(15);
        if !self.enabled && enabled {
            // Reload internal registers
            // TODO address validation - only DMA1/DMA2 can write to audio FIFOs, and only DMA3 can access cartridge
            self.internal_source_address = self.source_address;
            self.internal_dest_address = self.dest_address;
            self.internal_length = self.length.into();

            if self.start_trigger == DmaStartTrigger::Immediate {
                state.add_active_channel(self.idx);
            }
        } else if self.enabled && !enabled {
            state.remove_active_channel(self.idx);
        }
        self.enabled = enabled;

        log::trace!("DMA{}CNT_H write: {value:04X}", self.idx);
        log::trace!("  Destination address step: {:?}", self.dest_address_step);
        log::trace!("  Source address step: {:?}", self.source_address_step);
        log::trace!("  Repeat: {}", self.repeat);
        log::trace!("  Transfer unit: {:?}", self.unit);
        log::trace!("  DMA3 Game Pak DRQ: {}", self.game_pak_drq);
        log::trace!("  Start trigger: {:?}", self.start_trigger);
        log::trace!("  IRQ enabled: {}", self.irq_enabled);
        log::trace!("  DMA enabled: {}", self.enabled);
    }

    pub fn run_dma(&mut self, bus: &mut Bus<'_>) -> u32 {
        log::debug!(
            "Running DMA on channel {}; source=${:X}, dest=${:X}, length={}",
            self.idx,
            self.internal_source_address,
            self.internal_dest_address,
            self.internal_length
        );

        let audio_mode =
            (self.idx == 1 || self.idx == 2) && self.start_trigger == DmaStartTrigger::Special;
        if audio_mode {
            // Audio DMA ignores length, unit, and destination address step
            for _ in 0..4 {
                let word = bus.read_word(self.internal_source_address);
                bus.write_word(self.internal_dest_address, word);

                self.internal_source_address =
                    self.source_address_step.apply(self.internal_source_address, 4);
            }
        } else {
            match self.unit {
                DmaTransferUnit::Halfword => {
                    for _ in 0..self.internal_length {
                        let halfword = bus.read_halfword(self.internal_source_address);
                        bus.write_halfword(self.internal_dest_address, halfword);

                        self.internal_source_address =
                            self.source_address_step.apply(self.internal_source_address, 2);
                        self.internal_dest_address =
                            self.dest_address_step.apply(self.internal_dest_address, 2);
                    }
                }
                DmaTransferUnit::Word => {
                    for _ in 0..self.internal_length {
                        let word = bus.read_word(self.internal_source_address);
                        bus.write_word(self.internal_dest_address, word);

                        self.internal_source_address =
                            self.source_address_step.apply(self.internal_source_address, 4);
                        self.internal_dest_address =
                            self.dest_address_step.apply(self.internal_dest_address, 4);
                    }
                }
            }
        }

        // 2N + 2*(n-1)*S + 2I
        // TODO +2I if source and destination addresses are both in cartridge memory
        let effective_length = if audio_mode { 4 } else { self.internal_length };
        let cycles = 4 + 2 * (effective_length - 1);

        self.internal_length = 0;
        self.enabled = self.repeat;
        if self.repeat {
            self.internal_length = self.length.into();
            if self.dest_address_step == DmaAddressStep::IncrementReload {
                self.internal_dest_address = self.dest_address;
            }
        }

        bus.control.dma_state.remove_current_active_channel();

        if self.irq_enabled {
            bus.control.set_interrupt_flag(self.irq);
        }

        cycles
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaState {
    pub active_channels: Vec<u32>,
}

impl DmaState {
    fn new() -> Self {
        Self { active_channels: Vec::with_capacity(4) }
    }

    fn add_active_channel(&mut self, channel: u32) {
        if self.active_channels.contains(&channel) {
            return;
        }

        let i = self
            .active_channels
            .iter()
            .position(|&other_channel| other_channel > channel)
            .unwrap_or(self.active_channels.len());
        self.active_channels.insert(i, channel);
    }

    fn remove_current_active_channel(&mut self) {
        self.active_channels.remove(0);
    }

    fn remove_active_channel(&mut self, channel: u32) {
        self.active_channels.retain(|&active_channel| active_channel != channel);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ControlRegisters {
    // IE: Interrupts enabled
    pub interrupts_enabled: u16,
    // IF: Interrupt flags
    pub interrupt_flags: u16,
    // WAITCNT: Waitstate control
    // TODO implement memory access timings
    pub waitcnt: u16,
    // IME: Interrupt master enable flag
    pub ime: bool,
    // POSTFLG: Post boot / debug control
    pub postflg: u16,
    // DMA channels
    pub dma: [DmaChannel; 4],
    pub dma_state: DmaState,
}

impl ControlRegisters {
    pub fn new() -> Self {
        Self {
            interrupts_enabled: 0,
            interrupt_flags: 0,
            waitcnt: 0,
            ime: false,
            postflg: 0,
            dma: array::from_fn(|i| DmaChannel::new(i as u32)),
            dma_state: DmaState::new(),
        }
    }

    // $04000200: IE (Interrupts enabled)
    pub fn read_ie(&self) -> u16 {
        self.interrupts_enabled
    }

    // $04000200: IE (Interrupts enabled)
    pub fn write_ie(&mut self, value: u16) {
        self.interrupts_enabled = value;

        log::trace!("IE write: {value:04X}");
        log::trace!("  VBlank: {}", value.bit(0));
        log::trace!("  HBlank: {}", value.bit(1));
        log::trace!("  V counter match: {}", value.bit(2));
        log::trace!("  Timer 0 overflow: {}", value.bit(3));
        log::trace!("  Timer 1 overflow: {}", value.bit(4));
        log::trace!("  Timer 2 overflow: {}", value.bit(5));
        log::trace!("  Timer 3 overflow: {}", value.bit(6));
        log::trace!("  Serial: {}", value.bit(7));
        log::trace!("  DMA 0: {}", value.bit(8));
        log::trace!("  DMA 1: {}", value.bit(9));
        log::trace!("  DMA 2: {}", value.bit(10));
        log::trace!("  DMA 3: {}", value.bit(11));
        log::trace!("  Keypad: {}", value.bit(12));
        log::trace!("  Game Pak: {}", value.bit(13));
    }

    // $04000202: IF (Interrupt flags)
    pub fn read_if(&self) -> u16 {
        self.interrupt_flags
    }

    // $04000202: IF (Interrupt flags)
    pub fn write_if(&mut self, value: u16) {
        // IF writes clear all bits set to 1 in the written value
        self.interrupt_flags &= !value;

        log::trace!("IF write: {value:04X}");
    }

    // $04000204: WAITCNT (Waitstate control)
    pub fn read_waitcnt(&self) -> u16 {
        self.waitcnt
    }

    // $04000204: WAITCNT (Waitstate control)
    pub fn write_waitcnt(&mut self, value: u16) {
        // Bit 15 (GBA cartridge vs. GBC cartridge) is not writable
        self.waitcnt = value & 0x7FFF;

        log::warn!(
            "Unhandled WAITCNT write: {value:04X}, prefetch buffer enabled: {}",
            value.bit(14)
        );
    }

    // $04000208: IME (Interrupt master enable)
    pub fn read_ime(&self) -> u16 {
        self.ime.into()
    }

    // $04000208: IME (Interrupt master enable)
    pub fn write_ime(&mut self, value: u16) {
        self.ime = value.bit(0);

        log::trace!("IME: {}", self.ime);
    }

    // $04000300: POSTFLG (Post boot / debug control)
    pub fn write_postflg(&mut self, value: u16) {
        self.postflg = value;
    }

    pub fn read_dma_register(&self, address: u32) -> u16 {
        let channel = ((address / 12) & 3) as usize;
        match address % 12 {
            0xA => self.dma[channel].read_cnt_high(),
            _ => {
                log::debug!("Read from write-only DMA register: {address:08X}");
                0
            }
        }
    }

    pub fn write_dma_register(&mut self, address: u32, value: u16) {
        let channel = ((address / 12) & 3) as usize;
        match address % 12 {
            0x0 => self.dma[channel].write_sad_low(value),
            0x2 => self.dma[channel].write_sad_high(value),
            0x4 => self.dma[channel].write_dad_low(value),
            0x6 => self.dma[channel].write_dad_high(value),
            0x8 => self.dma[channel].write_cnt_low(value),
            0xA => self.dma[channel].write_cnt_high(value, &mut self.dma_state),
            _ => panic!("Invalid DMA register address: {address:08X} {value:04X}"),
        }
    }

    pub fn trigger_vblank_dma(&mut self) {
        for (channel_idx, channel) in self.dma.iter().enumerate() {
            if channel.enabled && channel.start_trigger == DmaStartTrigger::VBlank {
                self.dma_state.add_active_channel(channel_idx as u32);
            }
        }
    }

    pub fn trigger_hblank_dma(&mut self) {
        for (channel_idx, channel) in self.dma.iter().enumerate() {
            if channel.enabled && channel.start_trigger == DmaStartTrigger::HBlank {
                self.dma_state.add_active_channel(channel_idx as u32);
            }
        }
    }

    pub fn update_audio_drq(&mut self, apu: &Apu) {
        if apu.fifo_a_drq() {
            for channel in [1, 2] {
                if self.dma[channel].enabled
                    && self.dma[channel].start_trigger == DmaStartTrigger::Special
                    && self.dma[channel].internal_dest_address == apu::FIFO_A_ADDRESS
                {
                    log::trace!("Adding {channel} for FIFO A; len is {}", apu.fifo_a_len());
                    self.dma_state.add_active_channel(channel as u32);
                }
            }
        }

        if apu.fifo_b_drq() {
            for channel in [1, 2] {
                if self.dma[channel].enabled
                    && self.dma[channel].start_trigger == DmaStartTrigger::Special
                    && self.dma[channel].internal_dest_address == apu::FIFO_B_ADDRESS
                {
                    log::trace!("Adding {channel} for FIFO B; len is {}", apu.fifo_b_len());
                    self.dma_state.add_active_channel(channel as u32);
                }
            }
        }
    }

    pub fn set_interrupt_flag(&mut self, interrupt: InterruptType) {
        self.interrupt_flags |= interrupt.to_bit();
        log::trace!("Set interrupt flag {interrupt:?}");
    }

    pub fn irq(&self) -> bool {
        self.ime && self.interrupts_enabled & self.interrupt_flags != 0
    }
}
