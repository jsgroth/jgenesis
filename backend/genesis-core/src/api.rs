//! Genesis public interface and main loop

use crate::audio::GenesisAudioResampler;
use bincode::{Decode, Encode};
use genesis_components::GenesisEmulatorConfigExt;
use genesis_components::cartridge::Cartridge;
use genesis_components::debug::{CartridgeDebugView, GenesisDebugger, GenesisEmulatorDebugView};
use genesis_components::input::InputState;
use genesis_components::memory::debug::DebugMainBus;
use genesis_components::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_components::timing::GenesisCycleCounters;
use genesis_components::vdp::{DarkenColors, Vdp, VdpTickEffect};
use genesis_components::ym2612::Ym2612;
use genesis_config::{GenesisButton, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, InputPoller, Modal, PartialClone, Renderer, SaveWriter, TickEffect,
    TickResult, TimingMode,
};
use m68000_emu::M68000;
use std::fmt::{Debug, Display};
use thiserror::Error;
use ti_sn76489::{Sn76489, Sn76489TickEffect, Sn76489Version};
use z80_emu::Z80;

#[derive(Debug, Error)]
pub enum GenesisError<RErr, AErr, SErr> {
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    Save(SErr),
}

pub type GenesisResult<RErr, AErr, SErr> = Result<TickEffect, GenesisError<RErr, AErr, SErr>>;

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct GenesisEmulator {
    #[partial_clone(partial)]
    memory: Memory<Cartridge>,
    m68k: M68000,
    z80: Z80,
    vdp: Vdp,
    psg: Sn76489,
    ym2612: Ym2612,
    input: InputState,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    audio_resampler: GenesisAudioResampler,
    cycles: GenesisCycleCounters,
    config: GenesisEmulatorConfig,
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
            $self.m68k.next_opcode(),
            $self.timing_mode,
            MainBusSignals { m68k_reset: $m68k_reset },
            std::mem::take(&mut $self.main_bus_writes),
        )
    };
}

impl GenesisEmulator {
    /// Initialize the emulator from the given ROM.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to parse the ROM header.
    #[must_use]
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: GenesisEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let initial_ram = save_writer.load_bytes("sav").ok();
        let cartridge = Cartridge::new(rom, initial_ram, config.forced_region, &config.cheat_codes);
        let memory = Memory::new(cartridge, &config);

        let timing_mode =
            config.forced_timing_mode.unwrap_or_else(|| match memory.hardware_region() {
                GenesisRegion::Europe => TimingMode::Pal,
                GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
            });

        log::info!("Using timing / display mode {timing_mode}");

        let z80 = Z80::new();
        let vdp = Vdp::new(timing_mode, config.to_vdp_config(DarkenColors::No));
        let psg = Sn76489::new(Sn76489Version::Standard);
        let ym2612 = Ym2612::new(&config);
        let input = InputState::new(&config, memory.medium().metadata().six_button_incompatible);

        // The Genesis does not allow TAS to lock the bus, so don't allow TAS writes
        let m68k = M68000::builder().allow_tas_writes(false).build();

        let mut emulator = Self {
            memory,
            m68k,
            z80,
            vdp,
            psg,
            ym2612,
            input,
            timing_mode,
            main_bus_writes: MainBusWrites::new(),
            audio_resampler: GenesisAudioResampler::new(timing_mode, &config),
            cycles: GenesisCycleCounters::new(config.clamped_m68k_divider()),
            config,
        };

