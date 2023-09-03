use crate::audio;
use crate::audio::AudioDownsampler;
use crate::input::{GenesisInputs, InputState};
use crate::memory::{Cartridge, CartridgeLoadError, MainBus, Memory};
use crate::vdp::{Vdp, VdpTickEffect};
use crate::ym2612::{Ym2612, YmTickEffect};
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_traits::frontend::{
    AudioOutput, EmulatorTrait, FrameSize, PixelAspectRatio, Renderer, Resettable, SaveWriter,
    TakeRomFrom, TickEffect, TickableEmulator,
};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgVersion};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use z80_emu::Z80;

const M68K_MCLK_DIVIDER: u64 = 7;
const Z80_MCLK_DIVIDER: u64 = 15;

#[derive(Debug)]
pub enum GenesisError<RErr, AErr, SErr> {
    Render(RErr),
    Audio(AErr),
    Save(SErr),
}

impl<RErr, AErr, SErr> Display for GenesisError<RErr, AErr, SErr>
where
    RErr: Display,
    AErr: Display,
    SErr: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(err) => write!(f, "Rendering error: {err}"),
            Self::Audio(err) => write!(f, "Audio error: {err}"),
            Self::Save(err) => write!(f, "Save error: {err}"),
        }
    }
}

impl<RErr, AErr, SErr> Error for GenesisError<RErr, AErr, SErr>
where
    RErr: Debug + Display + AsRef<dyn Error + 'static>,
    AErr: Debug + Display + AsRef<dyn Error + 'static>,
    SErr: Debug + Display + AsRef<dyn Error + 'static>,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Render(err) => Some(err.as_ref()),
            Self::Audio(err) => Some(err.as_ref()),
            Self::Save(err) => Some(err.as_ref()),
        }
    }
}

pub type GenesisResult<RErr, AErr, SErr> = Result<TickEffect, GenesisError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GenesisAspectRatio {
    #[default]
    Ntsc,
    SquarePixels,
    Stretched,
}

impl GenesisAspectRatio {
    fn to_pixel_aspect_ratio(
        self,
        frame_size: FrameSize,
        adjust_for_2x_resolution: bool,
    ) -> Option<PixelAspectRatio> {
        let mut pixel_aspect_ratio = match (self, frame_size.width) {
            (Self::SquarePixels, _) => Some(1.0),
            (Self::Stretched, _) => None,
            (Self::Ntsc, 256) => Some(8.0 / 7.0),
            (Self::Ntsc, 320) => Some(32.0 / 35.0),
            (Self::Ntsc, _) => panic!("unexpected Genesis frame width: {}", frame_size.width),
        };

        if adjust_for_2x_resolution && frame_size.height == 448 {
            pixel_aspect_ratio = pixel_aspect_ratio.map(|par| par * 2.0);
        }

        pixel_aspect_ratio.map(|par| PixelAspectRatio::try_from(par).unwrap())
    }
}

#[derive(Debug, Clone)]
pub struct GenesisEmulatorConfig {
    pub aspect_ratio: GenesisAspectRatio,
    pub adjust_aspect_ratio_in_2x_resolution: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GenesisEmulator {
    memory: Memory,
    m68k: M68000,
    z80: Z80,
    vdp: Vdp,
    psg: Psg,
    ym2612: Ym2612,
    input: InputState,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    audio_downsampler: AudioDownsampler,
    master_clock_cycles: u64,
}

impl GenesisEmulator {
    /// Initialize the emulator from the given ROM.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to parse the ROM header.
    pub fn create(
        rom: Vec<u8>,
        initial_ram: Option<Vec<u8>>,
        config: GenesisEmulatorConfig,
    ) -> Result<Self, CartridgeLoadError> {
        let cartridge = Cartridge::from_rom(rom, initial_ram)?;
        let mut memory = Memory::new(cartridge);

        let z80 = Z80::new();
        let mut vdp = Vdp::new();
        let mut psg = Psg::new(PsgVersion::Standard);
        let mut ym2612 = Ym2612::new();
        let mut input = InputState::new();

        let mut m68k = M68000::new();
        m68k.reset(&mut MainBus::new(
            &mut memory,
            &mut vdp,
            &mut psg,
            &mut ym2612,
            &mut input,
            z80.stalled(),
        ));

        Ok(Self {
            memory,
            m68k,
            z80,
            vdp,
            psg,
            ym2612,
            input,
            aspect_ratio: config.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: config.adjust_aspect_ratio_in_2x_resolution,
            audio_downsampler: AudioDownsampler::new(),
            master_clock_cycles: 0,
        })
    }

