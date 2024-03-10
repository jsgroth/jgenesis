//! Sega Master System / Game Gear public interface and main loop

use crate::audio::AudioResampler;
use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::{Psg, PsgTickEffect, PsgVersion};
use crate::vdp::{Vdp, VdpBuffer, VdpTickEffect};
use crate::ym2413::Ym2413;
use crate::{vdp, SmsGgInputs, VdpVersion};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, FrameSize, PartialClone, PixelAspectRatio, Renderer,
    SaveWriter, TickEffect, TimingMode,
};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, FakeDecode, FakeEncode};
use std::fmt::{Debug, Display};
use std::ops::{Deref, DerefMut};
use thiserror::Error;
use z80_emu::{InterruptMode, Z80};

#[derive(Debug, Error)]
pub enum SmsGgError<RErr, AErr, SErr> {
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

pub type SmsGgResult<RErr, AErr, SErr> = Result<TickEffect, SmsGgError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct FrameBuffer(Vec<Color>);

impl FrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); vdp::FRAME_BUFFER_LEN])
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FrameBuffer {
    type Target = Vec<Color>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SmsRegion {
    #[default]
    International,
    Domestic,
}

#[derive(Debug, Clone, Copy)]
pub struct SmsGgEmulatorConfig {
    pub vdp_version: VdpVersion,
    pub psg_version: PsgVersion,
    pub pixel_aspect_ratio: Option<PixelAspectRatio>,
    pub remove_sprite_limit: bool,
    pub sms_region: SmsRegion,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub fm_sound_unit_enabled: bool,
    pub overclock_z80: bool,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct SmsGgEmulator {
    #[partial_clone(partial)]
    memory: Memory,
    z80: Z80,
    vdp: Vdp,
    vdp_version: VdpVersion,
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    psg: Psg,
    ym2413: Option<Ym2413>,
    input: InputState,
    audio_resampler: AudioResampler,
    frame_buffer: FrameBuffer,
    sms_crop_vertical_border: bool,
    sms_crop_left_border: bool,
    overclock_z80: bool,
    z80_cycles_remainder: u32,
    vdp_cycles_remainder: u32,
    frame_count: u64,
    reset_frames_remaining: u32,
}

impl SmsGgEmulator {
    #[must_use]
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: SmsGgEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let cartridge_ram = save_writer.load_bytes("sav").ok();

        let memory = Memory::new(rom, cartridge_ram);
        let vdp = Vdp::new(config.vdp_version, config.remove_sprite_limit);
        let psg = Psg::new(config.psg_version);
        let input = InputState::new(config.sms_region);

        let mut z80 = Z80::new();
        init_z80(&mut z80);

        let ym2413 = config.fm_sound_unit_enabled.then(Ym2413::new);

        let timing_mode = vdp.timing_mode();
        Self {
            memory,
            z80,
            vdp,
            vdp_version: config.vdp_version,
            pixel_aspect_ratio: config.pixel_aspect_ratio,
            psg,
            ym2413,
            input,
            audio_resampler: AudioResampler::new(timing_mode),
            frame_buffer: FrameBuffer::new(),
            sms_crop_vertical_border: config.sms_crop_vertical_border,
            sms_crop_left_border: config.sms_crop_left_border,
            overclock_z80: config.overclock_z80,
            z80_cycles_remainder: 0,
            vdp_cycles_remainder: 0,
            frame_count: 0,
            reset_frames_remaining: 0,
        }
    }

    #[must_use]
    pub fn vdp_version(&self) -> VdpVersion {
        self.vdp_version
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.memory.cartridge_has_battery()
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        let crop_vertical_border =
            self.vdp_version.is_master_system() && self.sms_crop_vertical_border;
        let crop_left_border = self.vdp_version.is_master_system() && self.sms_crop_left_border;
        populate_frame_buffer(
            self.vdp.frame_buffer(),
            self.vdp_version,
            crop_vertical_border,
            crop_left_border,
            &mut self.frame_buffer,
        );

        let viewport = self.vdp_version.viewport_size();
        let frame_width = if crop_left_border {
            viewport.width_without_border().into()
        } else {
            viewport.width.into()
        };
        let frame_height = if crop_vertical_border {
            viewport.height_without_border().into()
        } else {
            viewport.height.into()
        };

        let frame_size = FrameSize { width: frame_width, height: frame_height };
        renderer.render_frame(&self.frame_buffer, frame_size, self.pixel_aspect_ratio)
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }
}

fn init_z80(z80: &mut Z80) {
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);
    z80.set_interrupt_mode(InterruptMode::Mode1);
}

impl EmulatorTrait for SmsGgEmulator {
    type Inputs = SmsGgInputs;
    type Config = SmsGgEmulatorConfig;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = SmsGgError<RErr, AErr, SErr>;

