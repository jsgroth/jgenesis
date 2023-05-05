use crate::apu::{ApuConfig, ApuState};
use crate::bus::cartridge::CartridgeFileError;
use crate::bus::{cartridge, Bus, PpuBus};
use crate::cpu::{CpuRegisters, CpuState};
use crate::input::JoypadState;
use crate::ppu::{FrameBuffer, PpuState};
use crate::{apu, cpu, ppu};
use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Note that while the NES's internal resolution is 256x240, virtually no TVs displayed more
    /// than 224 pixels vertically due to overscan, so just about every implementation will want to
    /// remove the top 8 and bottom 8 rows of pixels to produce a 256x224 frame. Some games may look
    /// better with even more overscan on certain sides of the frame.
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
}

pub trait AudioPlayer {
    type Err;

    /// Push audio samples to the audio device.
    ///
    /// Samples are provided as a single channel of 32-bit floating point PCM samples, and they will
    /// be timed to an output frequency of 48000Hz.
    ///
    /// # Errors
    ///
    /// This method can return an error if it is unable to play audio, and the error will be
    /// propagated.
    fn push_samples(&mut self, samples: &[f32]) -> Result<(), Self::Err>;
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

#[derive(Debug)]
pub enum EmulationError<RenderError, AudioError, SaveError> {
    Render(RenderError),
    Audio(AudioError),
    Save(SaveError),
}

impl<R: Display, A: Display, S: Display> Display for EmulationError<R, A, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(err) => write!(f, "Rendering error: {err}"),
            Self::Audio(err) => write!(f, "Audio error: {err}"),
            Self::Save(err) => write!(f, "Save error: {err}"),
        }
    }
}

impl<R: Error + 'static, A: Error + 'static, S: Error + 'static> Error for EmulationError<R, A, S> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Render(err) => Some(err),
            Self::Audio(err) => Some(err),
            Self::Save(err) => Some(err),
        }
    }
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
}

pub type EmulationResult<RenderError, AudioError, SaveError> =
    Result<(), EmulationError<RenderError, AudioError, SaveError>>;

impl<R: Renderer, A: AudioPlayer, I: InputPoller, S: SaveWriter> Emulator<R, A, I, S> {
    /// Create a new emulator instance.
    ///
    /// # Errors
    ///
    /// This function will return an error if it cannot successfully parse NES ROM data out of the
    /// given ROM bytes.
    pub fn create(
        rom_bytes: Vec<u8>,
        sav_bytes: Option<Vec<u8>>,
        renderer: R,
        audio_player: A,
        input_poller: I,
        save_writer: S,
    ) -> Result<Self, CartridgeFileError> {
        let mapper = cartridge::from_ines_file(&rom_bytes, sav_bytes)?;
        let mut bus = Bus::from_cartridge(mapper);

        let cpu_registers = CpuRegisters::create(&mut bus.cpu());
        let cpu_state = CpuState::new(cpu_registers);
        let ppu_state = PpuState::new();
        let mut apu_state = ApuState::new();

        init_apu(&mut apu_state, &ApuConfig::new(), &mut bus);

        Ok(Self {
            bus,
            cpu_state,
            ppu_state,
            apu_state,
            renderer,
            audio_player,
            input_poller,
            save_writer,
        })
    }

    /// Run the emulator for one CPU cycle / three PPU cycles.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering a frame, pushing
    /// audio samples, or persisting SRAM.
    pub fn tick(&mut self) -> EmulationResult<R::Err, A::Err, S::Err> {
        let prev_in_vblank = self.ppu_state.in_vblank();

        cpu::tick(
            &mut self.cpu_state,
            &mut self.bus.cpu(),
            self.apu_state.is_active_cycle(),
        );
        apu::tick(&mut self.apu_state, &ApuConfig::new(), &mut self.bus.cpu());
        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();
        self.bus.tick_cpu();

        self.bus.poll_interrupt_lines();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();

        ppu::tick(&mut self.ppu_state, &mut self.bus.ppu());
        self.bus.tick();

        if !prev_in_vblank && self.ppu_state.in_vblank() {
            let frame_buffer = self.ppu_state.frame_buffer();
            let color_emphasis = ColorEmphasis::get_current(&self.bus.ppu());

            self.renderer
                .render_frame(frame_buffer, color_emphasis)
                .map_err(EmulationError::Render)?;

            let sample_queue = self.apu_state.get_sample_queue_mut();
            if !sample_queue.is_empty() {
                let samples: Vec<_> = sample_queue.drain(..).collect();
                self.audio_player
                    .push_samples(&samples)
                    .map_err(EmulationError::Audio)?;
            }

            let p1_joypad_state = self.input_poller.poll_p1_input();
            self.bus.update_p1_joypad_state(p1_joypad_state);

            let p2_joypad_state = self.input_poller.poll_p2_input();
            self.bus.update_p2_joypad_state(p2_joypad_state);

            if self.bus.get_and_clear_sram_dirty_bit() {
                let sram = self.bus.get_sram();
                self.save_writer
                    .persist_sram(sram)
                    .map_err(EmulationError::Save)?;
            }
        }

        Ok(())
    }
}

fn init_apu(apu_state: &mut ApuState, apu_config: &ApuConfig, bus: &mut Bus) {
    // Write 0x00 to JOY2 to reset the frame counter
    bus.cpu().write_address(0x4017, 0x00);
    bus.tick();

    // Run the APU for 10 cycles
    for _ in 0..10 {
        apu::tick(apu_state, apu_config, &mut bus.cpu());
        bus.tick();
    }
}