        // Reset CPU so that execution will start from the right place
        emulator.m68k.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        emulator
    }

    #[must_use]
    pub fn cartridge_title(&self) -> String {
        self.memory.game_title()
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.memory.is_external_ram_persistent()
    }

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        genesis_components::render_frame(self.timing_mode, &self.vdp, &self.config, renderer)
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
        self.input.set_inputs(*input_poller.poll());

        let mut bus = new_main_bus!(self, m68k_reset: false);
        let m68k_pc = self.m68k.pc();
        let m68k_wait = bus.cycles.m68k_wait_cpu_cycles != 0;
        let m68k_cycles = if m68k_wait {
            bus.cycles.take_m68k_wait_cpu_cycles()
        } else if DEBUG && let Some(debugger) = &mut debugger {
            let mut debug_bus =
                DebugMainBus { bus: &mut bus, debugger: debugger.for_68k(&mut self.z80) };
            self.m68k.execute_instruction(&mut debug_bus)
        } else {
            self.m68k.execute_instruction(&mut bus)
        };

        let elapsed_mclk_cycles = bus.cycles.record_68k_instruction(
            m68k_pc,
            m68k_cycles,
            m68k_wait,
            bus.vdp.should_halt_cpu(),
        );

        while bus.cycles.should_tick_z80() {
            if !bus.cycles.z80_halt {
                if DEBUG && let Some(debugger) = &mut debugger {
                    let mut debug_bus =
                        DebugMainBus { bus: &mut bus, debugger: debugger.for_z80(&mut self.m68k) };
                    self.z80.tick(&mut debug_bus);
                } else {
                    self.z80.tick(&mut bus);
                }
            }
            bus.cycles.z80_cycle();
        }

        self.main_bus_writes = bus.pending_writes;

        self.memory.medium_mut().tick(m68k_cycles);

        self.input.tick(m68k_cycles);

        while self.cycles.should_tick_psg() {
            if self.psg.tick() == Sn76489TickEffect::Clocked {
                // PSG only has mono output in the Genesis; stereo output is only for Game Gear
                let (psg_sample, _) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample);
            }

            self.cycles.psg_cycle();
        }

        let vdp_tick_effect = self.vdp.tick(elapsed_mclk_cycles, &mut self.memory);

        self.cycles.maybe_sync_and_drain_ym2612(
            vdp_tick_effect,
            &mut self.ym2612,
            |(sample_l, sample_r)| self.audio_resampler.collect_ym2612_sample(sample_l, sample_r),
        );

        self.audio_resampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

        let mut tick_effect = TickEffect::None;
        if vdp_tick_effect == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GenesisError::Render)?;

            if self.memory.is_external_ram_persistent()
                && self.memory.get_and_clear_external_ram_dirty()
            {
                let ram = self.memory.external_ram();
                if !ram.is_empty() {
                    save_writer.persist_bytes("sav", ram).map_err(GenesisError::Save)?;
                }
            }

            tick_effect = TickEffect::FrameRendered;
        }

        genesis_components::check_for_long_dma_skip(&self.vdp, &mut self.cycles);

        if !m68k_wait {
            self.vdp.update_interrupt_latches();
        }

        self.main_bus_writes = new_main_bus!(self, m68k_reset: false).apply_writes();

        Ok(tick_effect)
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

    #[must_use]
    pub fn as_debug_view(&mut self) -> GenesisEmulatorDebugView<'_> {
        GenesisEmulatorDebugView {
            m68k: &mut self.m68k,
            z80: &mut self.z80,
            memory: self.memory.as_debug_view(|cartridge| CartridgeDebugView { cartridge }),
            pending_writes: &self.main_bus_writes,
            vdp: &mut self.vdp,
            ym2612: &mut self.ym2612,
            psg: &mut self.psg,
        }
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
        self.vdp.reload_config(config.to_vdp_config(DarkenColors::No));
        self.ym2612.reload_config(config);
        self.memory.reload_config(config);
        self.memory.medium_mut().reload_config(config);
        self.input.reload_config(config);
        self.audio_resampler.reload_config(self.timing_mode, config);
        self.cycles.update_m68k_divider(config.clamped_m68k_divider());

        self.config = config.clone();
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        self.m68k.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();
        self.ym2612.reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        log::info!("Hard resetting console");

        let rom = self.memory.take_rom();
        *self = GenesisEmulator::create(rom, self.config.clone(), save_writer);
    }

    fn load_state(&mut self, mut state: Self::SaveState) {
        state.memory.take_rom_from(&mut self.memory);
        *self = state;
    }

    fn to_save_state(&self) -> Self::SaveState {
        self.partial_clone()
    }

    fn target_fps(&self) -> f64 {
        genesis_components::target_framerate(&self.vdp, self.timing_mode)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }

    fn startup_modals(&self) -> Vec<Modal> {
        let mut modals = Vec::new();

        if self.config.remove_sprite_limits
            && self.memory.medium().metadata().sprite_limit_compatibility_issues
        {
            modals.push(Modal {
                id: None,
                text: genesis_components::SPRITE_LIMITS_MODAL_MESSAGE.into(),
            });
        }

        modals
    }
}
