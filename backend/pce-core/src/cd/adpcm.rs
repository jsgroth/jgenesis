//! Code for emulating the CD-ROM² add-on's ADPCM sound chip
//!
//! The core decoder is an MSM5205 but there's a lot of additional hardware around it

mod msm5205;

use crate::api;
use crate::api::PceEmulatorConfig;
use crate::cd::adpcm::msm5205::Msm5205;
use crate::cd::scsi::ScsiCdDrive;
use crate::cd::{CdEvent, CdInterruptFlags, CdInterruptType, CdRomController};
use bincode::{Decode, Encode};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_common::num::{GetBit, U16Ext};
use std::collections::VecDeque;

// Nominally 32000 Hz, but runs slightly faster in actual hardware according to:
//   https://www.ysutopia.net/special/MSM5205.htm
// CD-ROM² has a 1.540200 MHz oscillator that gets divided by 48 for an ADPCM frequency of 32087.5 Hz
#[allow(clippy::inconsistent_digit_grouping)]
const ADPCM_FREQUENCY_SCALED: u64 = 32087_5;
const MCLK_FREQUENCY_SCALED: u64 = 10 * (api::MASTER_CLOCK_FREQUENCY as u64);

pub const ADPCM_SAMPLE_RATE: f64 = (ADPCM_FREQUENCY_SCALED as f64) / 10.0;

const MSM5205_MAX_DIVIDER: u8 = 16;

const ADPCM_RAM_LEN: usize = 64 * 1024;

// TODO validate these timings; these values are just copied from Mednafen
const READ_MCLK_CYCLES: u64 = 19 * 3;
const WRITE_MCLK_CYCLES: u64 = 11 * 3;

// Based on tcd-verificator DMA timing tests
const DMA_WRITE_MCLK_CYCLES: u64 = 25 * 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct AdpcmControl(u8);

impl AdpcmControl {
    fn reset(self) -> bool {
        self.0.bit(7)
    }

    fn terminate_at_end(self) -> bool {
        self.0.bit(6)
    }

    fn play(self) -> bool {
        self.0.bit(5)
    }

    fn latch_length(self) -> bool {
        self.0.bit(4)
    }

    fn latch_read_address(self) -> bool {
        self.0.bit(3)
    }

    fn decrement_read_address(self) -> bool {
        !self.0.bit(2)
    }

    fn latch_write_address(self) -> bool {
        self.0.bit(1)
    }

