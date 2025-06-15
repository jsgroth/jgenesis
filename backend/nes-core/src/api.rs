use crate::apu::ApuState;
use crate::audio::AudioResampler;
use crate::bus::cartridge::CartridgeFileError;
use crate::bus::{Bus, cartridge};
use crate::cpu::CpuState;
use crate::graphics::TimingModeGraphicsExt;
use crate::input::NesInputs;
use crate::ppu::PpuState;
use crate::{apu, audio, cpu, graphics, ppu};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, FrameSize, Renderer, SaveWriter,
    TickEffect, TickResult, TimingMode,
};
use jgenesis_proc_macros::{ConfigDisplay, PartialClone};
use std::fmt::{Debug, Display};
use std::mem;
use thiserror::Error;

pub use graphics::PatternTable;
use mos6502_emu::bus::BusInterface;
use nes_config::{NesAspectRatio, NesAudioResampler, NesButton, Overscan};

// The number of master clock ticks to run in one `Emulator::tick` call
const PAL_MASTER_CLOCK_TICKS: u32 = 80;

const PAL_CPU_DIVIDER: u32 = 16;
const PAL_PPU_DIVIDER: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, ConfigDisplay)]
pub struct NesEmulatorConfig {
    /// Force timing mode to NTSC/PAL if set
    /// If None, timing mode will default based on iNES ROM header
    pub forced_timing_mode: Option<TimingMode>,
    /// Aspect ratio
    pub aspect_ratio: NesAspectRatio,
    /// Crop frame vertically from 240px to 224px in NTSC mode
    pub ntsc_crop_vertical_overscan: bool,
    /// Overscan in pixels
    pub overscan: Overscan,
    /// If true, do not emulate the 8 sprite per scanline limit; this eliminates sprite flickering
    /// but can cause bugs in some games
    pub remove_sprite_limit: bool,
    /// If true, add a black border over the top scanline, the leftmost 2 columns, and the rightmost 2 columns
    pub pal_black_border: bool,
    /// If true, silence the triangle wave channel when it is outputting a wave at ultrasonic frequency
    pub silence_ultrasonic_triangle_output: bool,
    pub audio_resampler: NesAudioResampler,
    /// If true, adjust audio frequency so that audio sync times to 60Hz NTSC / 50Hz PAL
    pub audio_refresh_rate_adjustment: bool,
    /// Whether to allow simultaneous left+right and up+down joypad inputs.
    /// Some games exhibit severe glitches when opposing joypad directions are pressed
    /// simultaneously, e.g. Zelda 2 and Battletoads
    pub allow_opposing_joypad_inputs: bool,
}

impl EmulatorConfigTrait for NesEmulatorConfig {}

#[derive(Debug, Error)]
pub enum NesError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error outputting audio samples: {0}")]
    Audio(AErr),
    #[error("Error persisting save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Error)]
