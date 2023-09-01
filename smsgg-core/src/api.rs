use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::{Psg, PsgTickEffect, PsgVersion};
use crate::vdp::{Vdp, VdpBuffer, VdpTickEffect};
use crate::{vdp, SmsGgInputs, VdpVersion};
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, FakeDecode, FakeEncode};
use jgenesis_traits::frontend::{
    AudioOutput, Color, FrameSize, PixelAspectRatio, Renderer, SaveWriter, TickEffect,
    TickableEmulator,
};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use z80_emu::{InterruptMode, Z80};

// 53_693_175 / 15 / 16 / 48000
const DOWNSAMPLING_RATIO: f64 = 4.6608658854166665;

#[derive(Debug)]
pub enum SmsGgError<RErr, AErr, SErr> {
    Render(RErr),
    Audio(AErr),
    SaveWrite(SErr),
}

impl<RErr, AErr, SErr> Display for SmsGgError<RErr, AErr, SErr>
where
    RErr: Display,
    AErr: Display,
    SErr: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(err) => write!(f, "Rendering error: {err}"),
            Self::Audio(err) => write!(f, "Audio error: {err}"),
            Self::SaveWrite(err) => write!(f, "Error writing save file: {err}"),
        }
    }
}

impl<RErr, AErr, SErr> Error for SmsGgError<RErr, AErr, SErr>
where
    RErr: Debug + Display + AsRef<dyn Error + 'static>,
    AErr: Debug + Display + AsRef<dyn Error + 'static>,
    SErr: Debug + Display + AsRef<dyn Error + 'static>,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Render(err) => Some(err.as_ref()),
            Self::Audio(err) => Some(err.as_ref()),
            Self::SaveWrite(err) => Some(err.as_ref()),
        }
    }
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
    pub pixel_aspect_ratio: Option<PixelAspectRatio>,
    pub remove_sprite_limit: bool,
    pub sms_region: SmsRegion,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SmsGgEmulator {
    memory: Memory,
    z80: Z80,
    vdp: Vdp,
    vdp_version: VdpVersion,
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    psg: Psg,
    input: InputState,
    frame_buffer: FrameBuffer,
    sms_crop_vertical_border: bool,
    sms_crop_left_border: bool,
    leftover_vdp_cycles: u32,
    sample_count: u64,
    frame_count: u64,
}

impl SmsGgEmulator {
    #[must_use]
    pub fn create(
        rom: Vec<u8>,
        cartridge_ram: Option<Vec<u8>>,
        vdp_version: VdpVersion,
        psg_version: PsgVersion,
        config: SmsGgEmulatorConfig,
    ) -> Self {
        let memory = Memory::new(rom, cartridge_ram);
        let vdp = Vdp::new(vdp_version, config.remove_sprite_limit);
        let psg = Psg::new(psg_version);
        let input = InputState::new(config.sms_region);

        let mut z80 = Z80::new();
        z80.set_pc(0x0000);
        z80.set_sp(0xDFFF);
        z80.set_interrupt_mode(InterruptMode::Mode1);

        Self {
            memory,
            z80,
            vdp,
            vdp_version,
            pixel_aspect_ratio: config.pixel_aspect_ratio,
            psg,
            input,
            frame_buffer: FrameBuffer::new(),
            sms_crop_vertical_border: config.sms_crop_vertical_border,
            sms_crop_left_border: config.sms_crop_left_border,
            leftover_vdp_cycles: 0,
            sample_count: 0,
            frame_count: 0,
        }
    }

    #[must_use]
    pub fn vdp_version(&self) -> VdpVersion {
        self.vdp_version
    }

    pub fn reload_config(&mut self, psg_version: Option<PsgVersion>, config: SmsGgEmulatorConfig) {
        if let Some(psg_version) = psg_version {
            self.psg.set_version(psg_version);
        }

        self.pixel_aspect_ratio = config.pixel_aspect_ratio;
        self.vdp.set_remove_sprite_limit(config.remove_sprite_limit);
        self.input.set_region(config.sms_region);
        self.sms_crop_vertical_border = config.sms_crop_vertical_border;
        self.sms_crop_left_border = config.sms_crop_left_border;
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
    }
}

impl TickableEmulator for SmsGgEmulator {
    type Inputs = SmsGgInputs;
    type Err<RErr, AErr, SErr> = SmsGgError<RErr, AErr, SErr>;

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
            &mut self.input,
        ));

        for _ in 0..t_cycles {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let prev_count = self.sample_count;
                self.sample_count += 1;

                if (prev_count as f64 / DOWNSAMPLING_RATIO).round() as u64
                    != (self.sample_count as f64 / DOWNSAMPLING_RATIO).round() as u64
                {
                    let (sample_l, sample_r) = self.psg.sample();
                    audio_output.push_sample(sample_l, sample_r).map_err(SmsGgError::Audio)?;
                }
            }
        }

        let t_cycles_plus_leftover = t_cycles + self.leftover_vdp_cycles;
        self.leftover_vdp_cycles = t_cycles_plus_leftover % 2;

        let mut frame_rendered = false;
        let vdp_cycles = t_cycles_plus_leftover / 2 * 3;
        for _ in 0..vdp_cycles {
            if self.vdp.tick() == VdpTickEffect::FrameComplete {
                let crop_vertical_border =
                    self.vdp_version.is_master_system() && self.sms_crop_vertical_border;
                let crop_left_border =
                    self.vdp_version.is_master_system() && self.sms_crop_left_border;
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
                renderer
                    .render_frame(&self.frame_buffer, frame_size, self.pixel_aspect_ratio)
                    .map_err(SmsGgError::Render)?;
                frame_rendered = true;

                self.input.set_inputs(inputs);

                self.frame_count += 1;
                if self.frame_count % 60 == 0
                    && self.memory.cartridge_has_battery()
                    && self.memory.cartridge_ram_dirty()
                {
                    self.memory.clear_cartridge_ram_dirty();
                    save_writer
                        .persist_save(self.memory.cartridge_ram())
                        .map_err(SmsGgError::SaveWrite)?;
                }
            }
        }

        Ok(if frame_rendered { TickEffect::FrameRendered } else { TickEffect::None })
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
            let (r, g, b) = match vdp_version {
                VdpVersion::NtscMasterSystem2 | VdpVersion::PalMasterSystem2 => (
                    convert_sms_color(color & 0x03),
                    convert_sms_color((color >> 2) & 0x03),
                    convert_sms_color((color >> 4) & 0x03),
                ),
                VdpVersion::GameGear => (
                    convert_gg_color(color & 0x0F),
                    convert_gg_color((color >> 4) & 0x0F),
                    convert_gg_color((color >> 8) & 0x0F),
                ),
            };

            frame_buffer[i * screen_width + j] = Color::rgb(r, g, b);
        }
    }
}

#[inline]
fn convert_sms_color(color: u16) -> u8 {
    [0, 85, 170, 255][color as usize]
}

#[inline]
fn convert_gg_color(color: u16) -> u8 {
    [0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255][color as usize]
}
