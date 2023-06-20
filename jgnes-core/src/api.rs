use crate::apu::ApuState;
use crate::bus::cartridge::CartridgeFileError;
use crate::bus::{cartridge, Bus, PpuBus, TimingMode};
use crate::cpu::{CpuError, CpuRegisters, CpuState};
use crate::input::JoypadState;
use crate::ppu::{FrameBuffer, PpuState};
use crate::serialize::SaveStateError;
use crate::{apu, cpu, ppu, serialize};
use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::rc::Rc;
use thiserror::Error;

// The number of master clock ticks to run in one `Emulator::tick` call
const PAL_MASTER_CLOCK_TICKS: u32 = 80;

const PAL_CPU_DIVIDER: u32 = 16;
const PAL_PPU_DIVIDER: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorEmphasis {
    pub red: bool,
    pub green: bool,
    pub blue: bool,
}

impl ColorEmphasis {
    fn get_current(bus: &PpuBus<'_>) -> Self {
        let ppu_registers = bus.get_ppu_registers();
        Self {
            red: ppu_registers.emphasize_red(),
            green: ppu_registers.emphasize_green(),
            blue: ppu_registers.emphasize_blue(),
        }
    }
}

impl Display for ColorEmphasis {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ColorEmphasis[R={}, G={}, B={}]",
            self.red, self.green, self.blue
        )
    }
}

pub trait Renderer {
    type Err;

    /// Render a completed frame. This will be called once per frame, immediately after the NES PPU
    /// enters vertical blanking. Implementations should assume that the entire frame has changed
    /// every time this method is called.
    ///
    /// The frame buffer is a 256x240 grid, with each cell in the grid holding a 6-bit NES color
    /// (0-63). Implementations are responsible for mapping these colors into an appropriate color
    /// space for display (e.g. RGB).
    ///
    /// The R/G/B color emphasis bits are included directly from the NES PPU. It is up to
    /// implementations to choose how to apply these.
    ///
    /// Note that while the NES's internal resolution is 256x240, the visible vertical resolution
    /// depends on whether the emulated console is running in NTSC mode or PAL mode. If in NTSC
    /// mode, the renderer should display at most 224px vertically, removing the top 8 and bottom 8
    /// rows of pixels. If in PAL mode, the renderer should display the full 240px.
    ///
    /// # Errors
    ///
    /// This method can return an error if it is unable to render a frame, and the error will be
    /// propagated.
    fn render_frame(
        &mut self,
        frame_buffer: &FrameBuffer,
        color_emphasis: ColorEmphasis,
    ) -> Result<(), Self::Err>;

    /// Set the timing mode (NTSC/PAL). This will be called whenever a new ROM is loaded, although
    /// renderers should default to NTSC if it is not called.
    ///
    /// Renderers need to be aware of timing mode because the visible screen height is different
    /// between NTSC (224px) and PAL (240px), but the NES frame buffer is 256x240 in both cases.
    ///
    /// # Errors
    ///
    /// This method can return an error if it is unable to initialize some inner state, and the
    /// error will be propagated.
    fn set_timing_mode(&mut self, timing_mode: TimingMode) -> Result<(), Self::Err>;
}

impl<R: Renderer> Renderer for Rc<RefCell<R>> {
    type Err = R::Err;

    fn render_frame(
        &mut self,
        frame_buffer: &FrameBuffer,
        color_emphasis: ColorEmphasis,
    ) -> Result<(), Self::Err> {
        self.borrow_mut().render_frame(frame_buffer, color_emphasis)
    }

    fn set_timing_mode(&mut self, timing_mode: TimingMode) -> Result<(), Self::Err> {
        self.borrow_mut().set_timing_mode(timing_mode)
    }
}

pub trait AudioPlayer {
    type Err;

    /// Process an audio sample.
    ///
    /// Samples are provided as raw 64-bit floating-point PCM samples directly from the NES APU, at
    /// the APU's clock speed of 1.789773 MHz (or more precisely, 236.25 MHz / 132). Implementations
    /// are responsible for downsampling to a frequency that the audio device can play.
    ///
    /// All samples will be in the range \[-1.0, 1.0\].
    ///
    /// # Errors
    ///
    /// This method can return an error if it is unable to play audio, and the error will be
    /// propagated.
    fn push_sample(&mut self, sample: f64) -> Result<(), Self::Err>;
}

impl<A: AudioPlayer> AudioPlayer for Rc<RefCell<A>> {
    type Err = A::Err;

    fn push_sample(&mut self, sample: f64) -> Result<(), Self::Err> {
        self.borrow_mut().push_sample(sample)
    }
}

pub trait InputPoller {
    /// Retrieve the current Player 1 input state.
    fn poll_p1_input(&self) -> JoypadState;