pub enum NesInitializationError {
    #[error("Error loading cartridge ROM: {0}")]
    CartridgeLoad(#[from] CartridgeFileError),
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct NesEmulator {
    #[partial_clone(partial)]
    bus: Bus,
    cpu_state: CpuState,
    ppu_state: PpuState,
    apu_state: ApuState,
    config: NesEmulatorConfig,
    rgba_frame_buffer: Vec<Color>,
    audio_resampler: AudioResampler,
    // Kept around to enable hard reset
    #[partial_clone(default)]
    raw_rom_bytes: Vec<u8>,
}

impl NesEmulator {
    /// Create a new emulator instance.
    ///
    /// # Errors
    ///
    /// This function will return an error if it cannot successfully parse NES ROM data out of the
    /// given ROM bytes.
    pub fn create<S: SaveWriter>(
        rom_bytes: Vec<u8>,
        config: NesEmulatorConfig,
        save_writer: &mut S,
    ) -> Result<Self, NesInitializationError> {
        let sav_bytes = save_writer.load_bytes("sav").ok();
        let mapper = cartridge::from_ines_file(&rom_bytes, sav_bytes, config.forced_timing_mode)?;
        let timing_mode = mapper.timing_mode();

        let mut bus = Bus::from_cartridge(mapper, config.overscan);

        let cpu_state = CpuState::new(&mut bus.cpu());
        let ppu_state = PpuState::new(timing_mode, config.ntsc_crop_vertical_overscan);
        let mut apu_state = ApuState::new(timing_mode);

        init_apu(&mut apu_state, &mut bus, config);

        Ok(Self {
            bus,
            cpu_state,
            ppu_state,
            apu_state,
            config,
            rgba_frame_buffer: new_rgba_frame_buffer(),
            audio_resampler: AudioResampler::new(timing_mode, &config),
            raw_rom_bytes: rom_bytes,
        })
    }

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.bus.mapper().timing_mode()
    }

    fn ntsc_tick(&mut self) {
        cpu::tick(&mut self.cpu_state, &mut self.bus.cpu(), self.apu_state.is_active_cycle());
        apu::tick(&mut self.apu_state, &mut self.bus.cpu(), self.config);
        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu(), self.config);
        self.bus.tick_cpu();
        self.bus.tick();

        self.bus.poll_interrupt_lines();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu(), self.config);
        self.bus.tick();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu(), self.config);
        self.bus.tick();

        self.push_audio_sample();
    }

    fn pal_tick(&mut self) {
        // Both CPU and PPU tick on the first master clock cycle
        cpu::tick(&mut self.cpu_state, &mut self.bus.cpu(), self.apu_state.is_active_cycle());
        apu::tick(&mut self.apu_state, &mut self.bus.cpu(), self.config);
        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu(), self.config);
        self.bus.tick_cpu();
        self.bus.tick();

        self.bus.poll_interrupt_lines();

        self.push_audio_sample();

        for i in 1..PAL_MASTER_CLOCK_TICKS {
            if i % PAL_CPU_DIVIDER == 0 {
                cpu::tick(
                    &mut self.cpu_state,
                    &mut self.bus.cpu(),
                    self.apu_state.is_active_cycle(),
                );
                apu::tick(&mut self.apu_state, &mut self.bus.cpu(), self.config);
                self.bus.tick_cpu();
                self.bus.tick();

                self.bus.poll_interrupt_lines();

                self.push_audio_sample();
            } else if i % PAL_PPU_DIVIDER == 0 {
                ppu::tick(&mut self.ppu_state, &mut self.bus.ppu(), self.config);
                self.bus.tick();
            }
        }
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        let overscan = self.config.overscan;
        let display_mode = if !self.config.ntsc_crop_vertical_overscan {
            TimingMode::Pal
        } else {
            self.bus.mapper().timing_mode()
        };
        graphics::ppu_frame_buffer_to_rgba(
            self.ppu_state.frame_buffer(),
            &mut self.rgba_frame_buffer,
            overscan,
            display_mode,
        );

        let visible_screen_height = display_mode.visible_screen_height();
        let frame_size = FrameSize {
            width: ppu::SCREEN_WIDTH
                .saturating_sub(overscan.left)
                .saturating_sub(overscan.right)
                .into(),
            height: visible_screen_height
                .saturating_sub(overscan.top)
                .saturating_sub(overscan.bottom)
                .into(),
        };

        if frame_size.width == 0 || frame_size.height == 0 {
            log::error!("Overscan values are too large, entire frame was cropped: {overscan}");
            return renderer.render_frame(&[Color::BLACK], FrameSize { width: 1, height: 1 }, None);
        }

        let pixel_aspect_ratio = self.config.aspect_ratio.to_pixel_aspect_ratio();

        renderer.render_frame(&self.rgba_frame_buffer, frame_size, pixel_aspect_ratio)
    }

    fn push_audio_sample(&mut self) {
        let audio_sample = {
            let sample = self.apu_state.sample();
            self.bus.mapper().sample_audio(sample)
        };

        self.audio_resampler.collect_sample(audio_sample);
    }

    pub fn copy_nametables(&mut self, pattern_table: PatternTable, out: &mut [Color]) {
        graphics::copy_nametables(pattern_table, &mut self.bus.ppu(), out);
    }

    pub fn copy_oam(&mut self, pattern_table: PatternTable, out: &mut [Color]) {
        graphics::copy_oam(pattern_table, &mut self.bus.ppu(), out);
    }

    pub fn copy_palette_ram(&mut self, out: &mut [Color]) {
        graphics::copy_palette_ram(&self.bus.ppu(), out);
    }

    #[inline]
    pub fn using_double_height_sprites(&mut self) -> bool {
        self.bus.ppu().get_ppu_registers().double_height_sprites()
    }
}

