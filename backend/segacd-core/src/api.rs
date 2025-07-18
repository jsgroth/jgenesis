//! Sega CD public interface and main loop

use crate::audio::AudioResampler;
use crate::graphics::GraphicsCoprocessor;
use crate::memory;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::Rf5c164;
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::reader::{CdRom, CdRomFileFormat};
use genesis_config::{GenesisButton, GenesisRegion, PcmInterpolation};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::timing::CycleCounters;
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::Ym2612;
use genesis_core::{GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, PartialClone, Renderer, SaveWriter,
    TickEffect, TimingMode,
};
use jgenesis_proc_macros::ConfigDisplay;
use m68000_emu::M68000;
use smsgg_config::Sn76489Version;
use smsgg_core::psg::{Sn76489, Sn76489TickEffect};
use std::fmt::{Debug, Display};
use std::num::{NonZeroU16, NonZeroU64};
use std::path::Path;
use thiserror::Error;
use z80_emu::Z80;

pub const DEFAULT_SUB_CPU_DIVIDER: u64 = genesis_config::DEFAULT_SUB_CPU_DIVIDER;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
pub const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

const BIOS_LEN: usize = memory::BIOS_LEN;

// Stall the main CPU for 2 out of every 172 mclk cycles instead of 2 out of 128 because this fixes
// some tests in mcd-verificator.
// I have no evidence that the main CPU actually does run faster with Sega CD compared to standalone
// Genesis, but it's at least plausible that the memory refresh behavior is different when executing
// out of BIOS ROM compared to Genesis cartridge ROM or WRAM
type SegaCdCycleCounters = CycleCounters<172>;

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

