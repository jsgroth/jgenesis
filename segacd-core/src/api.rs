use crate::audio::AudioDownsampler;
use crate::cdrom::cue;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::Rf5c164;
use anyhow::anyhow;
use bincode::{Decode, Encode};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, Memory};
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{GenesisAspectRatio, GenesisEmulator, GenesisError, GenesisInputs};
use jgenesis_traits::frontend::{
    AudioOutput, Color, ConfigReload, EmulatorDebug, EmulatorTrait, LightClone, Renderer,
    Resettable, SaveWriter, TakeRomFrom, TickEffect, TickableEmulator, TimingMode,
};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::path::Path;
use z80_emu::Z80;

const MAIN_CPU_DIVIDER: u64 = 7;
const SUB_CPU_DIVIDER: u64 = 4;
const Z80_DIVIDER: u64 = 15;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

#[derive(Debug, Clone)]
pub struct SegaCdEmulatorConfig;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SegaCdEmulator {
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
    pub fn create<P: AsRef<Path>>(
        bios: Vec<u8>,
        cue_path: P,
        initial_backup_ram: Option<Vec<u8>>,
    ) -> anyhow::Result<Self> {
        let cue_path = cue_path.as_ref();
        let cue_parent_dir = cue_path.parent().ok_or_else(|| {
            anyhow!("Unable to determine parent directory of CUE file '{}'", cue_path.display())
        })?;

        let cue_sheet = cue::parse(cue_path)?;
        let disc = CdRom::open(cue_sheet, cue_parent_dir)?;

        // TODO read header information from disc
        let mut sega_cd = SegaCd::new(bios, disc, initial_backup_ram);
        let disc_title = sega_cd.disc_title()?.unwrap_or("(no disc)".into());

        let timing_mode = TimingMode::Ntsc;

        let mut memory = Memory::new(sega_cd);
        let mut main_cpu = M68000::default();
        let sub_cpu = M68000::default();
        let z80 = Z80::new();
        let mut vdp = Vdp::new(timing_mode, true);
        let graphics_coprocessor = GraphicsCoprocessor::new();
        let mut ym2612 = Ym2612::new();
        let mut psg = Psg::new(PsgVersion::Standard);
        let pcm = Rf5c164;
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
        genesis_core::render_frame(&self.vdp, GenesisAspectRatio::Ntsc, true, renderer)
    }

    #[must_use]
    pub fn disc_title(&self) -> &str {
        &self.disc_title
    }
}

impl TickableEmulator for SegaCdEmulator {
    type Inputs = GenesisInputs;
    type Err<RErr, AErr, SErr> = GenesisError<RErr, AErr, SErr>;

    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> Result<TickEffect, Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        S: SaveWriter,
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

        // Disc drive and timer/stopwatch
        let sega_cd = self.memory.medium_mut();
        sega_cd.tick(elapsed_scd_mclk_cycles);

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

        // Output any audio samples that are queued up
        self.audio_downsampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

        // VDP
        if self.vdp.tick(genesis_mclk_elapsed, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GenesisError::Render)?;

            self.input.set_inputs(inputs);

            if self.memory.medium_mut().get_and_clear_backup_ram_dirty_bit() {
                save_writer
                    .persist_save(self.memory.medium().backup_ram())
                    .map_err(GenesisError::Save)?;
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
        todo!("soft reset")
    }

    fn hard_reset(&mut self) {
        todo!("hard reset")
    }
}

impl ConfigReload for SegaCdEmulator {
    type Config = SegaCdEmulatorConfig;

    fn reload_config(&mut self, _config: &Self::Config) {}
}

#[derive(Debug, Clone)]
pub struct EmulatorClone(SegaCdEmulator);

impl LightClone for SegaCdEmulator {
    type Clone = EmulatorClone;

    fn light_clone(&self) -> Self::Clone {
        EmulatorClone(Self { memory: self.memory.clone_without_rom(), ..self.clone() })
    }

    fn reconstruct_from(&mut self, mut clone: Self::Clone) {
        clone.0.memory.medium_mut().take_rom_from(self.memory.medium_mut());
        *self = clone.0;
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
    type EmulatorConfig = SegaCdEmulatorConfig;

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
