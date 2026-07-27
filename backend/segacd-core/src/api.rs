//! Sega CD public interface and main loop

use crate::bus::SegaCdBus;
use crate::memory;
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::cdtime::CdTime;
use cdrom::reader::CdRom;
use genesis_components::cartridge::GenesisRegionExt;
use genesis_config::GenesisEmulatorConfig;
use genesis_config::GenesisRegion;
use jgenesis_common::frontend::{EmulatorTrait, PartialClone, TimingMode};
use jgenesis_common::num::U16Ext;
use m68000_emu::M68000;
use std::fmt::Debug;
use thiserror::Error;

pub const DEFAULT_SUB_CPU_DIVIDER: u64 = genesis_config::NATIVE_SUB_CPU_DIVIDER;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
pub const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

const BIOS_LEN: usize = memory::BIOS_LEN;

#[derive(Debug, Error)]
pub enum SegaCdLoadError {
    #[error("BIOS is required for Sega CD emulation")]
    MissingBios,
    #[error("BIOS must be {BIOS_LEN} bytes, was {bios_len} bytes")]
    InvalidBios { bios_len: usize },
    #[error("CD-ROM-related error: {0}")]
    CdRom(#[from] CdRomError),
}

pub type SegaCdLoadResult<T> = Result<T, SegaCdLoadError>;

pub trait SegaCdAudioOutput {
    fn collect_pcm(&mut self, sample: (f64, f64));

    fn collect_cd(&mut self, sample: (f64, f64));
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct SegaCd {
    sub_cpu: M68000,
    #[partial_clone(partial)]
    bus: SegaCdBus,
    disc_title: Option<String>,
    six_button_incompatible_game: bool,
    timing_mode: TimingMode,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycle_product: u64,
    sub_cpu_divider: u64,
    sub_cpu_wait_cycles: u64,
    config: GenesisEmulatorConfig,
}

impl SegaCd {
    pub fn new(
        bios: Vec<u8>,
        disc: Option<CdRom>,
        timing_mode: TimingMode,
        initial_backup_ram: Option<Vec<u8>>,
        initial_ram_cartridge: Option<Vec<u8>>,
        config: &GenesisEmulatorConfig,
    ) -> SegaCdLoadResult<Self> {
        if bios.len() != memory::BIOS_LEN {
            return Err(SegaCdLoadError::InvalidBios { bios_len: bios.len() });
        }

        let mut bus =
            SegaCdBus::new(bios, disc, initial_backup_ram, initial_ram_cartridge, config)?;

        let disc_title = bus.disc_title()?;
        let six_button_incompatible_game = bus.has_six_button_incompatible_game()?;

        let sub_cpu = M68000::builder().allow_tas_writes(true).name("Sub".into()).build();
        let sub_cpu_divider = config.sega_cd.sub_cpu_divider.get();

        Ok(Self {
            sub_cpu,
            bus,
            timing_mode,
            disc_title,
            six_button_incompatible_game,
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycle_product: 0,
            sub_cpu_divider,
            sub_cpu_wait_cycles: 0,
            config: config.clone(),
        })
    }