#[derive(Debug, Error)]
pub enum SegaCdError<RErr, AErr, SErr> {
    #[error("Disc-related error: {0}")]
    Disc(#[from] SegaCdLoadError),
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

pub type SegaCdResult<T, RErr, AErr, SErr> = Result<T, SegaCdError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct SegaCdEmulatorConfig {
    #[cfg_display(skip)]
    pub genesis: GenesisEmulatorConfig,
    pub pcm_interpolation: PcmInterpolation,
    pub enable_ram_cartridge: bool,
    pub load_disc_into_ram: bool,
    pub disc_drive_speed: NonZeroU16,
    pub sub_cpu_divider: NonZeroU64,
    pub pcm_lpf_enabled: bool,
    pub pcm_lpf_cutoff: u32,
    pub apply_genesis_lpf_to_pcm: bool,
    pub apply_genesis_lpf_to_cd_da: bool,
    pub pcm_enabled: bool,
    pub cd_audio_enabled: bool,
}

impl EmulatorConfigTrait for SegaCdEmulatorConfig {
    fn with_overclocking_disabled(&self) -> Self {
        Self {
            genesis: self.genesis.with_overclocking_disabled(),
            disc_drive_speed: NonZeroU16::new(1).unwrap(),
            sub_cpu_divider: NonZeroU64::new(DEFAULT_SUB_CPU_DIVIDER).unwrap(),
            ..*self
        }
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct SegaCdEmulator {
    #[partial_clone(partial)]
    memory: Memory<SegaCd>,
    main_cpu: M68000,
    sub_cpu: M68000,
    z80: Z80,
    vdp: Vdp,
    graphics_coprocessor: GraphicsCoprocessor,
    ym2612: Ym2612,
    psg: Sn76489,
    pcm: Rf5c164,
    input: InputState,
    audio_resampler: AudioResampler,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    disc_title: String,
    cycles: SegaCdCycleCounters,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycle_product: u64,
    sub_cpu_divider: u64,
    sub_cpu_wait_cycles: u64,
    sub_cpu_pending_intack: Option<u8>,
    config: SegaCdEmulatorConfig,
}

// This is a macro instead of a function so that it only mutably borrows the needed fields
macro_rules! new_main_bus {
    ($self:expr, m68k_reset: $m68k_reset:expr) => {
        MainBus::new(
            &mut $self.memory,
            &mut $self.vdp,
            &mut $self.psg,
            &mut $self.ym2612,
            &mut $self.input,
            &mut $self.cycles,
            $self.main_cpu.next_opcode(),
            $self.timing_mode,
            MainBusSignals { m68k_reset: $m68k_reset },
            std::mem::take(&mut $self.main_bus_writes),
        )
    };
}

impl SegaCdEmulator {
    /// Create a Sega CD emulator that reads a CD-ROM image from disk.
    ///
    /// # Errors
    ///
    /// Returns an error in any of the following conditions:
    /// * The BIOS is invalid
    /// * Unable to read the given CUE or CHD file
    /// * Unable to read every BIN file that is referenced in the CUE file
    /// * Unable to read boot information from the beginning of the CD-ROM data track
    #[allow(clippy::if_then_some_else_none)]
    pub fn create<P: AsRef<Path>, S: SaveWriter>(
        bios: Vec<u8>,
        rom_path: P,
        format: CdRomFileFormat,
        run_without_disc: bool,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> SegaCdLoadResult<Self> {
        let disc = if !run_without_disc {
            Some(if emulator_config.load_disc_into_ram {
                CdRom::open_in_memory(rom_path, format)?
            } else {
                CdRom::open(rom_path, format)?
            })
        } else {
            None
        };

        Self::create_from_disc(bios, disc, emulator_config, save_writer)
    }

    /// Create a Sega CD emulator that reads a CD-ROM image from an in-memory CHD image.
    ///
    /// # Errors
    ///
    /// Returns an error if the BIOS is invalid, the CHD image is invalid, or the emulator is unable
    /// to read boot information from the beginning of the CD-ROM data track.
    pub fn create_in_memory<S: SaveWriter>(
        bios: Vec<u8>,
        chd_bytes: Vec<u8>,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> SegaCdLoadResult<Self> {
        let disc = CdRom::open_chd_in_memory(chd_bytes)?;

        Self::create_from_disc(bios, Some(disc), emulator_config, save_writer)
    }

    fn create_from_disc<S: SaveWriter>(
        bios: Vec<u8>,
        disc: Option<CdRom>,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> SegaCdLoadResult<Self> {
        if bios.len() != BIOS_LEN {
            return Err(SegaCdLoadError::InvalidBios { bios_len: bios.len() });
        }

        let initial_backup_ram = save_writer.load_bytes("sav").ok();
        let initial_ram_cartridge = save_writer.load_bytes("ramc").ok();
        let mut sega_cd =
            SegaCd::new(bios, disc, initial_backup_ram, initial_ram_cartridge, &emulator_config)?;
        let disc_title = sega_cd.disc_title()?.unwrap_or("(no disc)".into());

        let memory = Memory::new(sega_cd);
        let timing_mode =
            emulator_config.genesis.forced_timing_mode.unwrap_or_else(|| {
                match memory.hardware_region() {
                    GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
                    GenesisRegion::Europe => TimingMode::Pal,
                }
            });

        log::info!("Running with timing/display mode: {timing_mode}");

        let main_cpu = M68000::builder().allow_tas_writes(false).name("Main".into()).build();
        let sub_cpu = M68000::builder().name("Sub".into()).build();
        let z80 = Z80::new();
        let vdp = Vdp::new(timing_mode, emulator_config.genesis.to_vdp_config());
        let graphics_coprocessor = GraphicsCoprocessor::new();
        let ym2612 = Ym2612::new_from_config(&emulator_config.genesis);
        let psg = Sn76489::new(Sn76489Version::Standard);
        let pcm = Rf5c164::new(&emulator_config);
        let input = InputState::new(
            emulator_config.genesis.p1_controller_type,
            emulator_config.genesis.p2_controller_type,
        );

        let audio_resampler = AudioResampler::new(timing_mode, emulator_config);
        let mut emulator = Self {
            memory,
            main_cpu,
            sub_cpu,
            z80,
            vdp,
            graphics_coprocessor,
            ym2612,
            psg,
            pcm,
            input,
            audio_resampler,
            timing_mode,
            main_bus_writes: MainBusWrites::new(),
            disc_title,
            cycles: SegaCdCycleCounters::new(emulator_config.genesis.clamped_m68k_divider()),
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycle_product: 0,
            sub_cpu_divider: emulator_config.sub_cpu_divider.get(),
            sub_cpu_wait_cycles: 0,
            sub_cpu_pending_intack: None,
            config: emulator_config,
        };

        // Reset main CPU so that execution starts from the right place
        emulator.main_cpu.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        Ok(emulator)
    }

    #[inline]
    fn tick_sub_cpu(&mut self, mut sub_cpu_cycles: u64) {
        if self.memory.medium().word_ram().sub_performed_blocked_access() {
            // If the sub CPU accesses word RAM while it's in 2M mode and owned by the main CPU, it
            // should halt until the main CPU writes DMNA=1 to transfer ownership to the sub CPU.
            // Marko's Magic Football depends on this or it will have glitched map graphics
            log::trace!("Not running sub CPU because word RAM writes are buffered");
            return;
        }

        let mut bus = SubBus::new(&mut self.memory, &mut self.graphics_coprocessor, &mut self.pcm);

        while sub_cpu_cycles >= self.sub_cpu_wait_cycles {
            let wait_cycles = self.sub_cpu_wait_cycles;
            self.sub_cpu_wait_cycles = self.sub_cpu.execute_instruction(&mut bus).into();
            sub_cpu_cycles -= wait_cycles;

            if bus.memory.medium().word_ram().sub_performed_blocked_access() {
                return;
            }
        }

        self.sub_cpu_wait_cycles -= sub_cpu_cycles;
    }

    fn render_frame<R: Renderer>(&self, renderer: &mut R) -> Result<(), R::Err> {
        genesis_core::render_frame(self.timing_mode, &self.vdp, &self.config.genesis, renderer)
    }

    #[inline]
    #[must_use]
    pub fn disc_title(&self) -> &str {
        &self.disc_title
    }

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }

    pub fn remove_disc(&mut self) {
        self.memory.medium_mut().remove_disc();
        self.disc_title = "(no disc)".into();
    }

    /// # Errors
    ///
    /// This method will return an error if the disc drive is unable to load the disc.
    pub fn change_disc<P: AsRef<Path>>(
        &mut self,
        rom_path: P,
        format: CdRomFileFormat,
    ) -> SegaCdLoadResult<()> {
        let sega_cd = self.memory.medium_mut();
        sega_cd.change_disc(rom_path, format, self.config.load_disc_into_ram)?;
        self.disc_title = sega_cd.disc_title()?.unwrap_or_else(|| "(no disc)".into());

        Ok(())
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }

    pub fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.vdp.dump_registers(callback);
    }
}

impl EmulatorTrait for SegaCdEmulator {
    type Button = GenesisButton;
    type Inputs = GenesisInputs;
    type Config = SegaCdEmulatorConfig;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = SegaCdError<RErr, AErr, SErr>;

    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> Result<TickEffect, Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        let mut main_bus = new_main_bus!(self, m68k_reset: false);

        // Main 68000
        let m68k_wait = main_bus.cycles.m68k_wait_cpu_cycles != 0;
        let main_cpu_cycles = if m68k_wait {
            main_bus.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.main_cpu.execute_instruction(&mut main_bus)
        };
        let genesis_mclk_elapsed = main_bus.cycles.record_68k_instruction(
            main_cpu_cycles,
            m68k_wait,
            main_bus.vdp.should_halt_cpu(),
        );

        // Z80
        while main_bus.cycles.should_tick_z80() {
            if !main_bus.cycles.z80_halt {
                self.z80.tick(&mut main_bus);
            }
            main_bus.cycles.decrement_z80();
        }

        self.main_bus_writes = main_bus.take_writes();

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

        // Disc drive and timer/stopwatch
        let sega_cd = self.memory.medium_mut();
        sega_cd.tick(elapsed_scd_mclk_cycles, &mut self.pcm, |sample_l, sample_r| {
            self.audio_resampler.collect_cd_sample(sample_l, sample_r);
        })?;

        // Graphics ASIC
        if !sega_cd.word_ram().is_sub_access_blocked() {
            let graphics_interrupt_enabled = sega_cd.graphics_interrupt_enabled();
            self.graphics_coprocessor.tick(
                elapsed_scd_mclk_cycles,
                sega_cd.word_ram_mut(),
                graphics_interrupt_enabled,
            );
        }

        // Sub 68000
        self.tick_sub_cpu(sub_cpu_cycles);

        // Input state (for 6-button controller reset)
        self.input.tick(main_cpu_cycles);

        // PSG
        while self.cycles.should_tick_psg() {
            if self.psg.tick() == Sn76489TickEffect::Clocked {
                // PSG output is mono in Genesis; stereo output is only for Game Gear
                let (psg_sample, _) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample);
            }
            self.cycles.decrement_psg();
        }

        // YM2612
        if self.cycles.has_ym2612_ticks() {
            let ym2612_ticks = self.cycles.take_ym2612_ticks();
            self.ym2612
                .tick(ym2612_ticks, |(l, r)| self.audio_resampler.collect_ym2612_sample(l, r));
        }

        // RF5C164
        self.pcm.tick(pcm_cycles, |(pcm_sample_l, pcm_sample_r)| {
            self.audio_resampler.collect_pcm_sample(pcm_sample_l, pcm_sample_r);
        });

        // Output any audio samples that are queued up
        self.audio_resampler.output_samples(audio_output).map_err(SegaCdError::Audio)?;

        // VDP
        let mut tick_effect = TickEffect::None;
        if self.vdp.tick(genesis_mclk_elapsed, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(SegaCdError::Render)?;

            self.input.set_inputs(*inputs);

            if self.memory.medium_mut().get_and_clear_backup_ram_dirty_bit() {
                let sega_cd = self.memory.medium();

                save_writer
                    .persist_bytes("sav", sega_cd.backup_ram())
                    .map_err(SegaCdError::SaveWrite)?;

                save_writer
                    .persist_bytes("ramc", sega_cd.ram_cartridge())
                    .map_err(SegaCdError::SaveWrite)?;
            }

            tick_effect = TickEffect::FrameRendered;
        }

        genesis_core::check_for_long_dma_skip(&self.vdp, &mut self.cycles);

        if !m68k_wait {
            self.vdp.clear_interrupt_delays();
        }

        // Apply main CPU writes after ticking the sub CPU; this fixes random freezing in Silpheed
        self.main_bus_writes = new_main_bus!(self, m68k_reset: false).apply_writes();

        Ok(tick_effect)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.vdp.reload_config(config.genesis.to_vdp_config());
        self.ym2612.reload_config(config.genesis);
        self.pcm.reload_config(config);
        self.input.reload_config(config.genesis);
        self.audio_resampler.reload_config(self.timing_mode, *config);
        self.cycles.update_m68k_divider(config.genesis.clamped_m68k_divider());
        self.sub_cpu_divider = config.sub_cpu_divider.get();

        let sega_cd = self.memory.medium_mut();
        sega_cd.reload_config(config);

        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }

    fn soft_reset(&mut self) {
        // Reset main CPU
        self.main_cpu.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();

        self.ym2612.reset();
        self.pcm.disable();

        self.memory.medium_mut().reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let sega_cd = self.memory.medium_mut();
        let bios = Vec::from(sega_cd.bios());
        let disc = sega_cd.take_cdrom();

        *self = Self::create_from_disc(bios, disc, self.config, save_writer)
            .expect("Hard reset should not cause an I/O error");
    }

    fn save_state_version() -> &'static str {
        "0.10.1-0"
    }

    fn target_fps(&self) -> f64 {
        genesis_core::target_framerate(&self.vdp, self.timing_mode)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}
