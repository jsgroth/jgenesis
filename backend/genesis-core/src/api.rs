//! Genesis public interface and main loop

pub mod debug;

use crate::api::debug::GenesisDebugger;
use crate::audio::GenesisAudioResampler;
use crate::bus::GenesisBus;
use crate::bus::debug::{Debug68000Bus, DebugZ80Bus};
use bincode::{Decode, Encode};
use cdrom::reader::CdRom;
use genesis_components::GenesisEmulatorConfigExt;
use genesis_components::cartridge::Cartridge;
use genesis_components::vdp::{Vdp, VdpTickEffect};
use genesis_config::{GenesisButton, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, InputPoller, Modal, PartialClone, RenderFrameOptions, Renderer,
    SaveWriter, TickEffect, TickResult, TimingMode,
};
use m68000_emu::M68000;
use s32x_core::api::Sega32X;
use segacd_core::api::{SegaCd, SegaCdLoadError, SegaCdLoadResult};
use std::fmt::{Debug, Display, Formatter};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum GenesisHardware {
    Standalone,
    SegaCd,
    Sega32X,
    SegaCd32X,
}

impl Display for GenesisHardware {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standalone => write!(f, "Genesis"),
            Self::SegaCd => write!(f, "Sega CD"),
            Self::Sega32X => write!(f, "32X"),
            Self::SegaCd32X => write!(f, "Sega CD 32X"),
        }
    }
}

impl GenesisHardware {
    #[must_use]
    #[inline]
    pub fn has_sega_cd(self) -> bool {
        matches!(self, Self::SegaCd | Self::SegaCd32X)
    }

