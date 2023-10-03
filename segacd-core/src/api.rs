use crate::audio::AudioDownsampler;
use crate::cddrive::CdTickEffect;
use crate::cdrom::cue;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::memory;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::{PcmTickEffect, Rf5c164};
use bincode::{Decode, Encode};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, Memory};
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{
    GenesisAspectRatio, GenesisEmulator, GenesisEmulatorConfig, GenesisInputs, GenesisRegion,
};
use jgenesis_traits::frontend::{
    AudioOutput, Color, ConfigReload, EmulatorDebug, EmulatorTrait, PartialClone, Renderer,
    Resettable, SaveWriter, TakeRomFrom, TickEffect, TickableEmulator, TimingMode,
};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fmt::{Debug, Display};
use std::io;
use std::path::Path;
use thiserror::Error;
use z80_emu::Z80;

const MAIN_CPU_DIVIDER: u64 = 7;
const SUB_CPU_DIVIDER: u64 = 4;
const Z80_DIVIDER: u64 = 15;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

// Arbitrary value to keep mclk counts low-ish for better floating-point precision
const SEGA_CD_MCLK_MODULO: f64 = 100_000_000.0;

const BIOS_LEN: usize = memory::BIOS_LEN;

#[derive(Debug, Error)]
pub enum DiscError {
    #[error("BIOS is required for Sega CD emulation")]
    MissingBios,
    #[error("BIOS must be {BIOS_LEN} bytes, was {bios_len} bytes")]
    InvalidBios { bios_len: usize },
    #[error("Unable to determine parent directory of CUE file '{0}'")]
    CueParentDir(String),
    #[error("Error parsing CUE file: {0}")]
    CueParse(String),
    #[error("Invalid/unsupported FILE line in CUE file: {0}")]
    CueInvalidFileLine(String),
    #[error("Invalid/unsupported TRACK line in CUE file: {0}")]
    CueInvalidTrackLine(String),
    #[error("Invalid/unsupported INDEX line in CUE file: {0}")]
    CueInvalidIndexLine(String),
    #[error("Invalid/unsupported PREGAP line in CUE file: {0}")]
    CueInvalidPregapLine(String),
    #[error("Unable to get file metadata for file '{path}': {source}")]
    FsMetadata {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening CUE file '{path}': {source}")]
    CueOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening BIN file '{path}': {source}")]
    BinOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("I/O error reading from disc: {0}")]
    DiscReadIo(#[source] io::Error),
    #[error(
        "CD-ROM error detection check failed for track {track_number} sector {sector_number}; expected={expected:08X}, actual={actual:08X}"
    )]
    DiscReadInvalidChecksum { track_number: u8, sector_number: u32, expected: u32, actual: u32 },
}

pub type DiscResult<T> = Result<T, DiscError>;

#[derive(Debug, Error)]
pub enum SegaCdError<RErr, AErr, SErr> {
    #[error("Disc-related error: {0}")]
    Disc(#[from] DiscError),
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

pub type SegaCdResult<T, RErr, AErr, SErr> = Result<T, SegaCdError<RErr, AErr, SErr>>;

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
    audio_downsampler: AudioDownsampler,
    timing_mode: TimingMode,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    disc_title: String,
    genesis_mclk_cycles: u64,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycles_float: f64,
    sub_cpu_wait_cycles: u64,
}

impl SegaCdEmulator {
    /// # Errors
    ///
    /// Returns an error in any of the following conditions:
    /// * Unable to read the given CUE file
    /// * Unable to read every BIN file that is referenced in the CUE
    /// * Unable to read boot information from the beginning of the data track
    #[allow(clippy::if_then_some_else_none)]
    pub fn create<P: AsRef<Path>>(
        bios: Vec<u8>,
        cue_path: P,
        initial_backup_ram: Option<Vec<u8>>,
        run_without_disc: bool,
        emulator_config: GenesisEmulatorConfig,
    ) -> DiscResult<Self> {
        if bios.len() != BIOS_LEN {
            return Err(DiscError::InvalidBios { bios_len: bios.len() });
        }

        let disc = if !run_without_disc {
            let cue_path = cue_path.as_ref();
            let cue_parent_dir = cue_path
                .parent()
                .ok_or_else(|| DiscError::CueParentDir(cue_path.display().to_string()))?;

            let cue_sheet = cue::parse(cue_path)?;
            Some(CdRom::open(cue_sheet, cue_parent_dir)?)
        } else {
            None
        };

        Self::create_from_disc(bios, initial_backup_ram, disc, emulator_config)
    }