fn new_rgba_frame_buffer() -> Vec<Color> {
    vec![Color::default(); ppu::SCREEN_WIDTH as usize * ppu::MAX_SCREEN_HEIGHT as usize]
}

impl EmulatorTrait for NesEmulator {
    type Button = NesButton;
    type Inputs = NesInputs;
    type Config = NesEmulatorConfig;
    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = NesError<RErr, AErr, SErr>;

    /// Run the emulator for 1 CPU cycle / 3 PPU cycles (NTSC) or 5 CPU cycles / 16 PPU cycles (PAL).
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering a frame, pushing
    /// audio samples, or persisting SRAM.
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> TickResult<Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        let prev_in_vblank = self.ppu_state.in_vblank();

        self.bus.update_p1_joypad_state(inputs.p1, self.config.allow_opposing_joypad_inputs);
        self.bus.update_p2_joypad_state(inputs.p2, self.config.allow_opposing_joypad_inputs);

        let timing_mode = self.bus.mapper().timing_mode();

        match timing_mode {
            TimingMode::Ntsc => self.ntsc_tick(),
            TimingMode::Pal => self.pal_tick(),
        }

        self.audio_resampler.output_samples(audio_output).map_err(NesError::Audio)?;

        if !prev_in_vblank && self.ppu_state.in_vblank() {
            if self.config.pal_black_border {
                ppu::render_pal_black_border(&mut self.ppu_state);
            }

            self.render_frame(renderer).map_err(NesError::Render)?;

            if self.bus.mapper_mut().get_and_clear_ram_dirty_bit() {
                let sram = self.bus.mapper().get_prg_ram();
                save_writer.persist_bytes("sav", sram).map_err(NesError::SaveWrite)?;
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
        self.config = *config;

        self.bus.reload_config(*config);
        self.audio_resampler.reload_config(config);
        self.ppu_state.ntsc_crop_vertical_overscan = config.ntsc_crop_vertical_overscan;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.bus.move_rom_from(&mut other.bus);
        self.raw_rom_bytes = mem::take(&mut other.raw_rom_bytes);
    }

    fn soft_reset(&mut self) {
        cpu::reset(&mut self.cpu_state, &mut self.bus.cpu());
        apu::reset(&mut self.apu_state, &mut self.bus.cpu());
        ppu::reset(&mut self.ppu_state, &mut self.bus.ppu());

        for _ in 0..10 {
            apu::tick(&mut self.apu_state, &mut self.bus.cpu(), self.config);
            self.bus.tick();
        }
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom_bytes = mem::take(&mut self.raw_rom_bytes);

        *self = Self::create(rom_bytes, self.config, save_writer)
            .expect("Creation during hard reset should never fail");
    }

    fn target_fps(&self) -> f64 {
        let timing_mode = self.bus.mapper().timing_mode();
        match (timing_mode, self.config.audio_refresh_rate_adjustment) {
            (TimingMode::Ntsc, true) => 60.0,
            (TimingMode::Ntsc, false) => audio::NTSC_NES_NATIVE_DISPLAY_RATE,
            (TimingMode::Pal, true) => 50.0,
            (TimingMode::Pal, false) => audio::PAL_NES_NATIVE_DISPLAY_RATE,
        }
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}

fn init_apu(apu_state: &mut ApuState, bus: &mut Bus, config: NesEmulatorConfig) {
    // Write 0x00 to JOY2 to reset the frame counter
    bus.cpu().write(0x4017, 0x00);
    bus.tick();

    // Run the APU for 10 cycles
    for _ in 0..10 {
        apu::tick(apu_state, &mut bus.cpu(), config);
        bus.tick();
    }
}