    #[must_use]
    #[inline]
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
    /// # Errors
    ///
    /// Propagates any errors encountered while initializing Sega CD (if Sega CD is enabled).
    pub fn create<S: SaveWriter>(
        hardware: GenesisHardware,
        mut cartridge_rom: Option<Vec<u8>>,
        sega_cd_bios_rom: Option<Vec<u8>>,
        mut disc: Option<CdRom>,
        config: GenesisEmulatorConfig,
        save_writer: &mut S,
    ) -> SegaCdLoadResult<Self> {
        log::info!("Running with hardware {hardware}");

        if cartridge_rom.as_ref().is_some_and(Vec::is_empty) {
            // Later code assumes cartridge ROM is non-empty if Some
            cartridge_rom = None;
        }

        let sega_cd_present = hardware.has_sega_cd();
        let s32x_present = hardware.has_32x();

        let initial_ram = save_writer.load_bytes(SRAM_EXTENSION).ok();

        let mut cartridge = cartridge_rom
            .map(|rom| Cartridge::new(rom, initial_ram, config.forced_region, &config.cheat_codes));

        let region = cartridge
            .as_ref()
            .map(Cartridge::region)
            .or_else(|| {
                let disc = disc.as_mut()?;
                segacd_core::api::parse_disc_region(disc).ok()
            })
            .unwrap_or(GenesisRegion::Americas);

        let timing_mode = config.forced_timing_mode.unwrap_or(match region {
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

        // If 32X is present, let 32X own the cartridge instead of GenesisBus
        let sega_32x = s32x_present.then(|| Sega32X::new(timing_mode, cartridge.take(), &config));

        let bus = GenesisBus::new(timing_mode, cartridge, sega_cd, sega_32x, &config);

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
    pub fn game_title(&mut self) -> Option<String> {
        self.bus.game_title()
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

    #[inline]
    #[must_use]
    pub fn hardware(&self) -> GenesisHardware {
        self.hardware
    }

    pub fn remove_disc(&mut self) {
        if let Some(sega_cd) = &mut self.bus.sega_cd {
            sega_cd.remove_disc();
        }
    }

    /// # Errors
    ///
    /// Propagates any CD-ROM read errors.
    pub fn change_disc(&mut self, disc: CdRom) -> SegaCdLoadResult<()> {
        if let Some(sega_cd) = &mut self.bus.sega_cd {
            sega_cd.change_disc(disc)?;
        }

        Ok(())
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        match &mut self.bus.sega_32x {
            Some(sega_32x) if sega_32x.adapter_enabled() => {
                sega_32x.render_frame(&self.bus.vdp, renderer)
            }
            _ => render_frame(self.timing_mode, &self.bus.vdp, &self.config, renderer),
        }
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
        } else if DEBUG && let Some(debugger) = &mut debugger {
            let mut debug_bus = Debug68000Bus::new(&mut self.bus, &mut self.z80, debugger);
            self.m68k.execute_instruction(&mut debug_bus)
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
                if DEBUG && let Some(debugger) = &mut debugger {
                    let mut debug_bus = DebugZ80Bus::new(&mut self.bus, &mut self.m68k, debugger);
                    self.z80.tick(&mut debug_bus);
                } else {
                    self.z80.tick(&mut self.bus);
                }
            }
            self.bus.cycles.z80_cycle();
        }

        let vdp_tick_effect = self.bus.tick_components::<DEBUG>(
            m68k_cycles,
            elapsed_mclk_cycles,
            m68k_wait,
            &mut self.audio_resampler,
            debugger.as_mut().map(|debugger| debugger.with_cpus(&mut self.m68k, &mut self.z80)),
        )?;

        self.audio_resampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

        if vdp_tick_effect == VdpTickEffect::FrameComplete {
            if let Some(sega_32x) = &mut self.bus.sega_32x
                && sega_32x.adapter_enabled()
            {
                sega_32x.composite_frame(&mut self.bus.vdp);
            }

            self.render_frame(renderer).map_err(GenesisError::Render)?;

            self.persist_save_files(save_writer).map_err(GenesisError::Save)?;
        }

        Ok(match vdp_tick_effect {
            VdpTickEffect::FrameComplete => TickEffect::FrameRendered,
            VdpTickEffect::None => TickEffect::None,
        })
    }

    fn persist_save_files<S: SaveWriter>(&mut self, save_writer: &mut S) -> Result<(), S::Err> {
        if !self.bus.has_persistent_ram() || !self.bus.get_and_clear_persistent_ram_dirty() {
            return Ok(());
        }

        if let Some(ram) = self.bus.cartridge().map(Cartridge::external_ram)
            && !ram.is_empty()
        {
            save_writer.persist_bytes(SRAM_EXTENSION, ram)?;
        }

        if let Some(sega_cd) = &self.bus.sega_cd {
            // Backup RAM is the primary save location for Sega CD games, so prefer .sav over .bram
            // if there's no game cartridge
            let backup_ram_extension = match self.bus.cartridge() {
                Some(_) => BACKUP_RAM_EXTENSION,
                None => SRAM_EXTENSION,
            };
            save_writer.persist_bytes(backup_ram_extension, sega_cd.backup_ram())?;

            // RAM cartridge is only present when there's no game cartridge (there's only 1 slot)
            if self.bus.cartridge().is_none() {
                save_writer.persist_bytes(RAM_CARTRIDGE_EXTENSION, sega_cd.ram_cartridge())?;
            }
        }

        Ok(())
    }

    /// Similar to [`<Self as EmulatorTrait>::tick`] but runs with debugger hooks enabled.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames or pushing audio
    /// samples.
    #[inline]
    pub fn debug_tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
        debugger: &mut GenesisDebugger,
    ) -> GenesisResult<R::Err, A::Err, S::Err>
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

fn render_frame<R: Renderer>(
    timing_mode: TimingMode,
    vdp: &Vdp,
    config: &GenesisEmulatorConfig,
    renderer: &mut R,
) -> Result<(), R::Err> {
    let frame_size = vdp.frame_size();
    let pixel_aspect_ratio = config.aspect_ratio.to_pixel_aspect_ratio(
        timing_mode,
        frame_size,
        config.to_gen_par_params(),
    );
    let target_fps = genesis_components::target_framerate(vdp, timing_mode);

    renderer.render_frame(
        vdp.frame_buffer(),
        frame_size,
        target_fps,
        RenderFrameOptions {
            pixel_aspect_ratio,
            composite_params: Some(vdp.composite_params()),
            ..RenderFrameOptions::default()
        },
    )
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

        self.bus.reset();

        self.bus.m68k_reset = true;
        self.m68k.execute_instruction(&mut self.bus);
        self.bus.m68k_reset = false;
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        log::info!("Hard resetting console");

        let cartridge_rom = self.bus.cartridge_mut().map(Cartridge::take_rom);

        let (sega_cd_bios, disc) = match self.bus.sega_cd.take().map(SegaCd::take_bios_and_disc) {
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
        if let Some(state_cartridge) = state.bus.cartridge_mut()
            && let Some(self_cartridge) = self.bus.cartridge_mut()
        {
            state_cartridge.take_rom_from(self_cartridge);
        }

        if let Some(state_sega_cd) = &mut state.bus.sega_cd
            && let Some(self_sega_cd) = &mut self.bus.sega_cd
        {
            state_sega_cd.take_bios_and_disc_from(self_sega_cd);
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

        if self.config.remove_sprite_limits
            && (self
                .bus
                .cartridge
                .as_ref()
                .is_some_and(|cartridge| cartridge.metadata().sprite_limit_compatibility_issues)
                || self.bus.sega_cd.as_ref().is_some_and(SegaCd::has_six_button_incompatible_game))
        {
            modals.push(Modal {
                id: None,
                text: genesis_components::SPRITE_LIMITS_MODAL_MESSAGE.into(),
            });
        }

        modals
    }
}