    fn create_from_disc(
        bios: Vec<u8>,
        initial_backup_ram: Option<Vec<u8>>,
        disc: Option<CdRom>,
        emulator_config: GenesisEmulatorConfig,
    ) -> DiscResult<Self> {
        let mut sega_cd =
            SegaCd::new(bios, disc, initial_backup_ram, emulator_config.forced_region)?;
        let disc_title = sega_cd.disc_title()?.unwrap_or("(no disc)".into());

        let mut memory = Memory::new(sega_cd);
        let timing_mode =
            emulator_config.forced_timing_mode.unwrap_or_else(|| match memory.hardware_region() {
                GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
                GenesisRegion::Europe => TimingMode::Pal,
            });

        log::info!("Running with timing/display mode: {timing_mode}");

        let mut main_cpu = M68000::builder().allow_tas_writes(false).name("Main".into()).build();
        let sub_cpu = M68000::builder().name("Sub".into()).build();
        let z80 = Z80::new();
        let mut vdp = Vdp::new(timing_mode, !emulator_config.remove_sprite_limits);
        let graphics_coprocessor = GraphicsCoprocessor::new();
        let mut ym2612 = Ym2612::new();
        let mut psg = Psg::new(PsgVersion::Standard);
        let pcm = Rf5c164::new();
        let mut input = InputState::new();

        // Reset main CPU
        main_cpu.execute_instruction(&mut MainBus::new(
            &mut memory,
            &mut vdp,
            &mut psg,
            &mut ym2612,
            &mut input,
            timing_mode,
            MainBusSignals { z80_busack: false, m68k_reset: true },
        ));

        let audio_downsampler = AudioDownsampler::new(timing_mode);
        Ok(Self {
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
            audio_downsampler,
            timing_mode,
            aspect_ratio: emulator_config.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: emulator_config
                .adjust_aspect_ratio_in_2x_resolution,
            disc_title,
            genesis_mclk_cycles: 0,
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycles_float: 0.0,
            sub_cpu_wait_cycles: 0,
        })
    }

    #[inline]
    fn tick_sub_cpu(&mut self, mut sub_cpu_cycles: u64) {
        while sub_cpu_cycles >= self.sub_cpu_wait_cycles {
            let wait_cycles = self.sub_cpu_wait_cycles;
            let mut bus =
                SubBus::new(&mut self.memory, &mut self.graphics_coprocessor, &mut self.pcm);
            self.sub_cpu_wait_cycles = self.sub_cpu.execute_instruction(&mut bus).into();
            sub_cpu_cycles -= wait_cycles;
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
    pub fn change_disc<P: AsRef<Path>>(&mut self, cue_path: P) -> DiscResult<()> {
        let sega_cd = self.memory.medium_mut();
        sega_cd.change_disc(cue_path)?;
        self.disc_title = sega_cd.disc_title()?.unwrap_or_else(|| "(no disc)".into());

        Ok(())
    }
}

impl TickableEmulator for SegaCdEmulator {
    type Inputs = GenesisInputs;
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
        let mut main_bus = MainBus::new(
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            &mut self.ym2612,
            &mut self.input,
            self.timing_mode,
            MainBusSignals { z80_busack: self.z80.stalled(), m68k_reset: false },
        );

        // Main 68000
        let main_cpu_cycles = self.main_cpu.execute_instruction(&mut main_bus);

        let genesis_mclk_elapsed = u64::from(main_cpu_cycles) * MAIN_CPU_DIVIDER;
        let z80_cycles = (self.genesis_mclk_cycles + genesis_mclk_elapsed) / Z80_DIVIDER
            - self.genesis_mclk_cycles / Z80_DIVIDER;
        self.genesis_mclk_cycles += genesis_mclk_elapsed;

        // Z80
        for _ in 0..z80_cycles {
            self.z80.tick(&mut main_bus);
        }

        let genesis_master_clock_rate = match self.timing_mode {
            TimingMode::Ntsc => NTSC_GENESIS_MASTER_CLOCK_RATE,
            TimingMode::Pal => PAL_GENESIS_MASTER_CLOCK_RATE,
        };

        // TODO avoid floating point
        let sega_cd_mclk_elapsed_float = genesis_mclk_elapsed as f64
            * SEGA_CD_MASTER_CLOCK_RATE as f64
            / genesis_master_clock_rate as f64;
        self.sega_cd_mclk_cycles_float += sega_cd_mclk_elapsed_float;
        let prev_scd_mclk_cycles = self.sega_cd_mclk_cycles;
        self.sega_cd_mclk_cycles = self.sega_cd_mclk_cycles_float.round() as u64;

        let sub_cpu_cycles =
            self.sega_cd_mclk_cycles / SUB_CPU_DIVIDER - prev_scd_mclk_cycles / SUB_CPU_DIVIDER;
        let elapsed_scd_mclk_cycles = self.sega_cd_mclk_cycles - prev_scd_mclk_cycles;

        self.sega_cd_mclk_cycles_float %= SEGA_CD_MCLK_MODULO;
        self.sega_cd_mclk_cycles = self.sega_cd_mclk_cycles_float.round() as u64;

        // Disc drive and timer/stopwatch
        let sega_cd = self.memory.medium_mut();
        if let CdTickEffect::OutputAudioSample(sample_l, sample_r) =
            sega_cd.tick(elapsed_scd_mclk_cycles, &mut self.pcm)?
        {
            self.audio_downsampler.collect_cd_sample(sample_l, sample_r);
        }

        // Graphics ASIC
        let graphics_interrupt_enabled = sega_cd.graphics_interrupt_enabled();
        self.graphics_coprocessor.tick(
            elapsed_scd_mclk_cycles,
            sega_cd.word_ram_mut(),
            graphics_interrupt_enabled,
        );

        // Sub 68000
        self.tick_sub_cpu(sub_cpu_cycles);

        // Input state (for 6-button controller reset)
        self.input.tick(main_cpu_cycles);

        // PSG
        for _ in 0..z80_cycles {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (psg_sample_l, psg_sample_r) = self.psg.sample();
                self.audio_downsampler.collect_psg_sample(psg_sample_l, psg_sample_r);
            }
        }

        // YM2612
        for _ in 0..main_cpu_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (ym2612_sample_l, ym2612_sample_r) = self.ym2612.sample();
                self.audio_downsampler.collect_ym2612_sample(ym2612_sample_l, ym2612_sample_r);
            }
        }