    /// Execute a single CPU instruction and run the rest of the components for the corresponding
    /// number of cycles.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames, pushing audio
    /// samples, or persisting cartridge SRAM.
    #[inline]
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> SmsGgResult<R::Err, A::Err, S::Err>
    where
        R: Renderer,
        A: AudioOutput,
        S: SaveWriter,
    {
        let t_cycles = self.z80.execute_instruction(&mut Bus::new(
            self.vdp_version,
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            self.ym2413.as_mut(),
            &mut self.input,
        ));
        let (t_cycles, remainder) = if self.overclock_z80 {
            // Emulate a Z80 running at 2x speed by only ticking the rest of the components for
            // half as many cycles
            let t_cycles = t_cycles + self.z80_cycles_remainder;
            (t_cycles / 2, t_cycles % 2)
        } else {
            (t_cycles, 0)
        };
        self.z80_cycles_remainder = remainder;

        for _ in 0..t_cycles {
            if let Some(ym2413) = &mut self.ym2413 {
                ym2413.tick();
            }
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (psg_sample_l, psg_sample_r) =
                    if self.memory.psg_enabled() { self.psg.sample() } else { (0.0, 0.0) };
                let ym_sample = if self.memory.fm_enabled() {
                    self.ym2413.as_ref().map_or(0.0, Ym2413::sample)
                } else {
                    0.0
                };

                let sample_l = psg_sample_l + ym_sample;
                let sample_r = psg_sample_r + ym_sample;
                self.audio_resampler.collect_sample(sample_l, sample_r);
            }
        }

        let t_cycles_plus_leftover = t_cycles + self.vdp_cycles_remainder;
        self.vdp_cycles_remainder = t_cycles_plus_leftover % 2;

        let mut frame_rendered = false;
        let vdp_cycles = t_cycles_plus_leftover / 2 * 3;
        for _ in 0..vdp_cycles {
            if self.vdp.tick() == VdpTickEffect::FrameComplete {
                self.render_frame(renderer).map_err(SmsGgError::Render)?;
                frame_rendered = true;

                self.audio_resampler.output_samples(audio_output).map_err(SmsGgError::Audio)?;

                self.input.set_inputs(*inputs);
                self.input.set_reset(self.reset_frames_remaining != 0);
                self.reset_frames_remaining = self.reset_frames_remaining.saturating_sub(1);

                self.frame_count += 1;
                if self.frame_count % 60 == 0
                    && self.memory.cartridge_has_battery()
                    && self.memory.cartridge_ram_dirty()
                {
                    self.memory.clear_cartridge_ram_dirty();
                    save_writer
                        .persist_bytes("sav", self.memory.cartridge_ram())
                        .map_err(SmsGgError::SaveWrite)?;
                }
            }
        }

        Ok(if frame_rendered { TickEffect::FrameRendered } else { TickEffect::None })
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.vdp_version = config.vdp_version;
        self.vdp.set_version(config.vdp_version);
        self.psg.set_version(config.psg_version);
        self.pixel_aspect_ratio = config.pixel_aspect_ratio;
        self.vdp.set_remove_sprite_limit(config.remove_sprite_limit);
        self.input.set_region(config.sms_region);
        self.sms_crop_vertical_border = config.sms_crop_vertical_border;
        self.sms_crop_left_border = config.sms_crop_left_border;
        self.overclock_z80 = config.overclock_z80;
        self.audio_resampler.update_timing_mode(self.vdp.timing_mode());
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        // The SMS RESET button only sets a bit in a register; emulate "soft reset" by keeping the
        // button virtually held down for 5 frames
        self.reset_frames_remaining = 5;
    }

    fn hard_reset<S: SaveWriter>(&mut self, _save_writer: &mut S) {
        log::info!("Hard resetting console");

        let (rom, ram) = self.memory.take_cartridge_rom_and_ram();
        self.memory = Memory::new(rom, Some(ram));

        self.z80 = Z80::new();
        init_z80(&mut self.z80);

        self.vdp = Vdp::new(self.vdp_version, self.vdp.get_remove_sprite_limit());
        self.psg = Psg::new(self.psg.version());
        self.input = InputState::new(self.input.region());

        self.vdp_cycles_remainder = 0;
        self.frame_count = 0;
    }

    fn timing_mode(&self) -> TimingMode {
        self.vdp.timing_mode()
    }
}

fn populate_frame_buffer(
    vdp_buffer: &VdpBuffer,
    vdp_version: VdpVersion,
    crop_vertical_border: bool,
    crop_left_border: bool,
    frame_buffer: &mut [Color],
) {
    let viewport = vdp_version.viewport_size();

    let (row_skip, row_take) = if crop_vertical_border {
        (viewport.top_border_height as usize, viewport.height_without_border() as usize)
    } else {
        (0, viewport.height as usize)
    };
    let (col_skip, screen_width) = if crop_left_border {
        (viewport.left_border_width as usize, viewport.width_without_border() as usize)
    } else {
        (0, viewport.width as usize)
    };

    for (i, row) in vdp_buffer.iter().skip(row_skip).take(row_take).enumerate() {
        for (j, color) in row.iter().copied().skip(col_skip).enumerate() {
            let (r, g, b) = if vdp_version.is_master_system() {
                (
                    vdp::convert_sms_color(color & 0x03),
                    vdp::convert_sms_color((color >> 2) & 0x03),
                    vdp::convert_sms_color((color >> 4) & 0x03),
                )
            } else {
                (
                    vdp::convert_gg_color(color & 0x0F),
                    vdp::convert_gg_color((color >> 4) & 0x0F),
                    vdp::convert_gg_color((color >> 8) & 0x0F),
                )
            };

            frame_buffer[i * screen_width + j] = Color::rgb(r, g, b);
        }
    }
}
