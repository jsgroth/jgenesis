//! Code for emulating the CD-ROM² add-on
//!
//! Much of the details on register and hardware behaviors are based on referencing Mednafen and
//! Mesen (including the protocols for the SCSI drive's vendor-specific commands), because I could
//! not find any documentation that describes the low-level behaviors in enough detail to be able to
//! emulate it

mod adpcm;
mod scsi;

use crate::api;
use crate::audio::PceAudioResampler;
use crate::cd::adpcm::AdpcmChip;
use crate::cd::scsi::ScsiCdDrive;
pub use adpcm::ADPCM_SAMPLE_RATE;
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::reader::CdRom;
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{EnumAll, PartialClone};
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

// CD-ROM² unit always has 64KB of working RAM; any additional RAM is part of the System Card
const WORKING_RAM_LEN: usize = 64 * 1024;
const BACKUP_RAM_LEN: usize = 2 * 1024;

const SCSI_DATA_IN_IRQ_BIT: u8 = 6;
const SCSI_STATUS_IRQ_BIT: u8 = 5;
const SUBCHANNEL_IRQ_BIT: u8 = 4;
const ADPCM_END_IRQ_BIT: u8 = 3;
const ADPCM_HALF_IRQ_BIT: u8 = 2;

const ALL_IRQ_BITS: u8 = (1 << SCSI_DATA_IN_IRQ_BIT)
    | (1 << SCSI_STATUS_IRQ_BIT)
    | (1 << SUBCHANNEL_IRQ_BIT)
    | (1 << ADPCM_END_IRQ_BIT)
    | (1 << ADPCM_HALF_IRQ_BIT);

// TODO validate this timing; this is copied from Mednafen
const ACK_CLEAR_MCLK_CYCLES: u64 = 15 * 3;

define_bit_enum!(AudioChannel, [Right, Left]);