    pub fn tick(
        &mut self,
        genesis_mclk_elapsed: u64,
        audio_output: &mut impl SegaCdAudioOutput,
    ) -> SegaCdLoadResult<()> {
        self.sega_cd_mclk_cycle_product += genesis_mclk_elapsed * SEGA_CD_MASTER_CLOCK_RATE;
        let scd_mclk_elapsed = match self.timing_mode {
            TimingMode::Ntsc => {
                let elapsed = self.sega_cd_mclk_cycle_product / NTSC_GENESIS_MASTER_CLOCK_RATE;
                self.sega_cd_mclk_cycle_product -= elapsed * NTSC_GENESIS_MASTER_CLOCK_RATE;
                elapsed
            }
            TimingMode::Pal => {
                let elapsed = self.sega_cd_mclk_cycle_product / PAL_GENESIS_MASTER_CLOCK_RATE;
                self.sega_cd_mclk_cycle_product -= elapsed * PAL_GENESIS_MASTER_CLOCK_RATE;
                elapsed
            }
        };

        let prev_scd_mclk_cycles = self.sega_cd_mclk_cycles;
        self.sega_cd_mclk_cycles += scd_mclk_elapsed;

        let pcm_cycles = self.sega_cd_mclk_cycles / DEFAULT_SUB_CPU_DIVIDER
            - prev_scd_mclk_cycles / DEFAULT_SUB_CPU_DIVIDER;
        let elapsed_scd_mclk_cycles = self.sega_cd_mclk_cycles - prev_scd_mclk_cycles;

        // This match seems silly, but it avoids doing an integer division for the common dividers
        // of 1-4. Dividers higher than 4 can only be set via the CLI or by manually editing config
        // (and underclocking probably won't work well anyway)
        let sub_cpu_cycles = match self.sub_cpu_divider {
            DEFAULT_SUB_CPU_DIVIDER => pcm_cycles,
            3 => self.sega_cd_mclk_cycles / 3 - prev_scd_mclk_cycles / 3,
            2 => (self.sega_cd_mclk_cycles >> 1) - (prev_scd_mclk_cycles >> 1),
            1 => elapsed_scd_mclk_cycles,
            _ => {
                self.sega_cd_mclk_cycles / self.sub_cpu_divider
                    - prev_scd_mclk_cycles / self.sub_cpu_divider
            }
        };

        self.bus.tick_components(scd_mclk_elapsed, pcm_cycles, audio_output)?;

        self.tick_sub_cpu(sub_cpu_cycles);

        Ok(())
    }

    fn tick_sub_cpu(&mut self, mut sub_cpu_cycles: u64) {
        if self.bus.word_ram().sub_performed_blocked_access() {
            // If the sub CPU accesses word RAM while it's in 2M mode and owned by the main CPU, it
            // should halt until the main CPU writes DMNA=1 to transfer ownership to the sub CPU.
            // Marko's Magic Football depends on this or it will have glitched map graphics
            log::trace!("Not running sub CPU because word RAM writes are buffered");
            return;
        }

        while sub_cpu_cycles >= self.sub_cpu_wait_cycles {
            self.bus.flush_buffered_sub_writes();

            sub_cpu_cycles -= self.sub_cpu_wait_cycles;
            self.sub_cpu_wait_cycles = self.sub_cpu.execute_instruction(&mut self.bus).into();

            if self.bus.word_ram().sub_performed_blocked_access() {
                return;
            }
        }

        self.sub_cpu_wait_cycles -= sub_cpu_cycles;
    }

    pub fn main_read_memory<const WORD: bool>(&self, address: u32) -> u16 {
        if address & 0x200000 == 0 {
            // BIOS ROM / PRG RAM
            if WORD {
                let msb = self.bus.main_read_bios_prg_ram(address);
                let lsb = self.bus.main_read_bios_prg_ram(address + 1);
                u16::from_be_bytes([msb, lsb])
            } else {
                self.bus.main_read_bios_prg_ram(address).into()
            }
        } else {
            // Word RAM
            if WORD {
                let msb = self.bus.main_read_word_ram(address);
                let lsb = self.bus.main_read_word_ram(address + 1);
                u16::from_be_bytes([msb, lsb])
            } else {
                self.bus.main_read_word_ram(address).into()
            }
        }
    }

    pub fn main_write_memory<const WORD: bool>(&mut self, address: u32, value: u16) {
        if address & 0x200000 == 0 {
            // BIOS ROM / PRG RAM
            if WORD {
                self.bus.main_write_bios_prg_ram(address, value.msb());
                self.bus.main_write_bios_prg_ram(address + 1, value.lsb());
            } else {
                self.bus.main_write_bios_prg_ram(address, value as u8);
            }
        } else {
            // Word RAM
            if WORD {
                self.bus.main_write_word_ram(address, value.msb());
                self.bus.main_write_word_ram(address + 1, value.lsb());
            } else {
                self.bus.main_write_word_ram(address, value as u8);
            }
        }
    }

