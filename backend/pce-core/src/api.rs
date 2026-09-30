pub mod debug;

use crate::audio::PceAudioResampler;
use crate::bus::Bus;
use crate::cd::CdRomController;
use crate::input::InputState;
use crate::memory::{HuCard, Memory};
use crate::psg::Huc6280Psg;
use crate::video;
use crate::video::VideoSubsystem;
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::reader::CdRom;
use huc6280_emu::Huc6280;
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, FiniteF64, InputPoller, PartialClone,
    RenderFrameOptions, Renderer, SaveWriter, TickEffect, TickResult,
};
use jgenesis_proc_macros::ConfigDisplay;
use pce_config::{
    PceAspectRatio, PceButton, PceInputDevice, PceInputs, PcePaletteType, PcePsgResampler,
    PceRegion, PceSystemCardModel,
};
use std::cmp;
use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use thiserror::Error;

// 236.25e6 / 11
pub const MASTER_CLOCK_FREQUENCY: f64 = 21_477_272.0;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct PceEmulatorConfig {
    pub load_disc_into_ram: bool,
    pub region: PceRegion,
    pub system_card_model: PceSystemCardModel,
    pub cpu_fast_clock_divider: NonZeroU64,
    pub aspect_ratio: PceAspectRatio,
    pub palette: PcePaletteType,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
    pub psg_audio_resampler: PcePsgResampler,
    pub input_device: PceInputDevice,
    #[cfg_display(debug_fmt)]
    pub turbo_tap_connected: [bool; pce_config::TURBO_TAP_GAMEPADS as usize],
    pub allow_opposing_joypad_directions: bool,
    pub allow_simultaneous_run_select: bool,
}

impl EmulatorConfigTrait for PceEmulatorConfig {
    fn with_overclocking_disabled(&self) -> Self {
        Self {
            cpu_fast_clock_divider: NonZeroU64::new(pce_config::NATIVE_FAST_CPU_DIVIDER).unwrap(),
            ..*self
        }
    }
}

impl PceEmulatorConfig {
    #[must_use]
    pub fn clamped_cpu_fast_divider(&self) -> u64 {
        cmp::min(pce_config::NATIVE_FAST_CPU_DIVIDER, self.cpu_fast_clock_divider.get())
    }
}

#[derive(Debug, Error)]
pub enum PceError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error outputting audio: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
    #[error("CD-ROM error: {0}")]
    CdRom(#[from] CdRomError),
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct PcEngineEmulator {
    cpu: Huc6280,
    video: VideoSubsystem,
    psg: Huc6280Psg,
    memory: Memory,
    cartridge: HuCard,
    #[partial_clone(partial)]
    cd: Option<CdRomController>,
    input_state: InputState,
    audio_resampler: PceAudioResampler,
    config: PceEmulatorConfig,
    cycle_counter: u64,
    last_psg_sync_cycles: u64,
}

impl PcEngineEmulator {
    #[must_use]
    pub fn create<S: SaveWriter>(
        hucard_rom: Vec<u8>,
        disc: Option<CdRom>,
        config: PceEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let initial_sav = save_writer.load_bytes("sav").ok();
        // TODO support running with CD-ROM drive present but no disc in drive
        let cd_hardware_present = disc.is_some();

        let mut emulator = Self {
            cpu: Huc6280::new(),
            video: VideoSubsystem::new(config),
            psg: Huc6280Psg::new(),
            memory: Memory::new(&config),
            cartridge: HuCard::new(
                hucard_rom,
                initial_sav.clone(),
                cd_hardware_present,
                config.system_card_model,
            ),
            // TODO support running with CD-ROM hardware present but no disc in drive
            cd: disc.map(|disc| CdRomController::new(Some(disc), initial_sav)),
            input_state: InputState::new(config, cd_hardware_present),
            audio_resampler: PceAudioResampler::new(
                config.psg_audio_resampler,
                cd_hardware_present,
                48000,
            ),
            config,
            cycle_counter: 0,
            last_psg_sync_cycles: 0,
        };

        emulator.cpu.reset(&mut Bus {
            memory: &mut emulator.memory,
            video: &mut emulator.video,
            psg: &mut emulator.psg,
            cartridge: &mut emulator.cartridge,
            cd: emulator.cd.as_mut(),
            input: &mut emulator.input_state,
            cycle_counter: &mut emulator.cycle_counter,
            audio_resampler: &mut emulator.audio_resampler,
            cd_error: &mut None,
        });

        emulator
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        self.video.render_rgba8_frame_buffer();

        let aspect_ratio = match self.config.aspect_ratio {
            PceAspectRatio::Ntsc => Some(self.video.ntsc_aspect_ratio()),
            PceAspectRatio::SquarePixels => Some(FiniteF64::ONE),
            PceAspectRatio::Stretched => None,
        };

        renderer.render_frame(
            self.video.frame_buffer(),
            self.video.frame_size(),
            self.video.target_fps(),
            RenderFrameOptions {
                pixel_aspect_ratio: aspect_ratio,
                composite_params: Some(self.video.composite_params()),
                ntsc_per_frame_params: Some(self.video.ntsc_per_frame_params()),
                ..RenderFrameOptions::default()
            },
        )
    }

    fn need_audio_flush(&self) -> bool {
        self.cycle_counter - self.last_psg_sync_cycles >= video::MCLK_CYCLES_PER_SCANLINE
    }

    fn flush_audio<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        self.psg.step_to(self.cycle_counter, &mut self.audio_resampler);

        if let Some(cd) = &mut self.cd {
            cd.drain_audio_samples_into(&mut self.audio_resampler);
        }

        self.audio_resampler.drain_audio_output(audio_output)?;

        self.last_psg_sync_cycles = self.cycle_counter;

        Ok(())
    }

    pub fn dump_vram(&self, palette: u16, out: &mut [[Color; 64]]) {
        self.video.dump_vram(palette, out);
    }

    pub fn dump_palettes(&self, out: &mut [Color]) {
        self.video.dump_palettes(out);
    }
}

