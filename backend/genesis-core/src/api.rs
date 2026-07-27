//! Genesis public interface and main loop

use crate::audio::GenesisAudioResampler;
use crate::bus::GenesisBus;
use bincode::{Decode, Encode};
use cdrom::reader::CdRom;
use genesis_components::cartridge::Cartridge;
use genesis_components::debug::GenesisDebugger;
use genesis_components::vdp::VdpTickEffect;
use genesis_config::{GenesisButton, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, InputPoller, Modal, PartialClone, Renderer, SaveWriter, TickEffect,
    TickResult, TimingMode,
};
use jgenesis_proc_macros::EnumDisplay;
use m68000_emu::M68000;
use segacd_core::api::{SegaCd, SegaCdLoadError, SegaCdLoadResult};
use std::fmt::{Debug, Display};
use thiserror::Error;
use z80_emu::Z80;

const SRAM_EXTENSION: &str = "sav";
const BACKUP_RAM_EXTENSION: &str = "bram";
const RAM_CARTRIDGE_EXTENSION: &str = "ramc";

#[derive(Debug, Error)]
pub enum GenesisError<RErr, AErr, SErr> {
    #[error("Sega CD disc error: {0}")]
    SegaCd(#[from] SegaCdLoadError),
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    Save(SErr),
}

pub type GenesisResult<RErr, AErr, SErr> = Result<TickEffect, GenesisError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumDisplay)]
pub enum GenesisHardware {
    Standalone,
    SegaCd,
    Sega32X,
    SegaCd32X,
}

impl GenesisHardware {
    pub fn has_sega_cd(self) -> bool {
        matches!(self, Self::SegaCd | Self::SegaCd32X)
    }

    pub fn has_32x(self) -> bool {
        matches!(self, Self::Sega32X | Self::SegaCd32X)
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct GenesisEmulator {
    m68k: M68000,
    z80: Z80,
    #[partial_clone(partial)]
    bus: GenesisBus,
    timing_mode: TimingMode,
    audio_resampler: GenesisAudioResampler,
    hardware: GenesisHardware,
    config: GenesisEmulatorConfig,
}

impl GenesisEmulator {
    pub fn create<S: SaveWriter>(
        hardware: GenesisHardware,
        cartridge_rom: Option<Vec<u8>>,
        sega_cd_bios_rom: Option<Vec<u8>>,
        mut disc: Option<CdRom>,
        config: GenesisEmulatorConfig,
        save_writer: &mut S,
    ) -> SegaCdLoadResult<Self> {
        let sega_cd_present = hardware.has_sega_cd();
        let s32x_present = hardware.has_32x();

        let initial_ram = save_writer.load_bytes(SRAM_EXTENSION).ok();

        let cartridge = cartridge_rom
            .map(|rom| Cartridge::new(rom, initial_ram, config.forced_region, &config.cheat_codes));

        let region = cartridge
            .as_ref()
            .map(Cartridge::region)
            .or_else(|| {
                let disc = disc.as_mut()?;
                segacd_core::api::parse_disc_region(disc).ok()
            })
            .unwrap_or(GenesisRegion::Americas);

        let timing_mode = config.forced_timing_mode.unwrap_or_else(|| match region {
            GenesisRegion::Europe => TimingMode::Pal,
            GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
        });

        log::info!("Using timing / display mode {timing_mode}");

        let sega_cd = if sega_cd_present {
            let Some(bios_rom) = sega_cd_bios_rom else { return Err(SegaCdLoadError::MissingBios) };

            let backup_ram_extension = match &cartridge {
                Some(_) => BACKUP_RAM_EXTENSION,
                None => SRAM_EXTENSION,
            };
            let initial_backup_ram = save_writer.load_bytes(backup_ram_extension).ok();

            let initial_ram_cartridge = save_writer.load_bytes(RAM_CARTRIDGE_EXTENSION).ok();

            let sega_cd = SegaCd::new(
                bios_rom,
                disc,
                timing_mode,
                initial_backup_ram,
                initial_ram_cartridge,
                &config,
            )?;
            Some(sega_cd)
        } else {
            None
        };

        let bus = GenesisBus::new(timing_mode, cartridge, sega_cd, &config);

        // The Genesis does not allow TAS to lock the bus, so don't allow TAS writes
        let m68k = M68000::builder().allow_tas_writes(false).name("Main".into()).build();
        let z80 = Z80::new();

        let audio_resampler = GenesisAudioResampler::new(hardware, timing_mode, &config);

        let mut emulator = Self { m68k, z80, bus, timing_mode, audio_resampler, hardware, config };

        // Reset CPU so that execution will start from the right place
        emulator.bus.m68k_reset = true;
        emulator.m68k.execute_instruction(&mut emulator.bus);
        emulator.bus.m68k_reset = false;

        Ok(emulator)
    }

    #[must_use]
    pub fn cartridge_title(&mut self) -> String {
        self.bus.game_title().unwrap_or_default()
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.bus.has_persistent_ram()
    }

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        genesis_components::render_frame(self.timing_mode, &self.bus.vdp, &self.config, renderer)
    }

    #[inline]
    fn tick_inner<const DEBUG: bool, R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
        mut debugger: Option<&mut GenesisDebugger>,
    ) -> TickResult<GenesisError<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        I: InputPoller<GenesisInputs>,
        S: SaveWriter,
    {
        self.bus.input.set_inputs(*input_poller.poll());
        self.bus.m68k_opcode = self.m68k.next_opcode();

        let m68k_pc = self.m68k.pc();
        let m68k_wait = self.bus.cycles.m68k_wait_cpu_cycles != 0;
        let m68k_cycles = if m68k_wait {
            self.bus.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.m68k.execute_instruction(&mut self.bus)
        };

        let elapsed_mclk_cycles = self.bus.cycles.record_68k_instruction(
            m68k_pc,
            m68k_cycles,
            m68k_wait,
            self.bus.vdp.should_halt_cpu(),
        );

        while self.bus.cycles.should_tick_z80() {
            if !self.bus.cycles.z80_halt {
                self.z80.tick(&mut self.bus);
            }
            self.bus.cycles.z80_cycle();
        }

        let vdp_tick_effect = self.bus.tick_components(
            m68k_cycles,
            elapsed_mclk_cycles,
            m68k_wait,
            &mut self.audio_resampler,
        )?;

        self.audio_resampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

        if vdp_tick_effect == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GenesisError::Render)?;

