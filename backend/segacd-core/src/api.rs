//! Sega CD public interface and main loop

use crate::audio::AudioResampler;
use crate::cddrive::CdTickEffect;
use crate::graphics::GraphicsCoprocessor;
use crate::memory;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::{PcmTickEffect, Rf5c164};
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::reader::{CdRom, CdRomFileFormat};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::timing::CycleCounters;
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, PartialClone, Renderer, SaveWriter, TickEffect, TimingMode,
};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fmt::{Debug, Display};
use std::path::Path;
use thiserror::Error;
use z80_emu::Z80;

pub(crate) const SUB_CPU_DIVIDER: u64 = 4;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
pub(crate) const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

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

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SegaCdEmulatorConfig {
    pub genesis: GenesisEmulatorConfig,
    pub enable_ram_cartridge: bool,
    pub load_disc_into_ram: bool,
    pub pcm_enabled: bool,
    pub cd_audio_enabled: bool,
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
    psg: Psg,
    pcm: Rf5c164,
    input: InputState,
    audio_resampler: AudioResampler,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    disc_title: String,
    cycles: SegaCdCycleCounters,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycle_product: u64,
    sub_cpu_wait_cycles: u64,
    sub_cpu_pending_intack: Option<u8>,
    load_disc_into_ram: bool,
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
        let mut sega_cd = SegaCd::new(
            bios,
            disc,
            initial_backup_ram,
            initial_ram_cartridge,
            emulator_config.enable_ram_cartridge,
            emulator_config.genesis.forced_region,
        )?;
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
        let ym2612 = Ym2612::new(emulator_config.genesis);
        let psg = Psg::new(PsgVersion::Standard);
        let pcm = Rf5c164::new();
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
            aspect_ratio: emulator_config.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: emulator_config
                .genesis
                .adjust_aspect_ratio_in_2x_resolution,
            disc_title,
            cycles: SegaCdCycleCounters::default(),
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycle_product: 0,
            sub_cpu_wait_cycles: 0,
            sub_cpu_pending_intack: None,
            load_disc_into_ram: emulator_config.load_disc_into_ram,
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
        genesis_core::render_frame(
            &self.vdp,
            self.aspect_ratio,
            self.adjust_aspect_ratio_in_2x_resolution,
            renderer,
        )
    }

    #[must_use]
    pub fn disc_title(&self) -> &str {
        &self.disc_title
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
        sega_cd.change_disc(rom_path, format, self.load_disc_into_ram)?;
        self.disc_title = sega_cd.disc_title()?.unwrap_or_else(|| "(no disc)".into());

        Ok(())
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }
}

impl EmulatorTrait for SegaCdEmulator {
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
        let main_cpu_cycles = if self.cycles.m68k_wait_cpu_cycles != 0 {
            self.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.main_cpu.execute_instruction(&mut main_bus)
        };
        let genesis_mclk_elapsed = self.cycles.record_68k_instruction(
            main_cpu_cycles,
            self.main_cpu.last_instruction_was_mul_or_div(),
        );

        // Z80
        while self.cycles.should_tick_z80() {
            self.z80.tick(&mut main_bus);
            self.cycles.decrement_z80();
        }

        if main_bus.z80_accessed_68k_bus() {
            self.cycles.record_z80_68k_bus_access();
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

        let sub_cpu_cycles =
            self.sega_cd_mclk_cycles / SUB_CPU_DIVIDER - prev_scd_mclk_cycles / SUB_CPU_DIVIDER;
        let elapsed_scd_mclk_cycles = self.sega_cd_mclk_cycles - prev_scd_mclk_cycles;

        // Disc drive and timer/stopwatch
        let sega_cd = self.memory.medium_mut();
        if let CdTickEffect::OutputAudioSample(sample_l, sample_r) =
            sega_cd.tick(elapsed_scd_mclk_cycles, &mut self.pcm)?
        {
            self.audio_resampler.collect_cd_sample(sample_l, sample_r);
        }

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

        // Apply main CPU writes after ticking the sub CPU; this fixes random freezing in Silpheed
        self.main_bus_writes = new_main_bus!(self, m68k_reset: false).apply_writes();

        // Input state (for 6-button controller reset)
        self.input.tick(main_cpu_cycles);

        // PSG
        while self.cycles.should_tick_psg() {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (psg_sample_l, psg_sample_r) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample_l, psg_sample_r);
            }
            self.cycles.decrement_psg();
        }

        // YM2612
        while self.cycles.should_tick_ym2612() {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (ym2612_sample_l, ym2612_sample_r) = self.ym2612.sample();
                self.audio_resampler.collect_ym2612_sample(ym2612_sample_l, ym2612_sample_r);
            }
            self.cycles.decrement_ym2612();
        }

        // RF5C164
        if self.pcm.tick(sub_cpu_cycles) == PcmTickEffect::Clocked {
            let (pcm_sample_l, pcm_sample_r) = self.pcm.sample();
            self.audio_resampler.collect_pcm_sample(pcm_sample_l, pcm_sample_r);
        }

        // Output any audio samples that are queued up
        self.audio_resampler.output_samples(audio_output).map_err(SegaCdError::Audio)?;

        // VDP
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

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.aspect_ratio = config.genesis.aspect_ratio;
        self.adjust_aspect_ratio_in_2x_resolution =
            config.genesis.adjust_aspect_ratio_in_2x_resolution;
        self.vdp.reload_config(config.genesis.to_vdp_config());
        self.ym2612.reload_config(config.genesis);
        self.input.reload_config(config.genesis);
        self.audio_resampler.reload_config(*config);

        let sega_cd = self.memory.medium_mut();
        sega_cd.set_forced_region(config.genesis.forced_region);
        sega_cd.set_enable_ram_cartridge(config.enable_ram_cartridge);

        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }

    fn soft_reset(&mut self) {
        // Reset main CPU
        self.main_cpu.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();

        self.ym2612.reset(self.config.genesis);
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

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
