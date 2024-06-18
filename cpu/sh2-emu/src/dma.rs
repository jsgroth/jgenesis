//! SH-2 DMA controller (DMAC)

use crate::bus::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaAddressMode {
    #[default]
    Fixed = 0,
    AutoIncrement = 1,
    AutoDecrement = 2,
    Invalid = 3,
}

impl DmaAddressMode {
    fn from_value(value: u32) -> Self {
        match value & 3 {
            0 => Self::Fixed,
            1 => Self::AutoIncrement,
            2 => Self::AutoDecrement,
            3 => Self::Invalid,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaTransferUnit {
    #[default]
    Byte = 0,
    Word = 1,
    Longword = 2,
    SixteenByte = 3,
}

impl DmaTransferUnit {
    fn from_value(value: u32) -> Self {
        match value & 3 {
            0 => Self::Byte,
            1 => Self::Word,
            2 => Self::Longword,
            3 => Self::SixteenByte,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaAckMode {
    #[default]
    Read = 0,
    Write = 1,
}

impl DmaAckMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Write } else { Self::Read }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DreqDetectionMode {
    #[default]
    Level = 0,
    Edge = 1,
}

impl DreqDetectionMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Edge } else { Self::Level }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaBusMode {
    #[default]
    CycleStealing = 0,
    Burst = 1,
}

impl DmaBusMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Burst } else { Self::CycleStealing }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaTransferAddressMode {
    #[default]
    Dual = 0,
    Single = 1,
}

impl DmaTransferAddressMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Single } else { Self::Dual }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct DmaChannelControl {
    pub source_address_mode: DmaAddressMode,
    pub destination_address_mode: DmaAddressMode,
    pub transfer_size: DmaTransferUnit,
    pub auto_request: bool,
    pub ack_mode: DmaAckMode,
    pub ack_level: bool,
    pub dreq_select: DreqDetectionMode,
    pub dreq_level: bool,
    pub bus_mode: DmaBusMode,
    pub transfer_address_mode: DmaTransferAddressMode,
    pub interrupt_enabled: bool,
    pub dma_complete: bool,
    pub dma_enabled: bool,
}

impl DmaChannelControl {
    fn to_register_value(&self) -> u32 {
        ((self.destination_address_mode as u32) << 14)
            | ((self.source_address_mode as u32) << 12)
            | ((self.transfer_size as u32) << 10)
            | (u32::from(self.auto_request) << 9)
            | ((self.ack_mode as u32) << 8)
            | (u32::from(self.ack_level) << 7)
            | ((self.dreq_select as u32) << 6)
            | (u32::from(self.dreq_level) << 5)
            | ((self.bus_mode as u32) << 4)
            | ((self.transfer_address_mode as u32) << 3)
            | (u32::from(self.interrupt_enabled) << 2)
            | (u32::from(self.dma_complete) << 1)
            | u32::from(self.dma_enabled)
    }

    fn write(&mut self, value: u32) {
        self.destination_address_mode = DmaAddressMode::from_value(value >> 14);
        self.source_address_mode = DmaAddressMode::from_value(value >> 12);
        self.transfer_size = DmaTransferUnit::from_value(value >> 10);
        self.auto_request = value.bit(9);
        self.ack_mode = DmaAckMode::from_bit(value.bit(8));
        self.ack_level = value.bit(7);
        self.dreq_select = DreqDetectionMode::from_bit(value.bit(6));
        self.dreq_level = value.bit(5);
        self.bus_mode = DmaBusMode::from_bit(value.bit(4));
        self.transfer_address_mode = DmaTransferAddressMode::from_bit(value.bit(3));
        self.interrupt_enabled = value.bit(2);
        // DMA complete flag can only be cleared by writes, not set
        self.dma_complete &= value.bit(1);
        self.dma_enabled = value.bit(0);
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_enabled && self.dma_complete && self.dma_enabled
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaChannel {
    pub source_address: u32,
    pub destination_address: u32,
    pub transfer_count: u32,
    pub control: DmaChannelControl,
    pub just_ran: bool,
}

impl DmaChannel {
    fn new() -> Self {
        Self {
            source_address: 0,
            destination_address: 0,
            transfer_count: 0,
            control: DmaChannelControl::default(),
            just_ran: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DmaPriorityMode {
    #[default]
    Fixed = 0,
    RoundRobin = 1,
}

impl DmaPriorityMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::RoundRobin } else { Self::Fixed }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct DmaOperation {
    pub priority: DmaPriorityMode,
    pub address_error: bool,
    pub dma_master_enabled: bool,
}

impl DmaOperation {
    pub fn read(&self) -> u32 {
        // TODO Bit 1 (NMI flag) hardcoded to 0
        ((self.priority as u32) << 3)
            | (u32::from(self.address_error) << 2)
            | u32::from(self.dma_master_enabled)
    }

    pub fn write(&mut self, value: u32) {
        self.priority = DmaPriorityMode::from_bit(value.bit(3));
        self.address_error &= value.bit(2);
        self.dma_master_enabled = value.bit(0);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaController {
    pub channels: [DmaChannel; 2],
    pub operation: DmaOperation,
}

impl DmaController {
    pub fn new() -> Self {
        Self { channels: array::from_fn(|_| DmaChannel::new()), operation: DmaOperation::default() }
    }

    pub fn read_register(&self, address: u32) -> u32 {
        match address {
            0xFFFFFF8C => self.channels[0].control.to_register_value(),
            0xFFFFFF9C => self.channels[1].control.to_register_value(),
            0xFFFFFFB0 => self.operation.read(),
            _ => todo!("Read DMA register {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u32) {
        match address {
            0xFFFFFF80 => {
                self.channels[0].source_address = value;
                log::trace!("DMAC channel 0 source address (SAR0): {value:08X}");
            }
            0xFFFFFF84 => {
                self.channels[0].destination_address = value;
                log::trace!("DMAC channel 0 destination address (DAR0): {value:08X}");
            }
            0xFFFFFF88 => {
                self.channels[0].transfer_count = value & 0xFFFFFF;
                log::trace!(
                    "DMAC channel 0 transfer count (TCR0): {:06X}",
                    self.channels[0].transfer_count
                );
            }
            0xFFFFFF8C => {
                self.channels[0].control.write(value);
                log::trace!("DMAC channel 0 control write (CHCR0): {value:08X}");
                log::trace!("  {:?}", self.channels[0].control);
            }
            0xFFFFFF90 => {
                self.channels[1].source_address = value;
                log::trace!("DMAC channel 1 source address (SAR1): {value:08X}");
            }
            0xFFFFFF94 => {
                self.channels[1].destination_address = value;
                log::trace!("DMAC channel 1 destination address (DAR1): {value:08X}");
            }
            0xFFFFFF98 => {
                self.channels[1].transfer_count = value & 0xFFFFFF;
                log::trace!(
                    "DMAC channel 1 transfer count (TCR1): {:06X}",
                    self.channels[1].transfer_count
                );
            }
            0xFFFFFF9C => {
                self.channels[1].control.write(value);
                log::trace!("DMAC channel 1 control write (CHCR1): {value:08X}");
                log::trace!("  {:?}", self.channels[1].control);
            }
            0xFFFFFFB0 => {
                self.operation.write(value);
                log::trace!("DMAOR write: {value:08X}");
                log::trace!("  {:?}", self.operation);
            }
            _ => todo!("Write DMA register {address:08X} {value:08X}"),
        }
    }

    pub fn channel_ready<B: BusInterface>(&mut self, bus: &mut B) -> Option<usize> {
        if !self.operation.dma_master_enabled || self.operation.address_error {
            return None;
        }

        // TODO respect priority

        for (idx, channel) in self.channels.iter_mut().enumerate() {
            if !channel.control.dma_enabled || channel.control.dma_complete {
                continue;
            }

            if channel.control.bus_mode == DmaBusMode::CycleStealing && channel.just_ran {
                channel.just_ran = false;
                continue;
            }

            if channel.control.auto_request
                || (idx == 0 && bus.dma_request_0())
                || (idx == 1 && bus.dma_request_1())
            {
                channel.just_ran = true;
                return Some(idx);
            }
        }

        None
    }
}
