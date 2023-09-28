use crate::memory::wordram::{WordRam, WordRamMode};
use crate::memory::ScdCpu;
use crate::rf5c164::Rf5c164;
use crate::{cdrom, memory};
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

const BUFFER_RAM_LEN: usize = 16 * 1024;
const BUFFER_RAM_ADDRESS_MASK: u16 = (1 << 14) - 1;

const DATA_TRACK_HEADER_LEN: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DeviceDestination {
    #[default]
    MainCpuRegister,
    SubCpuRegister,
    PrgRam,
    WordRam,
    Pcm,
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

    fn is_dma(self) -> bool {
        matches!(self, Self::Pcm | Self::PrgRam | Self::WordRam)
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
    data_transfer_in_progress: bool,
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
            data_transfer_in_progress: false,
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
        if device_destination != self.device_destination {
            // Abort any in-progress data transfer and reset DMA controller
            self.data_transfer_in_progress = false;
            self.dma_address = 0;
        }

        log::trace!("CDC device destination set to {device_destination:?}");

        self.device_destination = device_destination;
    }

    pub fn read_host_data(&mut self, cpu: ScdCpu) -> u16 {
        if !self.data_transfer_in_progress
            || (cpu == ScdCpu::Main
                && self.device_destination != DeviceDestination::MainCpuRegister)
            || (cpu == ScdCpu::Sub && self.device_destination != DeviceDestination::SubCpuRegister)
        {
            // Invalid host data read
            return 0x0000;
        }

        let msb_addr = self.data_address_counter;
        let lsb_addr = (self.data_address_counter + 1) & BUFFER_RAM_ADDRESS_MASK;
        let host_data = u16::from_be_bytes([
            self.buffer_ram[msb_addr as usize],
            self.buffer_ram[lsb_addr as usize],
        ]);
        self.data_address_counter = (self.data_address_counter + 2) & BUFFER_RAM_ADDRESS_MASK;

        let (new_byte_counter, overflowed) = self.data_byte_counter.overflowing_sub(2);
        self.data_byte_counter = new_byte_counter;
        if overflowed {
            self.data_transfer_in_progress = false;
            self.transfer_end_interrupt_pending = true;
        }

        log::trace!(
            "Host data read performed; data={host_data:04X}, DBC={new_byte_counter:04X}, ended={overflowed}"
        );

        host_data
    }

    pub fn register_address(&self) -> u8 {
        self.register_address
    }

    pub fn set_register_address(&mut self, register_address: u8) {
        self.register_address = register_address;
    }

    pub fn read_register(&mut self) -> u8 {
        let value = match self.register_address {
            0 => {
                // COMIN (Command Input)
                log::trace!("COMIN read");

                // Not used by Sega CD; return a dummy value
                0x00
            }
            1 => {
                // IFSTAT (Host Interface Status)
                // Hardcode CMDI, STBSY, STEN, and bit 4 (unused) to 1
                log::trace!("IFSTAT read");

                // TODO do DTBSY and DTEN need to be different values?
                0x95 | (u8::from(!self.transfer_end_interrupt_pending) << 6)
                    | (u8::from(!self.decoder_interrupt_pending) << 5)
                    | (u8::from(!self.data_transfer_in_progress) << 3)
                    | (u8::from(!self.data_transfer_in_progress) << 1)
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
            2 | 3 | 10 | 11 => todo!("CDC register read {}", self.register_address),
            _ => panic!("CDC register address should always be <= 15"),
        };

        self.increment_register_address();

        value
    }

    pub fn write_register(&mut self, value: u8) {
        match self.register_address {
            0 => {
                // SBOUT (Status Byte Output)
                log::trace!("SBOUT write: {value:02X}");

                // Not used by Sega CD; do nothing
            }
            1 => {
                // IFCTRL (Host Interface Control)
                log::trace!("IFCTRL write: {value:02X}");

                self.write_ifctrl(value);
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

                self.data_address_counter = ((self.data_address_counter & 0xFF00)
                    | u16::from(value))
                    & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  DAC: {:04X}", self.data_address_counter);
            }
            5 => {
                // DACH (Data Address Counter, High Byte)
                log::trace!("DACH write: {value:02X}");

                self.data_address_counter = ((self.data_address_counter & 0x00FF)
                    | (u16::from(value) << 8))
                    & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  DAC: {:04X}", self.data_address_counter);
            }
            6 => {
                // DTTRG (Data Transfer Trigger)
                log::trace!("DTTRG write");

                // Writing any value to this register initiates a data transfer if DOUTEN=1
                self.data_transfer_in_progress = self.data_out_enabled;
            }
            7 => {
                // DTACK (Data Transfer End Acknowledge)
                log::trace!("DTACK write");

                // Writing any value to this register clears the DTEI interrupt
                self.transfer_end_interrupt_pending = false;
            }
            8 => {
                // WAL (Write Address, Low Byte)
                log::trace!("WAL write: {value:02X}");

                self.write_address =
                    ((self.write_address & 0xFF00) | u16::from(value)) & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  WA: {:04X}", self.write_address);
            }
            9 => {
                // WAH (Write Address, High Byte)
                log::trace!("WAH write: {value:02X}");

                self.write_address = ((self.write_address & 0x00FF) | (u16::from(value) << 8))
                    & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  WA: {:04X}", self.write_address);
            }
            10 => {
                // CTRL0 (Control 0)
                // Intentionally ignore all bits except DECEN and WRRQ; the other bits are related
                // to error detection and correction settings
                log::trace!("CTRL0 write: {value:02X}");

                self.write_ctrl0(value);
            }
            11 => {
                // CTRL1 (Control 1)
                log::trace!("CTRL1 write: {value:02X}");

                self.write_ctrl1(value);
            }
            12 => {
                // PTL (Block Pointer, Low Byte)
                log::trace!("PTL write: {value:02X}");

                self.block_pointer =
                    ((self.block_pointer & 0xFF00) | u16::from(value)) & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            13 => {
                // PTH (Block Pointer, High Byte)
                log::trace!("PTH write: {value:02X}");

                self.block_pointer = ((self.block_pointer & 0x00FF) | (u16::from(value) << 8))
                    & BUFFER_RAM_ADDRESS_MASK;

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            14 => {
                // Unused, do nothing
            }
            15 => {
                // RESET
                log::trace!("RESET write");
                self.reset();
            }
            _ => panic!("CDC register address should always be <= 15"),
        }

        self.increment_register_address();
    }

    fn write_ifctrl(&mut self, value: u8) {
        // Intentionally ignoring CMDIEN, CMDBK, DTWAI, STWAI, SOUTEN bits

        self.transfer_end_interrupt_enabled = value.bit(6);
        self.decoder_interrupt_enabled = value.bit(5);

        self.data_out_enabled = value.bit(1);
        if !self.data_out_enabled {
            // Abort any in-progress data transfer
            self.data_transfer_in_progress = false;
        }

        log::trace!("  DTEIEN: {}", self.transfer_end_interrupt_enabled);
        log::trace!("  DECIEN: {}", self.decoder_interrupt_enabled);
        log::trace!("  DOUTEN: {}", self.data_out_enabled);
    }

    fn write_ctrl0(&mut self, value: u8) {
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

    fn write_ctrl1(&mut self, value: u8) {
        self.subheader_data_enabled = value.bit(0);
        log::trace!("  SHDREN: {}", self.subheader_data_enabled);
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

    pub fn data_ready(&self) -> bool {
        self.data_transfer_in_progress
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
                self.block_pointer =
                    (self.block_pointer + DATA_TRACK_HEADER_LEN) & BUFFER_RAM_ADDRESS_MASK;

                self.decoded_first_written_block = true;
            }

            log::trace!(
                "Performed decoder write; write address = {:04X}, block pointer = {:04X}",
                self.write_address,
                self.block_pointer
            );
        }
    }

    pub fn clock(
        &mut self,
        word_ram: &mut WordRam,
        prg_ram: &mut [u8; memory::PRG_RAM_LEN],
        pcm: &mut Rf5c164,
    ) {
        if self.data_transfer_in_progress && self.device_destination.is_dma() {
            self.progress_dma(word_ram, prg_ram, pcm);
        }
    }

    fn progress_dma(
        &mut self,
        word_ram: &mut WordRam,
        prg_ram: &mut [u8; memory::PRG_RAM_LEN],
        pcm: &mut Rf5c164,
    ) {
        let dma_address_mask = match self.device_destination {
            // All 19 bits of DMA address are used for PRG RAM
            DeviceDestination::PrgRam => (1 << 19) - 1,
            // DMA address is 18 bits in 2M mode (256KB), 17 bits in 1M mode (128KB)
            DeviceDestination::WordRam => match word_ram.mode() {
                WordRamMode::TwoM => (1 << 18) - 1,
                WordRamMode::OneM => (1 << 17) - 1,
            },
            // PCM address is 13 bits in the register, but it's effectively a 12-bit address
            DeviceDestination::Pcm => (1 << 12) - 1,
            _ => panic!("Invalid DMA destination: {:?}", self.device_destination),
        };

        // PCM DMA confusingly shifts the effective address bits down by 1, treating the register
        // as A11-A2 instead of A12-A3
        let dma_address_shift = match self.device_destination {
            DeviceDestination::Pcm => 1,
            DeviceDestination::PrgRam | DeviceDestination::WordRam => 0,
            _ => panic!("Invalid DMA destination: {:?}", self.device_destination),
        };

        let mut dma_address = (self.dma_address >> dma_address_shift) & dma_address_mask;

        log::trace!(
            "Progressing DMA transfer to {:?} starting at {dma_address:06X}; {} bytes remaining",
            self.device_destination,
            self.data_byte_counter + 1
        );

        // 128 is arbitrary; CDC DMA seems to always be in chunks of 2048 bytes so this will get the
        // DMA done fairly quickly
        for _ in 0..128 {
            let byte = self.buffer_ram[self.data_address_counter as usize];
            match self.device_destination {
                DeviceDestination::PrgRam => {
                    prg_ram[dma_address as usize] = byte;
                }
                DeviceDestination::WordRam => {
                    word_ram.dma_write(dma_address, byte);
                }
                DeviceDestination::Pcm => {
                    pcm.dma_write(dma_address, byte);
                }
                _ => unreachable!("device destination checked earlier in the function"),
            }

            self.data_address_counter = (self.data_address_counter + 1) & BUFFER_RAM_ADDRESS_MASK;
            dma_address = (dma_address + 1) & dma_address_mask;

            let (new_byte_counter, overflowed) = self.data_byte_counter.overflowing_sub(1);
            self.data_byte_counter = new_byte_counter;
            if overflowed {
                log::trace!("DMA transfer complete");

                self.data_transfer_in_progress = false;
                self.transfer_end_interrupt_pending = true;

                break;
            }
        }

        self.dma_address = dma_address << dma_address_shift;
    }

    pub fn reset(&mut self) {
        // Clear all values from IFCTRL, CTRL0, and CTRL1, as well as interrupt flags
        self.write_ifctrl(0x00);
        self.write_ctrl0(0x00);
        self.write_ctrl1(0x00);
        self.transfer_end_interrupt_pending = false;
        self.decoder_interrupt_pending = false;
    }
}