    pub fn reload_config(&mut self, config: GenesisEmulatorConfig) {
        self.aspect_ratio = config.aspect_ratio;
        self.adjust_aspect_ratio_in_2x_resolution = config.adjust_aspect_ratio_in_2x_resolution;
    }

    #[must_use]
    pub fn cartridge_title(&self) -> String {
        self.memory.cartridge_title()
    }
}

impl TakeRomFrom for GenesisEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
    }
}

impl TickableEmulator for GenesisEmulator {
    type Inputs = GenesisInputs;
    type Err<RErr, AErr, SErr> = GenesisError<RErr, AErr, SErr>;

    /// Execute one 68000 CPU instruction and run the rest of the components for the appropriate
    /// number of cycles.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames or pushing audio
    /// samples.
    #[inline]
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> GenesisResult<R::Err, A::Err, S::Err>
    where
        R: Renderer,
        A: AudioOutput,
        S: SaveWriter,
    {
        let mut bus = MainBus::new(
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            &mut self.ym2612,
            &mut self.input,
            self.z80.stalled(),
        );
        let m68k_cycles = self.m68k.execute_instruction(&mut bus);

        let elapsed_mclk_cycles = u64::from(m68k_cycles) * M68K_MCLK_DIVIDER;
        let z80_cycles = ((self.master_clock_cycles + elapsed_mclk_cycles) / Z80_MCLK_DIVIDER)
            - self.master_clock_cycles / Z80_MCLK_DIVIDER;
        self.master_clock_cycles += elapsed_mclk_cycles;

        for _ in 0..z80_cycles {
            self.z80.tick(&mut bus);
        }

        // The PSG uses the same master clock divider as the Z80, but it needs to be ticked in a
        // separate loop because MainBus holds a mutable reference to the PSG
        for _ in 0..z80_cycles {
            self.psg.tick();
        }

        // The YM2612 uses the same master clock divider as the 68000
        for _ in 0..m68k_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (ym_sample_l, ym_sample_r) = self.ym2612.sample();
                let (psg_sample_l, psg_sample_r) = self.psg.sample();

                // TODO more intelligent PSG mixing
                let sample_l =
                    (ym_sample_l + audio::PSG_COEFFICIENT * psg_sample_l).clamp(-1.0, 1.0);
                let sample_r =
                    (ym_sample_r + audio::PSG_COEFFICIENT * psg_sample_r).clamp(-1.0, 1.0);
                self.audio_downsampler
                    .collect_sample(sample_l, sample_r, audio_output)
                    .map_err(GenesisError::Audio)?;
            }
        }

        if self.vdp.tick(elapsed_mclk_cycles, &self.memory) == VdpTickEffect::FrameComplete {
            let frame_width = self.vdp.screen_width();
            let frame_height = self.vdp.screen_height();

            let frame_size = FrameSize { width: frame_width, height: frame_height };
            let pixel_aspect_ratio = self
                .aspect_ratio
                .to_pixel_aspect_ratio(frame_size, self.adjust_aspect_ratio_in_2x_resolution);

            renderer
                .render_frame(self.vdp.frame_buffer(), frame_size, pixel_aspect_ratio)
                .map_err(GenesisError::Render)?;

            self.input.set_inputs(inputs);

            if self.memory.cartridge_ram_persistent() && self.memory.cartridge_ram_dirty() {
                self.memory.clear_cartridge_ram_dirty();

                if let Some(ram) = self.memory.cartridge_ram() {
                    save_writer.persist_save(ram).map_err(GenesisError::Save)?;
                }
            }

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }
}

impl Resettable for GenesisEmulator {
    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        self.m68k.reset(&mut MainBus::new(
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            &mut self.ym2612,
            &mut self.input,
            false,
        ));
        self.memory.reset_z80_signals();
        self.ym2612.reset();
    }

    fn hard_reset(&mut self) {
        log::info!("Hard resetting console");

        let rom = self.memory.take_rom();
        let cartridge_ram = self.memory.take_cartridge_ram_if_persistent();
        let config = GenesisEmulatorConfig {
            aspect_ratio: self.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
        };

        *self = GenesisEmulator::create(rom, cartridge_ram, config).unwrap();
    }
}

impl EmulatorTrait<GenesisInputs> for GenesisEmulator {}