impl AudioChannel {
    #[must_use]
    fn other(self) -> Self {
        match self {
            Self::Right => Self::Left,
            Self::Left => Self::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdInterruptType {
    ScsiDataIn,
    ScsiStatus,
    Subchannel,
    AdpcmEnd,
    AdpcmHalf,
}

impl CdInterruptType {
    const fn bit(self) -> u8 {
        match self {
            Self::ScsiDataIn => SCSI_DATA_IN_IRQ_BIT,
            Self::ScsiStatus => SCSI_STATUS_IRQ_BIT,
            Self::Subchannel => SUBCHANNEL_IRQ_BIT,
            Self::AdpcmEnd => ADPCM_END_IRQ_BIT,
            Self::AdpcmHalf => ADPCM_HALF_IRQ_BIT,
        }
    }

    const fn mask(self) -> u8 {
        1 << self.bit()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct CdInterruptFlags(u8);

impl CdInterruptFlags {
    fn set(&mut self, interrupt: CdInterruptType) {
        self.0 |= interrupt.mask();
    }

    fn clear(&mut self, interrupt: CdInterruptType) {
        self.0 &= !interrupt.mask();
    }

    fn update(&mut self, interrupt: CdInterruptType, value: bool) {
        self.0 = (self.0 & !interrupt.mask()) | (u8::from(value) << interrupt.bit());
    }
}

define_bit_enum!(FadeTarget, [CdDa, Adpcm]);
define_bit_enum!(FadeSpeed, [Slow, Fast]);

#[derive(Debug, Clone, Encode, Decode)]
struct Fader {
    target: FadeTarget,
    speed: FadeSpeed,
    multiplier: f64,
    enabled: bool,
    last_control_write: u8, // Only bits 1-3 do anything but all bits are R/W
}

impl Fader {
    fn new() -> Self {
        Self {
            target: FadeTarget::default(),
            speed: FadeSpeed::default(),
            multiplier: 1.0,
            enabled: false,
            last_control_write: 0,
        }
    }

    fn read(&self) -> u8 {
        self.last_control_write
    }

    fn write(&mut self, value: u8) {
        self.enabled = value.bit(3);
        self.speed = FadeSpeed::from_bit(value.bit(2));
        self.target = FadeTarget::from_bit(value.bit(1));
        self.last_control_write = value;

        if !self.enabled {
            self.multiplier = 1.0;
        }

        log::debug!("Fader write: {value:02X}");
        log::debug!("  Fader enabled: {}", self.enabled);
        log::debug!("  Fade speed: {:?}", self.speed);
        log::debug!("  Fade target: {:?}", self.target);
    }

    fn tick(&mut self, elapsed_mclk: u64) {
        if !self.enabled || self.multiplier <= 0.0 {
            return;
        }

        // TODO the fade is absolutely not this smooth in actual hardware, but unclear how exactly
        // it works beyond the approximate duration

        let fade_length_secs = match self.speed {
            FadeSpeed::Slow => 6.0,
            FadeSpeed::Fast => 2.5,
        };
        let fade_delta = (elapsed_mclk as f64) / (fade_length_secs * api::MASTER_CLOCK_FREQUENCY);

        self.multiplier = (self.multiplier - fade_delta).clamp(0.0, 1.0);
    }

    fn apply_cd_da(&self, sample: [f64; 2]) -> [f64; 2] {
        if self.target != FadeTarget::CdDa {
            return sample;
        }

        sample.map(|sample| sample * self.multiplier)
    }

    fn apply_adpcm(&self, sample: f64) -> f64 {
        if self.target != FadeTarget::Adpcm {
            return sample;
        }

        sample * self.multiplier
    }
}

// Event "ordering" is arbitrary, just ensures that processing order is consistent if two events
// have the same time
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Encode, Decode, EnumAll)]
enum CdEvent {
    AckAutoClear,
    AdpcmRamRead,
    AdpcmRamWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct CdEventWithTime(CdEvent, u64);

impl PartialOrd for CdEventWithTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CdEventWithTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.1.cmp(&other.1).then(self.0.cmp(&other.0))
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct CdRomController {
    #[partial_clone(partial)]
    scsi: ScsiCdDrive,
    adpcm: AdpcmChip,
    fader: Fader,
    working_ram: BoxedByteArray<WORKING_RAM_LEN>,
    backup_ram: BoxedByteArray<BACKUP_RAM_LEN>,
    backup_ram_dirty: bool,
    backup_ram_enabled: bool,
    cd_da_read_channel: AudioChannel,
    cd_da_read_sample: i16,
    irqs_enabled: u8,
    irqs_pending: CdInterruptFlags,
    cycle_counter: u64,
    events: BinaryHeap<Reverse<CdEventWithTime>>,
    last_1804_write: u8, // Only bit 1 is meaningful (SCSI RST) but all bits are R/W
}

impl CdRomController {
    pub fn new(disc: Option<CdRom>, initial_backup_ram: Option<Vec<u8>>) -> Self {
        let mut backup_ram = BoxedByteArray::new();

        match initial_backup_ram {
            Some(initial_backup_ram) if initial_backup_ram.len() >= BACKUP_RAM_LEN => {
                backup_ram.copy_from_slice(&initial_backup_ram[..BACKUP_RAM_LEN]);
            }
            _ => {
                // Freshly formatted backup RAM contains this 8-byte sequence followed by all 0s
                // TODO does this vary by System Card?
                backup_ram[..8].copy_from_slice(&[0x48, 0x55, 0x42, 0x4D, 0x00, 0x88, 0x10, 0x80]);
            }
        }

        Self {
            scsi: ScsiCdDrive::new(disc),
            adpcm: AdpcmChip::new(),
            fader: Fader::new(),
            working_ram: BoxedByteArray::new(),
            backup_ram,
            backup_ram_dirty: false,
            backup_ram_enabled: false,
            cd_da_read_channel: AudioChannel::Right,
            cd_da_read_sample: 0,
            irqs_enabled: 0,
            irqs_pending: CdInterruptFlags(0),
            cycle_counter: 0,
            events: BinaryHeap::with_capacity(CdEvent::ALL.len()),
            last_1804_write: 0,
        }
    }

    // Pages $80-$87 ($100000-$10FFFF)
    pub fn read_working_ram(&self, address: u32) -> u8 {
        self.working_ram[(address & 0xFFFF) as usize]
    }

    // Pages $80-$87 ($100000-$10FFFF)
    pub fn write_working_ram(&mut self, address: u32, value: u8) {
        self.working_ram[(address & 0xFFFF) as usize] = value;
    }

    // Page $F7 ($1EE000-$1EFFFF)
    pub fn read_backup_ram(&self, address: u32) -> u8 {
        if !self.backup_ram_enabled {
            return 0xFF;
        }

        // Backup RAM is not mirrored
        self.backup_ram.get((address & 0x1FFF) as usize).copied().unwrap_or(0xFF)
    }

    // Page $F7 ($1EE000-$1EFFFF)
    pub fn write_backup_ram(&mut self, address: u32, value: u8) {
        if !self.backup_ram_enabled {
            return;
        }

        // Backup RAM is not mirrored
        let Some(ram_value) = self.backup_ram.get_mut((address & 0x1FFF) as usize) else { return };

        *ram_value = value;
        self.backup_ram_dirty = true;
    }

    // $1800-$1BFF in page $FF
    #[allow(clippy::match_same_arms)]
    pub fn read_register(&mut self, address: u32, irq2_pending: &mut bool) -> u8 {
        log::trace!("CD-ROM register read: {:04X}", address & 0x1FFF);

        let value = match address & 0x3FF {
            0x0 => {
                // SCSI bus signals (read-only)
                let signals = self.scsi.signals();
                (u8::from(signals.bsy) << 7)
                    | (u8::from(signals.req) << 6)
                    | (u8::from(signals.msg) << 5)
                    | (u8::from(signals.c_d) << 4)
                    | (u8::from(signals.i_o) << 3)
            }
            0x1 => {
                // SCSI data bus
                self.scsi.data_bus()
            }
            0x2 => {
                // SCSI ACK signal and IRQ enabled flags
                (u8::from(self.scsi.signals().ack) << 7) | self.irqs_enabled
            }
            0x3 => {
                // IRQ pending flags and current CD-DA read channel for $1805/$1806
                // Reading from this register locks backup RAM
                self.backup_ram_enabled = false;

                self.irqs_pending.0 | ((self.cd_da_read_channel as u8) << 1)
            }
            0x4 => {
                // SCSI RST signal
                self.last_1804_write
            }
            0x5 => {
                // CD-DA sample, LSB (read-only)
                self.cd_da_read_sample.to_le_bytes()[0]
            }
            0x6 => {
                // CD-DA sample, MSB (read-only)
                self.cd_da_read_sample.to_le_bytes()[1]
            }
            0x7 => {
                // Read subchannel
                // TODO
                log::warn!("Subchannel FIFO read ($1807); not implemented");
                0xFF
            }
            0x8 => {
                // SCSI data bus, and automatically progress REQ/ACK handshake if one is in progress
                // in a DATA IN phase
                let value = self.scsi.data_bus();

                let signals = self.scsi.signals();
                if signals.data_in_phase() && signals.req && !signals.ack {
                    self.scsi.set_ack(true, &mut self.irqs_pending);
                    self.trigger_event_after(CdEvent::AckAutoClear, ACK_CLEAR_MCLK_CYCLES);
                }

                value
            }
            0xA..=0xE => self.read_adpcm_register(address),
            0xF => self.fader.read(),
            0x0C0..=0x0C7 => {
                // Super CD-ROM² / Super System Card hardware version
                // If cartridge didn't respond, act like no Super hardware is present
                0xFF
            }
            0x200..=0x2FF => {
                // Arcade Card registers/version
                // If cartridge didn't respond, act like no Arcade Card is present
                0xFF
            }
            _ => {
                log::warn!("Unhandled CD-ROM register read {:04X}", address & 0x1FFF);
                0xFF
            }
        };

        log::trace!("  Returning {value:02X}");

        *irq2_pending = self.irq();

        value
    }

    // $1800-$1BFF in page $FF
    #[allow(clippy::match_same_arms)]
    pub fn write_register(&mut self, address: u32, value: u8, irq2_pending: &mut bool) {
        log::trace!("CD-ROM register write: {:04X} {value:02X}", address & 0x1FFF);

        match address & 0x3FF {
            0x0 => {
                // Unclear exactly what the bits in this register mean, but when the BIOS writes to
                // it, it expects the SCSI bus to transition from BUS FREE to COMMAND phase
                self.scsi.set_sel(true, &mut self.irqs_pending);
                self.scsi.set_sel(false, &mut self.irqs_pending);

                log::trace!("  SEL=1");
            }
            0x1 => {
                // SCSI data bus
                self.scsi.set_data_bus(value);
                log::trace!("  SCSI data bus: {value:02X}");
            }
            0x2 => {
                // SCSI ACK signal and IRQ enabled flags
                self.scsi.set_ack(value.bit(7), &mut self.irqs_pending);
                self.irqs_enabled = value & 0x7F;

                log::trace!("IRQ enabled / ACK: {value:02X}");
                log::trace!("  ACK={}", u8::from(self.scsi.signals().ack));
                log::trace!(
                    "  SCSI STATUS IRQ enabled: {}",
                    self.irqs_enabled.bit(SCSI_STATUS_IRQ_BIT)
                );
                log::trace!(
                    "  SCSI DATA IN IRQ enabled: {}",
                    self.irqs_enabled.bit(SCSI_DATA_IN_IRQ_BIT)
                );
                log::trace!(
                    "  Subchannel IRQ enabled: {}",
                    self.irqs_enabled.bit(SUBCHANNEL_IRQ_BIT)
                );
                log::trace!(
                    "  ADPCM end IRQ enabled: {}",
                    self.irqs_enabled.bit(ADPCM_END_IRQ_BIT)
                );
                log::trace!(
                    "  ADPCM half IRQ enabled: {}",
                    self.irqs_enabled.bit(ADPCM_HALF_IRQ_BIT)
                );
            }
            0x4 => {
                // SCSI RST signal
                self.scsi.set_rst(value.bit(1), &mut self.irqs_pending);
                self.last_1804_write = value;

                log::debug!("RST={}", u8::from(self.scsi.signals().rst));
            }
            0x5 => {
                // Writing to this register updates the readable CD-DA sample; value is ignored
                self.cd_da_read_channel = self.cd_da_read_channel.other();

                let current_sample = self.scsi.current_audio_sample();
                // TODO channels might be backwards relative to the readable bit in $1803
                self.cd_da_read_sample = match self.cd_da_read_channel {
                    AudioChannel::Left => current_sample.0,
                    AudioChannel::Right => current_sample.1,
                };
            }
            0x7 => {
                // Enable/disable backup RAM
                self.backup_ram_enabled = value.bit(7);
                log::debug!("Backup RAM enabled: {}", self.backup_ram_enabled);
            }
            0x8..=0xE => {
                self.write_adpcm_register(address, value);
            }
            0xF => {
                // Fader control
                self.fader.write(value);
            }
            0x0C0..=0x0C7 => {
                // Super CD-ROM² / Super System Card hardware version
                // Super System Card BIOS writes to these addresses during hardware detection for
                // some reason
            }
            0x200..=0x2FF => {
                // Arcade Card registers; ignore (assume cartridge responded if Arcade Card is present)
            }
            _ => {
                log::warn!("Unhandled CD-ROM register write {:04X} {value:02X}", address & 0x1FFF);
            }
        }

        *irq2_pending = self.irq();
    }

    pub fn irq(&self) -> bool {
        self.irqs_enabled & self.irqs_pending.0 & ALL_IRQ_BITS != 0
    }

    fn is_event_pending(&self, event: CdEvent) -> bool {
        self.events.iter().any(|&Reverse(CdEventWithTime(heap_event, ..))| heap_event == event)
    }

    fn trigger_event_at(&mut self, event: CdEvent, cycles: u64) {
        if self.is_event_pending(event) {
            return;
        }

        self.events.push(Reverse(CdEventWithTime(event, cycles)));
    }

    fn trigger_event_after(&mut self, event: CdEvent, cycles: u64) {
        self.trigger_event_at(event, self.cycle_counter + cycles);
    }

    pub fn step_to(
        &mut self,
        cycle_counter: u64,
        irq2_pending: &mut bool,
    ) -> Result<(), CdRomError> {
        let elapsed_mclk = cycle_counter.saturating_sub(self.cycle_counter);
        if elapsed_mclk == 0 {
            return Ok(());
        }
        self.cycle_counter = cycle_counter;

        self.scsi.tick(elapsed_mclk, &mut self.irqs_pending)?;
        self.tick_adpcm(elapsed_mclk);
        self.fader.tick(elapsed_mclk);

        while let Some(&Reverse(CdEventWithTime(event, event_cycles))) = self.events.peek()
            && event_cycles >= self.cycle_counter
        {
            self.events.pop();

            match event {
                CdEvent::AckAutoClear => self.scsi.set_ack(false, &mut self.irqs_pending),
                CdEvent::AdpcmRamRead => self.process_adpcm_ram_read(),
                CdEvent::AdpcmRamWrite => self.process_adpcm_ram_write(event_cycles),
            }
        }

        *irq2_pending = self.irq();

        Ok(())
    }

    pub fn drain_audio_samples_into(&mut self, resampler: &mut PceAudioResampler) {
        const I16_TO_F64: f64 = 1.0 / -(i16::MIN as f64);
        const I12_TO_F64: f64 = 1.0 / ((1 << 11) as f64);

        for (sample_l, sample_r) in self.scsi.drain_audio_samples() {
            let sample = self
                .fader
                .apply_cd_da([f64::from(sample_l) * I16_TO_F64, f64::from(sample_r) * I16_TO_F64]);
            resampler.collect_cd_da(sample);
        }

        for sample in self.adpcm.drain_audio_samples() {
            let sample = self.fader.apply_adpcm(f64::from(sample) * I12_TO_F64);
            resampler.collect_adpcm(sample);
        }
    }

    pub fn backup_ram_dirty(&self) -> bool {
        self.backup_ram_dirty
    }

    pub fn clear_backup_ram_dirty(&mut self) {
        self.backup_ram_dirty = false;
    }

    pub fn backup_ram(&self) -> &[u8] {
        self.backup_ram.as_slice()
    }

    pub fn take_disc(&mut self) -> Option<CdRom> {
        self.scsi.take_disc()
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.scsi.take_disc_from(&mut other.scsi);
    }

    pub fn debug_working_ram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.working_ram.as_mut_slice())
    }
}