        // RF5C164
        if self.pcm.tick(sub_cpu_cycles) == PcmTickEffect::Clocked {
            let (pcm_sample_l, pcm_sample_r) = self.pcm.sample();
            self.audio_downsampler.collect_pcm_sample(pcm_sample_l, pcm_sample_r);
        }

        // Output any audio samples that are queued up
        self.audio_downsampler.output_samples(audio_output).map_err(SegaCdError::Audio)?;

        // VDP
        if self.vdp.tick(genesis_mclk_elapsed, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(SegaCdError::Render)?;

            self.input.set_inputs(inputs);

            if self.memory.medium_mut().get_and_clear_backup_ram_dirty_bit() {
                save_writer
                    .persist_save(self.memory.medium().backup_ram())
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
}

impl Resettable for SegaCdEmulator {
    fn soft_reset(&mut self) {
        // Reset main CPU
        self.main_cpu.execute_instruction(&mut MainBus::new(
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            &mut self.ym2612,
            &mut self.input,
            self.timing_mode,
            MainBusSignals { z80_busack: false, m68k_reset: true },
        ));
        self.memory.reset_z80_signals();

        self.ym2612.reset();
        self.pcm.disable();

        self.memory.medium_mut().reset();
    }

    fn hard_reset(&mut self) {
        let sega_cd = self.memory.medium_mut();
        let bios = Vec::from(sega_cd.bios());
        let disc = sega_cd.take_cdrom();
        let backup_ram = Vec::from(sega_cd.backup_ram());
        let forced_region = sega_cd.forced_region();

        *self = Self::create_from_disc(
            bios,
            Some(backup_ram),
            disc,
            GenesisEmulatorConfig {
                forced_timing_mode: Some(self.timing_mode),
                forced_region,
                aspect_ratio: self.aspect_ratio,
                adjust_aspect_ratio_in_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
                remove_sprite_limits: !self.vdp.get_enforce_sprite_limits(),
            },
        )
        .expect("Hard reset should not cause an I/O error");
    }
}

impl ConfigReload for SegaCdEmulator {
    type Config = GenesisEmulatorConfig;

    fn reload_config(&mut self, config: &Self::Config) {
        self.aspect_ratio = config.aspect_ratio;
        self.adjust_aspect_ratio_in_2x_resolution = config.adjust_aspect_ratio_in_2x_resolution;
        self.vdp.set_enforce_sprite_limits(!config.remove_sprite_limits);
        self.memory.medium_mut().set_forced_region(config.forced_region);
    }
}

impl TakeRomFrom for SegaCdEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }
}

impl EmulatorDebug for SegaCdEmulator {
    const NUM_PALETTES: u32 = GenesisEmulator::NUM_PALETTES;
    const PALETTE_LEN: u32 = GenesisEmulator::PALETTE_LEN;
    const PATTERN_TABLE_LEN: u32 = GenesisEmulator::PATTERN_TABLE_LEN;

    fn debug_cram(&self, out: &mut [Color]) {
        self.vdp.debug_cram(out);
    }

    fn debug_vram(&self, out: &mut [Color], palette: u8) {
        self.vdp.debug_vram(out, palette);
    }
}

impl EmulatorTrait for SegaCdEmulator {
    type EmulatorInputs = GenesisInputs;
    type EmulatorConfig = GenesisEmulatorConfig;

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