    pub fn read_word_for_dma(&mut self, address: u32, open_bus: &mut u16) -> u16 {
        if address & 0x200000 == 0 {
            *open_bus = self.main_read_memory::<true>(address);
            *open_bus
        } else {
            // Word RAM reads are delayed
            let prev_open_bus = *open_bus;
            *open_bus = self.main_read_memory::<true>(address);
            prev_open_bus
        }
    }

    pub fn read_ram_cartridge<const WORD: bool>(&self, mut address: u32) -> u16 {
        if WORD {
            address |= 1;
        }
        self.bus.read_ram_cartridge(address).into()
    }

    pub fn write_ram_cartridge<const WORD: bool>(&mut self, mut address: u32, value: u16) {
        if WORD {
            address |= 1;
        }
        self.bus.write_ram_cartridge(address, value as u8);
    }

    pub fn main_read_register<const WORD: bool>(&mut self, address: u32) -> u16 {
        self.bus.main_read_register::<WORD>(address)
    }

    pub fn main_write_register<const WORD: bool>(&mut self, address: u32, value: u16) {
        self.bus.main_write_register::<WORD>(address, value);
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        self.sub_cpu_divider = config.sega_cd.sub_cpu_divider.get();
        self.bus.reload_config(config);

        self.config = config.clone();
    }

    pub fn reset(&mut self) {
        self.bus.reset();
    }

    pub fn disc_title(&self) -> Option<String> {
        self.disc_title.clone()
    }

    pub fn region(&self) -> GenesisRegion {
        self.bus.region()
    }

    pub fn has_six_button_incompatible_game(&self) -> bool {
        self.six_button_incompatible_game
    }

    pub fn take_backup_ram_dirty(&mut self) -> bool {
        self.bus.take_backup_ram_dirty()
    }

    pub fn backup_ram(&self) -> &[u8] {
        self.bus.backup_ram()
    }

    pub fn ram_cartridge(&self) -> &[u8] {
        self.bus.ram_cartridge()
    }

    pub fn take_bios_and_disc(self) -> (Vec<u8>, Option<CdRom>) {
        self.bus.take_bios_and_disc()
    }
}

/// Attempt to parse a console region out of the CD-ROM's data track.
///
/// Returns None if unable to confidently determine region.
///
/// # Errors
///
/// Propagates any errors encountered while reading the CD-ROM files from disk.
pub fn parse_disc_region(disc: &mut CdRom) -> SegaCdLoadResult<GenesisRegion> {
    // ROM header is always located at track 1 sector 0
    let mut rom_header = vec![0; cdrom::BYTES_PER_SECTOR as usize];
    disc.read_sector(1, CdTime::SECTOR_0_START, &mut rom_header)?;

    // Sega CD ROM header starts at $010 because the first 16 bytes are sync + CD-ROM data track header
    let region = GenesisRegion::from_rom(&rom_header[0x010..]).unwrap_or_else(|| {
        log::warn!("Unable to determine region from ROM header; defaulting to US");
        GenesisRegion::Americas
    });

    let serial_number = &rom_header[0x190..0x1A0];

    // The Smurfs (EU) has a US header but only works properly with PAL timings
    if region == GenesisRegion::Americas && serial_number == b"GM T-151015-00  " {
        return Ok(GenesisRegion::Europe);
    }

    // Hack to fix Snatcher (US/EU), which incorrectly reports its region as J in the header
    if region == GenesisRegion::Japan && serial_number == b"GM T-95035 -00  " {
        let console_name = &rom_header[0x110..0x120];
        if console_name == "SEGA GENESIS    ".as_bytes() {
            return Ok(GenesisRegion::Americas);
        } else if console_name == "SEGA MEGA DRIVE ".as_bytes() {
            return Ok(GenesisRegion::Europe);
        }
        // Any other console name is unexpected, leave region as-is
    }

    Ok(region)
}