            if self.bus.has_persistent_ram() && self.bus.get_and_clear_persistent_ram_dirty() {
                if let Some(ram) = self.bus.cartridge.as_ref().map(Cartridge::external_ram)
                    && !ram.is_empty()
                {
                    save_writer.persist_bytes(SRAM_EXTENSION, ram).map_err(GenesisError::Save)?;
                }

                if let Some(sega_cd) = &mut self.bus.sega_cd {
                    let backup_ram_extension = match &self.bus.cartridge {
                        Some(_) => BACKUP_RAM_EXTENSION,
                        None => SRAM_EXTENSION,
                    };
                    save_writer
                        .persist_bytes(backup_ram_extension, sega_cd.backup_ram())
                        .map_err(GenesisError::Save)?;

                    save_writer
                        .persist_bytes(RAM_CARTRIDGE_EXTENSION, sega_cd.ram_cartridge())
                        .map_err(GenesisError::Save)?;
                }
            }
        }

        Ok(match vdp_tick_effect {
            VdpTickEffect::FrameComplete => TickEffect::FrameRendered,
            VdpTickEffect::None => TickEffect::None,
        })
    }

    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames or pushing audio
    /// samples.
    pub fn debug_tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
        debugger: &mut GenesisDebugger,
    ) -> TickResult<GenesisError<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        I: InputPoller<GenesisInputs>,
        S: SaveWriter,
    {
        self.tick_inner::<true, _, _, _, _>(
            renderer,
            audio_output,
            input_poller,
            save_writer,
            Some(debugger),
        )
    }
}

impl EmulatorTrait for GenesisEmulator {
    type Button = GenesisButton;
    type Inputs = GenesisInputs;
    type Config = GenesisEmulatorConfig;
    type SaveState = Self;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = GenesisError<RErr, AErr, SErr>;

    /// Execute one 68000 CPU instruction and run the rest of the components for the appropriate
    /// number of cycles.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames or pushing audio
    /// samples.
    #[inline]
    fn tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
    ) -> GenesisResult<R::Err, A::Err, S::Err>
    where
        R: Renderer,
        A: AudioOutput,
        I: InputPoller<Self::Inputs>,
        S: SaveWriter,
    {
        self.tick_inner::<false, _, _, _, _>(
            renderer,
            audio_output,
            input_poller,
            save_writer,
            None,
        )
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.bus.reload_config(config);
        self.audio_resampler.reload_config(self.timing_mode, config);

        self.config = config.clone();
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        self.bus.m68k_reset = true;
        self.m68k.execute_instruction(&mut self.bus);
        self.bus.m68k_reset = false;

        self.bus.reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        log::info!("Hard resetting console");

        let cartridge_rom = self.bus.cartridge.take().map(|mut cartridge| cartridge.take_rom());

        let (sega_cd_bios, disc) =
            match self.bus.sega_cd.take().map(|sega_cd| sega_cd.take_bios_and_disc()) {
                Some((bios_rom, disc)) => (Some(bios_rom), disc),
                None => (None, None),
            };

        *self = GenesisEmulator::create(
            self.hardware,
            cartridge_rom,
            sega_cd_bios,
            disc,
            self.config.clone(),
            save_writer,
        )
        .expect("Hard reset should not error");
    }

    fn load_state(&mut self, mut state: Self::SaveState) {
        // TODO Sega CD
        if let Some(state_cartridge) = &mut state.bus.cartridge
            && let Some(self_cartridge) = &mut self.bus.cartridge
        {
            state_cartridge.take_rom_from(self_cartridge);
        }
        *self = state;
    }

    fn to_save_state(&self) -> Self::SaveState {
        self.partial_clone()
    }

    fn target_fps(&self) -> f64 {
        genesis_components::target_framerate(&self.bus.vdp, self.timing_mode)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }

    fn startup_modals(&self) -> Vec<Modal> {
        let mut modals = Vec::new();

        // TODO Sega CD
        if self.config.remove_sprite_limits
            && self
                .bus
                .cartridge
                .as_ref()
                .is_some_and(|cartridge| cartridge.metadata().sprite_limit_compatibility_issues)
        {
            modals.push(Modal {
                id: None,
                text: genesis_components::SPRITE_LIMITS_MODAL_MESSAGE.into(),
            });
        }

        modals
    }
}