impl EmulatorTrait for PcEngineEmulator {
    type Button = PceButton;
    type Inputs = PceInputs;
    type Config = PceEmulatorConfig;
    type SaveState = Self;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = PceError<RErr, AErr, SErr>;

    fn tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
    ) -> TickResult<Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        I: InputPoller<Self::Inputs>,
        S: SaveWriter,
    {
        self.input_state.update_inputs(*input_poller.poll());

        let mut cd_error = None;
        self.cpu.execute_instruction(&mut Bus {
            memory: &mut self.memory,
            video: &mut self.video,
            psg: &mut self.psg,
            cartridge: &mut self.cartridge,
            cd: self.cd.as_mut(),
            input: &mut self.input_state,
            cycle_counter: &mut self.cycle_counter,
            audio_resampler: &mut self.audio_resampler,
            cd_error: &mut cd_error,
        });

        if let Some(err) = cd_error {
            return Err(err.into());
        }

        // Sync PSG here in case the VDC blocked a CPU VRAM access for a long amount of time across
        // a frame boundary
        if self.need_audio_flush() {
            self.flush_audio(audio_output).map_err(PceError::Audio)?;
        }

        if self.video.frame_complete() {
            self.video.clear_frame_complete();

            self.flush_audio(audio_output).map_err(PceError::Audio)?;
            self.render_frame(renderer).map_err(PceError::Render)?;

            if self.cartridge.is_sram_dirty() {
                self.cartridge.clear_sram_dirty();

                if let Some(sram) = self.cartridge.sram() {
                    // TODO use a different extension when CD-ROM is present? do any System Cards
                    // have battery-backed RAM?
                    save_writer.persist_bytes("sav", sram).map_err(PceError::SaveWrite)?;
                }
            }

            if let Some(cd) = &mut self.cd
                && cd.backup_ram_dirty()
            {
                cd.clear_backup_ram_dirty();

                save_writer.persist_bytes("sav", cd.backup_ram()).map_err(PceError::SaveWrite)?;
            }

            Ok(TickEffect::FrameRendered)
        } else {
            Ok(TickEffect::None)
        }
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.config = *config;

        self.memory.reload_config(config);
        self.video.reload_config(*config);
        self.audio_resampler.reload_config(config);
        self.input_state.reload_config(*config);
    }

    fn soft_reset(&mut self) {
        log::warn!("PC Engine does not support soft reset except in software");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.cartridge.clone_rom();
        let disc = self.cd.as_mut().and_then(CdRomController::take_disc);

        *self = Self::create(rom, disc, self.config, save_writer);
    }

    fn load_state(&mut self, mut state: Self::SaveState) {
        state.cartridge.take_rom_from(&mut self.cartridge);

        if let Some(self_cd) = &mut self.cd
            && let Some(state_cd) = &mut state.cd
        {
            state_cd.take_disc_from(self_cd);
        }

        *self = state;
    }

    fn to_save_state(&self) -> Self::SaveState {
        self.partial_clone()
    }

    fn target_fps(&self) -> f64 {
        self.video.target_fps()
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}
