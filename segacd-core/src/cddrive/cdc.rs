use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::memory::ScdCpu;
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::cmp;

const BUFFER_RAM_LEN: usize = 16 * 1024;
const BUFFER_RAM_ADDRESS_MASK: u16 = (1 << 14) - 1;

pub const PLAY_DELAY_CLOCKS: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DeviceDestination {
    #[default]
    MainCpuRegister,
    SubCpuRegister,
    Pcm,
    PrgRam,
    WordRam,
}

impl DeviceDestination {
    pub fn to_bits(self) -> u8 {
        match self {
            Self::MainCpuRegister => 0b010,
            Self::SubCpuRegister => 0b011,
            Self::Pcm => 0b100,
            Self::PrgRam => 0b101,
            Self::WordRam => 0b111,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x07 {
            0b010 => Self::MainCpuRegister,
            0b011 => Self::SubCpuRegister,
            0b100 => Self::Pcm,
            0b101 => Self::PrgRam,
            0b111 => Self::WordRam,
            0b000 | 0b001 | 0b110 => {
                // Prohibited; just default to main CPU register
                log::warn!("Prohibited CDC device destination set: {:03b}", bits & 0x07);
                Self::MainCpuRegister
            }
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }
}

// The LC8951, which the documentation describes as a "Real-Time Error Correction and Host Interface
// Processor".
//
// Sega CD documentation refers to this chip as the CDC.
#[derive(Debug, Clone, Encode, Decode)]
pub struct Rchip {
    buffer_ram: Box<[u8; BUFFER_RAM_LEN]>,
    device_destination: DeviceDestination,
    register_address: u8,
    dma_address: u32,
    decoder_enabled: bool,
    decoder_writes_enabled: bool,
    decoded_first_written_block: bool,
    data_out_enabled: bool,
    subheader_data_enabled: bool,
    header_data: [u8; 4],
    subheader_data: [u8; 4],
    write_address: u16,
    block_pointer: u16,
    data_byte_counter: u16,
    data_address_counter: u16,
    transfer_end_interrupt_enabled: bool,
    transfer_end_interrupt_pending: bool,
    decoder_interrupt_enabled: bool,
    decoder_interrupt_pending: bool,
}

impl Rchip {
    pub(super) fn new() -> Self {
        Self {
            buffer_ram: vec![0; BUFFER_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            device_destination: DeviceDestination::default(),
            register_address: 0,
            dma_address: 0,
            decoder_enabled: false,
            decoder_writes_enabled: false,
            decoded_first_written_block: false,
            data_out_enabled: false,
            subheader_data_enabled: false,
            header_data: [0; 4],
            subheader_data: [0; 4],
            write_address: 0,
            block_pointer: 0,
            data_byte_counter: 0,
            data_address_counter: 0,
            transfer_end_interrupt_enabled: true,
            transfer_end_interrupt_pending: false,
            decoder_interrupt_enabled: true,
            decoder_interrupt_pending: false,
        }
    }

    pub fn device_destination(&self) -> DeviceDestination {
        self.device_destination
    }

    pub fn set_device_destination(&mut self, device_destination: DeviceDestination) {
        // Changing device destination resets DMA controller
        if device_destination != self.device_destination {
            // TODO cancel any in-progress DMA
            self.dma_address = 0;
        }

        log::trace!("CDC device destination set to {device_destination:?}");

        self.device_destination = device_destination;
    }

    pub fn read_host_data(&mut self, cpu: ScdCpu) -> u16 {
        if (cpu == ScdCpu::Main && self.device_destination != DeviceDestination::MainCpuRegister)
            || (cpu == ScdCpu::Sub && self.device_destination != DeviceDestination::SubCpuRegister)
        {
            // Invalid host data read
            return 0x0000;
        }

        todo!("read host data ({cpu:?})")
    }

    pub fn register_address(&self) -> u8 {
        self.register_address
    }

    pub fn set_register_address(&mut self, register_address: u8) {
        self.register_address = register_address;
    }

    pub fn read_register(&mut self) -> u8 {
        let value = match self.register_address {
            1 => {
                // IFSTAT (Host Interface Status)
                // Hardcode CMDI, STBSY, STEI, and bit 4 (unused) to 1
                log::trace!("IFSTAT read");

                // TODO DTBSY and DTEN bits; currently hardcoded to 1
                0x95 | 0x0A
                    | (u8::from(!self.transfer_end_interrupt_pending) << 6)
                    | (u8::from(!self.decoder_interrupt_pending) << 5)
            }
            4..=7 => {
                // HEAD0-3 (Header/Subheader Data)

                let idx = self.register_address - 4;
                log::trace!("HEAD{idx} read");

                if self.subheader_data_enabled {
                    self.subheader_data[idx as usize]
                } else {
                    self.header_data[idx as usize]
                }
            }
            8 => {
                // PTL (Block Pointer, Low Byte)
                log::trace!("PTL read");

                self.block_pointer as u8
            }
            9 => {
                // PTH (Block Pointer, High Byte)
                log::trace!("PTH read");

                (self.block_pointer >> 8) as u8
            }
            12 => {
                // STAT0 (Status 0)
                log::trace!("STAT0 read");

                // Hardcode CRCOK to 1 and all other bits (various error conditions) to 0
                0x80
            }
            13 => {
                // STAT1 (Status 1)
                log::trace!("STAT1 read");

                // Error flags for header/subheader data registers; hardcode all to 0
                0x00
            }
            14 => {
                // STAT2 (Status 2)
                log::trace!("STAT2 read");

                // TODO figure out what to put here
                0x00
            }
            15 => {
                // STAT3 (Status 3)
                log::trace!("STAT3 read");

                // In actual hardware VALST remains low for a short amount of time after the
                // decoder interrupt is generated, but the BIOS shouldn't read STAT3 multiple
                // times per interrupt
                let value = u8::from(!self.decoder_interrupt_pending) << 7;

                // Reading STAT3 clears the decoder interrupt
                self.decoder_interrupt_pending = false;

                // Hardcode WLONG and CBLK to 0; bits 4-0 are unused
                value
            }
            0 | 2 | 3 | 10 | 11 => todo!("CDC register read {}", self.register_address),
            _ => panic!("CDC register address should always be <= 15"),
        };

        self.increment_register_address();

        value
    }

    pub fn write_register(&mut self, value: u8) {
        match self.register_address {
            1 => {
                // IFCTRL (Host Interface Control)
                // Intentionally ignoring CMDIEN, CMDBK, DTWAI, STWAI, SOUTEN bits
                log::trace!("IFCTRL write: {value:02X}");

                self.transfer_end_interrupt_enabled = value.bit(6);
                self.decoder_interrupt_enabled = value.bit(5);

                self.data_out_enabled = value.bit(1);

                log::trace!("  DTEIEN: {}", self.transfer_end_interrupt_enabled);
                log::trace!("  DECIEN: {}", self.decoder_interrupt_enabled);
                log::trace!("  DOUTEN: {}", self.data_out_enabled);
            }
            2 => {
                // DBCL (Data Byte Counter, Low Byte)
                log::trace!("DBCL write: {value:02X}");

                self.data_byte_counter = (self.data_byte_counter & 0xFF00) | u16::from(value);

                log::trace!("  DBC: {:04X}", self.data_byte_counter);
            }
            3 => {
                // DBCH (Data Byte Counter, High Byte)
                log::trace!("DBCH write: {value:02X}");

                // DBC is only a 12-bit counter; mask out the highest 4 bits
                self.data_byte_counter =
                    (self.data_byte_counter & 0x00FF) | (u16::from(value & 0x0F) << 8);

                log::trace!("  DBC: {:04X}", self.data_byte_counter);
            }
            4 => {
                // DACL (Data Address Counter, Low Byte)
                log::trace!("DACL write: {value:02X}");

                self.data_address_counter = (self.data_address_counter & 0xFF00) | u16::from(value);

                log::trace!("  DAC: {:04X}", self.data_address_counter);
            }
            5 => {
                // DACH (Data Address Counter, High Byte)
                log::trace!("DACH write: {value:02X}");

                self.data_address_counter =
                    (self.data_address_counter & 0x00FF) | (u16::from(value) << 8);

                log::trace!("  DAC: {:04X}", self.data_address_counter);
            }
            8 => {
                // WAL (Write Address, Low Byte)
                log::trace!("WAL write: {value:02X}");

                self.write_address = (self.write_address & 0xFF00) | u16::from(value);

                log::trace!("  WA: {:04X}", self.write_address);
            }
            9 => {
                // WAH (Write Address, High Byte)
                log::trace!("WAH write: {value:02X}");

                self.write_address = (self.write_address & 0x00FF) | (u16::from(value) << 8);

                log::trace!("  WA: {:04X}", self.write_address);
            }
            10 => {
                // CTRL0 (Control 0)
                // Intentionally ignore all bits except DECEN and WRRQ; the other bits are related
                // to error detection and correction settings
                log::trace!("CTRL0 write: {value:02X}");

                self.decoder_enabled = value.bit(7);
                self.decoder_writes_enabled = value.bit(2);

                // Disabling the decoder also disables any pending interrupt
                if !self.decoder_enabled {
                    self.decoder_interrupt_pending = false;
                }

                if !self.decoder_enabled || !self.decoder_writes_enabled {
                    self.decoded_first_written_block = false;
                }

                log::trace!("  DECEN: {}", self.decoder_enabled);
                log::trace!("  WRRQ: {}", self.decoder_writes_enabled);
            }
            11 => {
                // CTRL1 (Control 1)
                log::trace!("CTRL1 write: {value:02X}");

                self.subheader_data_enabled = value.bit(0);

                log::trace!("  SHDREN: {}", self.subheader_data_enabled);
            }
            12 => {
                // PTL (Block Pointer, Low Byte)
                log::trace!("PTL write: {value:02X}");

                self.block_pointer = (self.block_pointer & 0xFF00) | u16::from(value);

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            13 => {
                // PTH (Block Pointer, High Byte)
                log::trace!("PTH write: {value:02X}");

                self.block_pointer = (self.block_pointer & 0x00FF) | (u16::from(value) << 8);

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            14 => {
                // Unused, do nothing
            }
            15 => {
                // RESET
                log::trace!("RESET write");

                self.transfer_end_interrupt_enabled = true;
                self.transfer_end_interrupt_pending = false;
                self.decoder_interrupt_enabled = true;
                self.decoder_interrupt_pending = false;
                self.data_out_enabled = false;
                self.decoder_enabled = false;
                self.decoder_writes_enabled = false;
                self.subheader_data_enabled = false;
            }
            0 | 6 | 7 => {
                todo!("write CDC register {}", self.register_address)
            }
            _ => panic!("CDC register address should always be <= 15"),
        }

        self.increment_register_address();
    }

    fn increment_register_address(&mut self) {
        // Register address automatically increments on each access when it is not 0
        if self.register_address != 0 {
            self.register_address = (self.register_address + 1) & 0x0F;
        }
    }

    pub fn set_dma_address(&mut self, dma_address: u32) {
        log::trace!("CDC DMA address set to {dma_address:X}");
        self.dma_address = dma_address;
    }

    pub fn interrupt_pending(&self) -> bool {
        (self.decoder_interrupt_enabled && self.decoder_interrupt_pending)
            || (self.transfer_end_interrupt_enabled && self.transfer_end_interrupt_pending)
    }

    pub(super) fn decode_block(&mut self, sector_buffer: &[u8; cdrom::BYTES_PER_SECTOR as usize]) {
        if !self.decoder_enabled {
            return;
        }

        // Header data and subheader data are always read from bytes 12-15 and 16-19 respectively
        self.header_data.copy_from_slice(&sector_buffer[12..16]);
        self.subheader_data.copy_from_slice(&sector_buffer[16..20]);

        self.decoder_interrupt_pending = true;

        if self.decoder_writes_enabled {
            for &byte in sector_buffer {
                self.buffer_ram[self.write_address as usize] = byte;
                self.write_address = (self.write_address + 1) & BUFFER_RAM_ADDRESS_MASK;
            }

            if self.decoded_first_written_block {
                self.block_pointer =
                    (self.block_pointer + cdrom::BYTES_PER_SECTOR as u16) & BUFFER_RAM_ADDRESS_MASK;
            } else {
                // Decoded blocks start at the header, skipping the 12-byte sync
                self.block_pointer = (self.block_pointer + 12) & BUFFER_RAM_ADDRESS_MASK;

                self.decoded_first_written_block = true;
            }

            log::trace!(
                "Performed decoder write; write address = {:04X}, block pointer = {:04X}",
                self.write_address,
                self.block_pointer
            );
        }
    }
}

pub fn estimate_seek_clocks(current_time: CdTime, seek_time: CdTime) -> u8 {
    let diff =
        if current_time >= seek_time { current_time - seek_time } else { seek_time - current_time };

    // It supposedly takes roughly 1.5 seconds / 113 frames to seek from one end of the disc to the
    // other, so scale based on that
    let seek_cycles = (113.0 * f64::from(diff.to_frames())
        / f64::from(CdTime::DISC_END.to_frames()))
    .round() as u8;

    // Require seek to always take at least 1 cycle
    cmp::max(1, seek_cycles)
}