    /// Retrieve the current Player 2 input state.
    ///
    /// If only one input device is desired, implementations can have this method return
    /// `JoypadState::default()`.
    fn poll_p2_input(&self) -> JoypadState;
}

impl<I: InputPoller> InputPoller for Rc<I> {
    fn poll_p1_input(&self) -> JoypadState {
        I::poll_p1_input(self)
    }

    fn poll_p2_input(&self) -> JoypadState {
        I::poll_p2_input(self)
    }
}

impl<I: InputPoller> InputPoller for RefCell<I> {
    fn poll_p1_input(&self) -> JoypadState {
        self.borrow().poll_p1_input()
    }

    fn poll_p2_input(&self) -> JoypadState {
        self.borrow().poll_p2_input()
    }
}

pub trait SaveWriter {
    type Err;

    /// Optionally persist the contents of non-volatile PRG RAM, which generally contains save data.
    ///
    /// This method will only be called when running games that have battery-backed PRG RAM.
    /// Additionally, it will only be called when the contents of PRG RAM have changed since the
    /// last time this method was called.
    ///
    /// # Errors
    ///
    /// This method can return an error if it is unable to persist the data to whatever it is
    /// writing to, and the error will be propagated.
    fn persist_sram(&mut self, sram: &[u8]) -> Result<(), Self::Err>;
}

impl<S: SaveWriter> SaveWriter for Rc<RefCell<S>> {
    type Err = S::Err;

    fn persist_sram(&mut self, sram: &[u8]) -> Result<(), Self::Err> {
        self.borrow_mut().persist_sram(sram)
    }
}

#[derive(Debug)]
pub enum EmulationError<RenderError, AudioError, SaveError> {
    Render(RenderError),
    Audio(AudioError),
    Save(SaveError),
    CpuInvalidOpcode(u8),
}

impl<RenderError, AudioError, SaveError> From<CpuError>
    for EmulationError<RenderError, AudioError, SaveError>
{
    fn from(value: CpuError) -> Self {
        match value {
            CpuError::InvalidOpcode(opcode) => Self::CpuInvalidOpcode(opcode),
        }
    }
}

impl<R: Display, A: Display, S: Display> Display for EmulationError<R, A, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(err) => write!(f, "Rendering error: {err}"),
            Self::Audio(err) => write!(f, "Audio error: {err}"),
            Self::Save(err) => write!(f, "Save error: {err}"),
            Self::CpuInvalidOpcode(opcode) => {
                write!(f, "CPU executed invalid/unsupported opcode: ${opcode:02X}")
            }
        }
    }
}

impl<R: Error + 'static, A: Error + 'static, S: Error + 'static> Error for EmulationError<R, A, S> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Render(err) => Some(err),
            Self::Audio(err) => Some(err),
            Self::Save(err) => Some(err),
            Self::CpuInvalidOpcode(..) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorConfig {
    /// If true, silence the triangle wave channel when it is outputting a wave at ultrasonic frequency
    pub silence_ultrasonic_triangle_output: bool,
}

pub struct EmulationState {
    pub(crate) bus: Bus,
    pub(crate) cpu_state: CpuState,
    pub(crate) ppu_state: PpuState,
    pub(crate) apu_state: ApuState,
}