    fn decrement_write_address(self) -> bool {
        !self.0.bit(0)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AdpcmChip {
    ram: BoxedByteArray<ADPCM_RAM_LEN>,
    msm5205: Msm5205,
    msm5205_divider: u8,
    sample_rate_nibble: u8,
    sample_rate_byte: u8, // Highest 4 bits do nothing but are R/W
    address_buffer: u16,
    read_address: u16,
    write_address: u16,
    length: u32, // Per tcd-verificator ADPCM tests, length is a 17-bit value
    end_flag: bool,
    half_flag: bool,
    read_buffer: u8,
    write_buffer: u8,
    control: AdpcmControl,
    dma_control: u8,
    playing: bool,
    playing_odd_nibble: bool,
    playing_at_reset: bool,
    sample_buffer: u8,
    cycle_product_scaled: u64,
    output_samples: VecDeque<i16>,
}

impl AdpcmChip {
    pub fn new(config: &PceEmulatorConfig) -> Self {
        Self {
            ram: BoxedByteArray::new(),
            msm5205: Msm5205::new(config.quantize_adpcm_output),
            msm5205_divider: MSM5205_MAX_DIVIDER,
            sample_rate_nibble: 0,
            sample_rate_byte: 0,
            address_buffer: 0,
            read_address: 0,
            write_address: 0,
            length: 0,
            end_flag: false,
            half_flag: false,
            read_buffer: 0,
            write_buffer: 0,
            control: AdpcmControl(0),
            dma_control: 0,
            playing: false,
            playing_odd_nibble: false,
            playing_at_reset: false,
            sample_buffer: 0,
            cycle_product_scaled: 0,
            output_samples: VecDeque::with_capacity((ADPCM_SAMPLE_RATE as usize) / 60),
        }
    }

    fn update_irq_flags(&self, irqs_pending: &mut CdInterruptFlags) {
        irqs_pending.update(CdInterruptType::AdpcmHalf, self.half_flag);
        irqs_pending.update(CdInterruptType::AdpcmEnd, self.end_flag);
    }

    // $1808
    pub fn write_address_low(&mut self, value: u8) {
        self.write_address::<true>(value);
    }

    // $1809
    pub fn write_address_high(&mut self, value: u8) {
        self.write_address::<false>(value);
    }

    fn write_address<const LOW: bool>(&mut self, value: u8) {
        if LOW {
            self.address_buffer.set_lsb(value);
        } else {
            self.address_buffer.set_msb(value);
        }

        // Length is constantly latched if control bit 4 = 1, unlike read/write addresses
        if self.control.latch_length() {
            self.length = self.address_buffer.into();
        }

        log::trace!(
            "Address buffer ({} write): {:04X}",
            if LOW { "lsb" } else { "msb" },
            self.address_buffer
        );
    }

    // $180B
    pub fn write_dma(&mut self, value: u8, scsi: &ScsiCdDrive) {
        // It seems like only bits 0 and 1 are meaningful, and either can enable ADPCM DMA
        // If bit 1 = 1, DMA is always enabled
        // If bit 0 = 1, DMA is enabled until the SCSI bus is no longer in DATA IN phase
        self.dma_control = value;

        // Bit 0 is immediately cleared if bus is not in DATA IN phase
        if !scsi.signals().data_in_phase() {
            self.dma_control &= !1;
        }

        log::debug!("DMA control: {value:02X}");
        log::debug!("  DMA always active: {}", value.bit(1));
        log::debug!("  DMA active during DATA IN: {}", value.bit(0));
    }

    // $180D
    pub fn write_control(&mut self, value: u8) {
        let prev_control = self.control;
        self.control = AdpcmControl(value);

        self.playing &= self.control.play();
        if !self.playing && self.control.play() {
            self.playing = true;
            self.playing_odd_nibble = false;
            self.msm5205.decode_start();
        }

        if self.control.reset() {
            self.reset();
            log::debug!("ADPCM reset");
            return;
        }

        if self.control.latch_length() {
            self.length = self.address_buffer.into();
            self.end_flag = false;
        }

        if !prev_control.latch_read_address() && self.control.latch_read_address() {
            self.read_address = self.address_buffer;
            if self.control.decrement_read_address() {
                self.read_address = self.read_address.wrapping_sub(1);
            }
        }

        if !prev_control.latch_write_address() && self.control.latch_write_address() {
            self.write_address = self.address_buffer;
            if self.control.decrement_write_address() {
                self.write_address = self.write_address.wrapping_sub(1);
            }
        }

        log::debug!("ADPCM control: {value:02X}");
        log::debug!("  ADPCM play: {}", self.control.play());
        log::debug!("  Terminate at end flag: {}", self.control.terminate_at_end());
        log::debug!("  Latch length: {}", self.control.latch_length());
        log::debug!("  ADPCM read address: {:04X}", self.read_address);
        log::debug!("  Decrement read address: {}", self.control.decrement_read_address());
        log::debug!("  ADPCM write address: {:04X}", self.write_address);
        log::debug!("  Decrement write address: {}", self.control.decrement_write_address());
    }

    fn reset(&mut self) {
        self.address_buffer = 0;
        self.read_address = 0;
        self.write_address = 0;
        self.length = 0;
        self.half_flag = false;
        self.end_flag = false;
        self.read_buffer = 0;
        self.write_buffer = 0;

        self.playing_at_reset = self.playing;
        self.playing = false;
    }

    // $180E
    pub fn write_sample_rate(&mut self, value: u8) {
        self.sample_rate_nibble = value & 0xF;
        self.sample_rate_byte = value;

        log::debug!("ADPCM sample rate: {:02X}", self.sample_rate_byte);
        log::debug!(
            "  ADPCM frequency: {} Hz",
            ADPCM_SAMPLE_RATE / (16.0 - f64::from(self.sample_rate_nibble))
        );
    }

    fn clock(&mut self) {
        self.msm5205_divider -= 1;
        if self.msm5205_divider <= self.sample_rate_nibble {
            self.msm5205_divider = MSM5205_MAX_DIVIDER;

            if self.playing {
                self.clock_msm5205();
            }
        }

        self.output_samples.push_back(if self.playing { self.msm5205.current_sample() } else { 0 });
    }

    fn clock_msm5205(&mut self) {
        if !self.playing_odd_nibble {
            self.half_flag = self.length < 0x8000;
            self.end_flag |= !self.control.latch_length() && self.length == 0;

            if self.end_flag && self.control.terminate_at_end() {
                self.playing = false;
                return;
            }

            self.sample_buffer = self.ram[self.read_address as usize];
            self.read_address = self.read_address.wrapping_add(1);

            if !self.control.latch_length() {
                self.length = self.length.wrapping_sub(1) & 0x1FFFF;
            }
        }

        let nibble = if !self.playing_odd_nibble {
            self.sample_buffer >> 4
        } else {
            self.sample_buffer & 0xF
        };
        self.msm5205.decode_nibble(nibble);

        self.playing_odd_nibble = !self.playing_odd_nibble;
    }

    // Signed 12-bit samples, 32087.5 Hz sample rate
    pub fn drain_audio_samples(&mut self) -> impl Iterator<Item = i16> {
        self.output_samples.drain(..)
    }

    pub fn reload_config(&mut self, config: &PceEmulatorConfig) {
        self.msm5205.set_quantize_output(config.quantize_adpcm_output);
    }
}

impl CdRomController {
    pub(super) fn read_adpcm_register(&mut self, address: u32) -> u8 {
        let value = match address & 0x3FF {
            0xA => self.read_adpcm_ram_port(),
            0xB => self.adpcm.dma_control,
            0xC => self.read_adpcm_status(),
            0xD => self.adpcm.control.0,
            0xE => self.adpcm.sample_rate_byte,
            _ => {
                log::warn!("Unhandled ADPCM register read: {:04X}", address & 0x1FFF);
                0
            }
        };

        self.adpcm.update_irq_flags(&mut self.irqs_pending);

        value
    }

    // $180A
    fn read_adpcm_ram_port(&mut self) -> u8 {
        // RAM port reads queue a read and return current read buffer contents
        self.trigger_event_after(CdEvent::AdpcmRamRead, READ_MCLK_CYCLES);
        self.adpcm.read_buffer
    }

    // $180C
    fn read_adpcm_status(&self) -> u8 {
        let read_pending = self.is_event_pending(CdEvent::AdpcmRamRead);
        let write_pending = self.is_event_pending(CdEvent::AdpcmRamWrite);
        let playing = if self.adpcm.control.reset() {
            self.adpcm.playing_at_reset
        } else {
            self.adpcm.playing
        };

        (u8::from(read_pending) << 7)
            | (u8::from(playing) << 3)
            | (u8::from(write_pending) << 2)
            | u8::from(self.adpcm.end_flag)
    }

    pub(super) fn write_adpcm_register(&mut self, address: u32, value: u8) {
        if self.adpcm.control.reset() && address & 0x3FF != 0xD {
            // TODO are any other registers writable while reset bit is set?
            log::warn!(
                "Write to non-control ADPCM register {:04X} while reset bit is set: {value:02X}",
                address & 0x1FFF
            );
            return;
        }

        match address & 0x3FF {
            0x8 => self.adpcm.write_address_low(value),
            0x9 => self.adpcm.write_address_high(value),
            0xA => self.write_adpcm_ram_port(value),
            0xB => self.adpcm.write_dma(value, &self.scsi),
            0xD => self.adpcm.write_control(value),
            0xE => self.adpcm.write_sample_rate(value),
            _ => {
                log::warn!("Unhandled ADPCM register write: {:04X}", address & 0x1FFF);
            }
        }

        if self.adpcm.control.reset() {
            self.remove_event(CdEvent::AdpcmRamRead);
            self.remove_event(CdEvent::AdpcmRamWrite);
        }

        self.adpcm.update_irq_flags(&mut self.irqs_pending);
    }

    // $180A
    fn write_adpcm_ram_port(&mut self, value: u8) {
        // TODO what happens if a write is already in progress?
        self.adpcm.write_buffer = value;
        self.trigger_event_after(CdEvent::AdpcmRamWrite, WRITE_MCLK_CYCLES);
    }

    pub(super) fn process_adpcm_ram_read(&mut self) {
        self.adpcm.read_buffer = self.adpcm.ram[self.adpcm.read_address as usize];
        self.adpcm.read_address = self.adpcm.read_address.wrapping_add(1);

        let mut force_half_off = false;
        if !self.adpcm.control.latch_length() {
            if self.adpcm.length != 0 {
                self.adpcm.length -= 1;
            } else {
                self.adpcm.end_flag = true;
                force_half_off = true;
            }
        }

        // Reads set half flag after length decrement
        self.adpcm.half_flag = self.adpcm.length < 0x8000 && !force_half_off;

        self.adpcm.update_irq_flags(&mut self.irqs_pending);
    }

    pub(super) fn process_adpcm_ram_write(&mut self, cycles: u64) {
        self.adpcm.ram[self.adpcm.write_address as usize] = self.adpcm.write_buffer;
        self.adpcm.write_address = self.adpcm.write_address.wrapping_add(1);

        // Writes set half flag before length increment
        self.adpcm.half_flag = self.adpcm.length < 0x8000;

        if !self.adpcm.control.latch_length() {
            self.adpcm.end_flag |= self.adpcm.length == 0;
            self.adpcm.length = (self.adpcm.length + 1) & 0x1FFFF;
        }

        self.try_progress_adpcm_dma(cycles);

        self.adpcm.update_irq_flags(&mut self.irqs_pending);
    }

    #[allow(clippy::nonminimal_bool)] // I think the suggestion makes the logic less readable
    pub(super) fn try_progress_adpcm_dma(&mut self, cycles: u64) {
        if self.adpcm.dma_control & 3 == 0 {
            // DMA is not enabled
            return;
        }

        let signals = self.scsi.signals();
        if !signals.data_in_phase() {
            // DMA control bit 0 is automatically cleared when DATA IN phase ends (disables DMA if
            // bit 1 is not also set)
            self.adpcm.dma_control &= !1;

            // Not in DATA IN phase, no data available
            return;
        }

        if self.is_event_pending(CdEvent::AdpcmRamWrite) {
            // ADPCM RAM write already in progress
            return;
        }

        if !(signals.req && !signals.ack) {
            // Next byte is not ready to read
            return;
        }

        // Byte is ready and no RAM write is pending, do the SCSI handshake
        self.adpcm.write_buffer = self.scsi.data_bus();
        self.trigger_event_at(CdEvent::AdpcmRamWrite, cycles + DMA_WRITE_MCLK_CYCLES);

        self.scsi.dma_ack_handshake(&mut self.irqs_pending);
    }

    pub(super) fn tick_adpcm(&mut self, elapsed_mclk: u64) {
        self.adpcm.cycle_product_scaled += ADPCM_FREQUENCY_SCALED * elapsed_mclk;
        while self.adpcm.cycle_product_scaled >= MCLK_FREQUENCY_SCALED {
            self.adpcm.cycle_product_scaled -= MCLK_FREQUENCY_SCALED;
            self.adpcm.clock();

            self.adpcm.update_irq_flags(&mut self.irqs_pending);
        }

        self.try_progress_adpcm_dma(self.cycle_counter);
    }

    pub fn debug_adpcm_ram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.adpcm.ram.as_mut_slice())
    }
}