pub struct Emulator<Renderer, AudioPlayer, InputPoller, SaveWriter> {
    bus: Bus,
    cpu_state: CpuState,
    ppu_state: PpuState,
    apu_state: ApuState,
    renderer: Renderer,
    audio_player: AudioPlayer,
    input_poller: InputPoller,
    save_writer: SaveWriter,
    // Kept around to enable hard reset
    raw_rom_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickEffect {
    None,
    FrameRendered,
}

pub type EmulationResult<RenderError, AudioError, SaveError> =
    Result<TickEffect, EmulationError<RenderError, AudioError, SaveError>>;

type UnitEmulationResult<RenderError, AudioError, SaveError> =
    Result<(), EmulationError<RenderError, AudioError, SaveError>>;

#[derive(Debug, Error)]
pub enum InitializationError<RenderError> {
    #[error("Error loading cartridge ROM: {source}")]
    CartridgeLoad {
        #[from]
        source: CartridgeFileError,
    },
    #[error("Error initializing renderer: {source}")]
    RendererInit {
        #[source]
        source: RenderError,
    },
}

impl<R: Renderer, A: AudioPlayer, I: InputPoller, S: SaveWriter> Emulator<R, A, I, S>
where
    R::Err: Debug,
{
    /// Create a new emulator instance.
    ///
    /// # Errors
    ///
    /// This function will return an error if it cannot successfully parse NES ROM data out of the
    /// given ROM bytes.
    pub fn create(
        rom_bytes: Vec<u8>,
        sav_bytes: Option<Vec<u8>>,
        mut renderer: R,
        audio_player: A,
        input_poller: I,
        save_writer: S,
    ) -> Result<Self, InitializationError<R::Err>> {
        let mapper = cartridge::from_ines_file(&rom_bytes, sav_bytes)?;
        let timing_mode = mapper.timing_mode();

        renderer
            .set_timing_mode(timing_mode)
            .map_err(|err| InitializationError::RendererInit { source: err })?;

        let mut bus = Bus::from_cartridge(mapper);

        let cpu_registers = CpuRegisters::create(&mut bus.cpu());
        let cpu_state = CpuState::new(cpu_registers);
        let ppu_state = PpuState::new(timing_mode);
        let mut apu_state = ApuState::new(timing_mode);

        init_apu(&mut apu_state, &mut bus);

        Ok(Self {
            bus,
            cpu_state,
            ppu_state,
            apu_state,
            renderer,
            audio_player,
            input_poller,
            save_writer,
            raw_rom_bytes: rom_bytes,
        })
    }

    /// Run the emulator for 1 CPU cycle / 3 PPU cycles (NTSC) or 5 CPU cycles / 16 PPU cycles (PAL).
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering a frame, pushing
    /// audio samples, or persisting SRAM. It will also return an error if the emulated CPU executes
    /// an invalid opcode.
    pub fn tick(&mut self, config: &EmulatorConfig) -> EmulationResult<R::Err, A::Err, S::Err> {
        let prev_in_vblank = self.ppu_state.in_vblank();

        let timing_mode = self.bus.mapper().timing_mode();

        match timing_mode {
            TimingMode::Ntsc => {
                self.ntsc_tick(config)?;
            }
            TimingMode::Pal => {
                self.pal_tick(config)?;
            }
        }

        if !prev_in_vblank && self.ppu_state.in_vblank() {
            let frame_buffer = self.ppu_state.frame_buffer();
            let color_emphasis = ColorEmphasis::get_current(&self.bus.ppu());

            self.renderer
                .render_frame(frame_buffer, color_emphasis)
                .map_err(EmulationError::Render)?;

            let p1_joypad_state = self.input_poller.poll_p1_input();
            self.bus.update_p1_joypad_state(p1_joypad_state);

            let p2_joypad_state = self.input_poller.poll_p2_input();
            self.bus.update_p2_joypad_state(p2_joypad_state);

            if self.bus.mapper_mut().get_and_clear_ram_dirty_bit() {
                let sram = self.bus.mapper().get_prg_ram();
                self.save_writer
                    .persist_sram(sram)
                    .map_err(EmulationError::Save)?;
            }

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn ntsc_tick(
        &mut self,
        config: &EmulatorConfig,
    ) -> UnitEmulationResult<R::Err, A::Err, S::Err> {
        cpu::tick(
            &mut self.cpu_state,
            &mut self.bus.cpu(),
            self.apu_state.is_active_cycle(),
        )?;
        apu::tick(&mut self.apu_state, &mut self.bus.cpu(), config);
        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();
        self.bus.tick_cpu();

        self.bus.poll_interrupt_lines();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();

        self.push_audio_sample()?;

        Ok(())
    }

    fn pal_tick(&mut self, config: &EmulatorConfig) -> UnitEmulationResult<R::Err, A::Err, S::Err> {
        // Both CPU and PPU tick on the first master clock cycle
        cpu::tick(
            &mut self.cpu_state,
            &mut self.bus.cpu(),
            self.apu_state.is_active_cycle(),
        )?;
        apu::tick(&mut self.apu_state, &mut self.bus.cpu(), config);
        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();
        self.bus.tick_cpu();

        self.bus.poll_interrupt_lines();

        self.push_audio_sample()?;

        for i in 1..PAL_MASTER_CLOCK_TICKS {
            if i % PAL_CPU_DIVIDER == 0 {
                cpu::tick(
                    &mut self.cpu_state,
                    &mut self.bus.cpu(),
                    self.apu_state.is_active_cycle(),
                )?;
                apu::tick(&mut self.apu_state, &mut self.bus.cpu(), config);
                self.bus.tick();
                self.bus.tick_cpu();

                self.bus.poll_interrupt_lines();

                self.push_audio_sample()?;
            } else if i % PAL_PPU_DIVIDER == 0 {
                ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
                self.bus.tick();
            }
        }

        Ok(())
    }

    fn push_audio_sample(&mut self) -> UnitEmulationResult<R::Err, A::Err, S::Err> {
        let audio_sample = {
            let sample = self.apu_state.sample();
            let sample = self.bus.mapper().sample_audio(sample);
            self.apu_state.high_pass_filter(sample)
        };
        self.audio_player
            .push_sample(audio_sample)
            .map_err(EmulationError::Audio)
    }

    /// Press the (emulated) reset button.
    ///
    /// Note that just like on a real NES, this leaves some state intact. For a hard reset you
    /// should call `hard_reset`.
    pub fn soft_reset(&mut self) {
        cpu::reset(&mut self.cpu_state, &mut self.bus.cpu());
        apu::reset(&mut self.apu_state, &mut self.bus.cpu());
        ppu::reset(&mut self.ppu_state, &mut self.bus.ppu());

        for _ in 0..10 {
            apu::tick(
                &mut self.apu_state,
                &mut self.bus.cpu(),
                &EmulatorConfig::default(),
            );
            self.bus.tick();
        }
    }

    /// Completely re-initialize all emulation state.
    ///
    /// `sav_bytes` will be used if set, otherwise PRG RAM will be moved from the existing Emulator.
    #[must_use]
    pub fn hard_reset(self, sav_bytes: Option<Vec<u8>>) -> Self {
        let prg_ram = sav_bytes.unwrap_or_else(|| Vec::from(self.bus.mapper().get_prg_ram()));
        Self::create(
            self.raw_rom_bytes,
            Some(prg_ram),
            self.renderer,
            self.audio_player,
            self.input_poller,
            self.save_writer,
        )
        .expect("hard reset should never fail cartridge validation")
    }
}

impl<R: Renderer, A, I, S> Emulator<R, A, I, S> {
    /// Force the emulator to render a frame based on its current state.
    ///
    /// The emulator will naturally render frames as `tick()` is called; this method should only be
    /// called if the caller wants to force the emulator to render a frame outside of normal
    /// operation.
    ///
    /// # Errors
    ///
    /// This method will propagate any error returned by the renderer.
    pub fn force_render(&mut self) -> Result<(), R::Err> {
        let color_emphasis = ColorEmphasis::get_current(&self.bus.ppu());
        let frame_buffer = self.ppu_state.frame_buffer();
        self.renderer.render_frame(frame_buffer, color_emphasis)
    }
}

impl<R, A, I, S> Emulator<R, A, I, S> {
    pub fn get_renderer(&self) -> &R {
        &self.renderer
    }

    pub fn get_renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }

    pub fn get_audio_player_mut(&mut self) -> &mut A {
        &mut self.audio_player
    }

    /// Save current emulation state to the given writer.
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to completely serialize and write state
    /// to the given writer.
    pub fn save_state<Writer>(&self, writer: Writer) -> Result<(), SaveStateError>
    where
        Writer: io::Write,
    {
        serialize::save_state(
            &self.bus,
            &self.cpu_state,
            &self.ppu_state,
            &self.apu_state,
            writer,
        )
    }

    /// Load emulation state from the specified reader.
    ///
    /// # Errors
    ///
    /// This method will return an error if it is unable to completely load or deserialize state
    /// from the given reader.
    ///
    /// This should not be considered a fatal error - for example, deserialization might fail if the
    /// internal state format has changed in an incompatible way due to code changes since the state
    /// was last saved.
    pub fn load_state<Reader>(&mut self, reader: Reader) -> Result<(), SaveStateError>
    where
        Reader: io::Read,
    {
        let state = serialize::load_state(reader)?;

        self.load_state_snapshot(state);

        Ok(())
    }

    /// Retrieve a snapshot of the emulator's current state. This snapshot can later be passed to
    /// `load_state_snapshot` to reset the emulator to that state.
    pub fn snapshot_state(&self) -> EmulationState {
        EmulationState {
            bus: self.bus.clone_without_rom(),
            cpu_state: self.cpu_state.clone(),
            ppu_state: self.ppu_state.clone(),
            apu_state: self.apu_state.clone(),
        }
    }

    /// Reset the emulator's state to a previously saved snapshot.
    pub fn load_state_snapshot(&mut self, mut state: EmulationState) {
        state.bus.move_rom_from(&mut self.bus);

        self.bus = state.bus;
        self.cpu_state = state.cpu_state;
        self.ppu_state = state.ppu_state;
        self.apu_state = state.apu_state;
    }

    /// Return whether the loaded cartridge has some sort of persistent RAM (e.g. SRAM or EEPROM).
    pub fn has_persistent_ram(&self) -> bool {
        self.bus.mapper().has_persistent_ram()
    }
}

fn init_apu(apu_state: &mut ApuState, bus: &mut Bus) {
    // Write 0x00 to JOY2 to reset the frame counter
    bus.cpu().write_address(0x4017, 0x00);
    bus.tick();

    // Run the APU for 10 cycles
    for _ in 0..10 {
        apu::tick(apu_state, &mut bus.cpu(), &EmulatorConfig::default());
        bus.tick();
    }
}
