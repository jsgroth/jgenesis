//! Sega Master System / Game Gear public interface and main loop

use crate::audio::{AudioResampler, TimingModeExt};
use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::Memory;
use crate::psg::{Sn76489, Sn76489TickEffect};
use crate::vdp::{Vdp, VdpBuffer, VdpTickEffect, ViewportSize};
use crate::{VdpVersion, vdp};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, FrameSize, PartialClone,
    PixelAspectRatio, Renderer, SaveWriter, TickEffect, TimingMode,
};
use jgenesis_proc_macros::{ConfigDisplay, FakeDecode, FakeEncode};
use smsgg_config::{
    GgAspectRatio, SmsAspectRatio, SmsGgButton, SmsGgInputs, SmsGgRegion, SmsModel, Sn76489Version,
};
use std::fmt::{Debug, Display};
use std::num::NonZeroU32;
use std::ops::{Deref, DerefMut};
use thiserror::Error;
use ym_opll::Ym2413;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum SmsGgHardware {
    MasterSystem,
    GameGear,
}

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct SmsGgEmulatorConfig {
    pub sms_timing_mode: TimingMode,
    pub sms_model: SmsModel,
    pub forced_psg_version: Option<Sn76489Version>,
    pub sms_aspect_ratio: SmsAspectRatio,
    pub gg_aspect_ratio: GgAspectRatio,
    pub remove_sprite_limit: bool,
    pub forced_region: Option<SmsGgRegion>,
    pub sms_crop_vertical_border: bool,
    pub sms_crop_left_border: bool,
    pub gg_use_sms_resolution: bool,
    pub fm_sound_unit_enabled: bool,
    pub z80_divider: NonZeroU32,
}

impl EmulatorConfigTrait for SmsGgEmulatorConfig {
    fn with_overclocking_disabled(&self) -> Self {
        Self { z80_divider: NonZeroU32::new(crate::NATIVE_Z80_DIVIDER).unwrap(), ..*self }
    }
}

impl SmsGgEmulatorConfig {
    pub(crate) fn region(self, memory: &Memory) -> SmsGgRegion {
        self.forced_region.unwrap_or_else(|| memory.guess_cartridge_region())
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct SmsGgEmulator {
    #[partial_clone(partial)]
    memory: Memory,
    z80: Z80,
    vdp: Vdp,
    vdp_version: VdpVersion,
    pixel_aspect_ratio: Option<PixelAspectRatio>,
    psg: Sn76489,
    ym2413: Option<Ym2413>,
    input: InputState,
    audio_resampler: AudioResampler,
    frame_buffer: FrameBuffer,
    config: SmsGgEmulatorConfig,
    vdp_mclk_counter: u32,
    psg_mclk_counter: u32,
    frame_count: u64,
    reset_frames_remaining: u32,
}

const VDP_DIVIDER: u32 = 10;
const PSG_DIVIDER: u32 = 15;

const YM2413_CLOCK_INTERVAL: u8 = 72;

impl SmsGgEmulator {
    #[must_use]
    pub fn create<S: SaveWriter>(
        rom: Option<Vec<u8>>,
        bios_rom: Option<Vec<u8>>,
        hardware: SmsGgHardware,
        config: SmsGgEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let cartridge_ram = save_writer.load_bytes("sav").ok();

        let vdp_version = determine_vdp_version(hardware, &config);
        let psg_version = determine_psg_version(hardware, &config);

        log::info!("VDP version: {vdp_version:?}");
        log::info!("PSG version: {psg_version:?}");

        let rom = rom.unwrap_or_else(|| vec![0xFF; 0x8000]);
        let memory = Memory::new(rom, bios_rom, cartridge_ram, hardware);
        let vdp = Vdp::new(vdp_version, &config);
        let psg = Sn76489::new(psg_version);
        let input = InputState::new(config.region(&memory));

        log::info!("Region in cartridge header: {:?}", memory.guess_cartridge_region());

        let mut z80 = Z80::new();
        init_z80(&mut z80);

        let ym2413 =
            config.fm_sound_unit_enabled.then(|| ym_opll::new_ym2413(YM2413_CLOCK_INTERVAL));

        let pixel_aspect_ratio = determine_aspect_ratio(hardware, &config);

        let timing_mode = vdp.timing_mode();
        Self {
            memory,
            z80,
            vdp,
            vdp_version,
            pixel_aspect_ratio,
            psg,
            ym2413,
            input,
            audio_resampler: AudioResampler::new(timing_mode),
            frame_buffer: FrameBuffer::new(),
            config,
            vdp_mclk_counter: 0,
            psg_mclk_counter: 0,
            frame_count: 0,
            reset_frames_remaining: 0,
        }
    }

    #[must_use]
    pub fn hardware(&self) -> SmsGgHardware {
        if self.vdp_version.is_master_system() {
            SmsGgHardware::MasterSystem
        } else {
            SmsGgHardware::GameGear
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
        populate_frame_buffer(
            self.vdp.frame_buffer(),
            self.vdp.viewport(),
            self.vdp_version,
            self.config.sms_crop_vertical_border,
            self.config.sms_crop_left_border,
            &mut self.frame_buffer,
        );

        let viewport = self.vdp.viewport();
        let frame_width = if self.config.sms_crop_left_border {
            viewport.width_without_border().into()
        } else {
            viewport.width.into()
        };
        let frame_height = if self.config.sms_crop_vertical_border {
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

    pub fn dump_vdp_registers(&self, callback: impl FnMut(u32, &[(&str, &str)])) {
        self.vdp.dump_registers(callback);
    }
}

fn init_z80(z80: &mut Z80) {
    z80.set_pc(0x0000);
    z80.set_sp(0xDFFF);
    z80.set_interrupt_mode(InterruptMode::Mode1);
}

fn determine_vdp_version(hardware: SmsGgHardware, config: &SmsGgEmulatorConfig) -> VdpVersion {
    match (hardware, config.sms_timing_mode, config.sms_model) {
        (SmsGgHardware::MasterSystem, TimingMode::Ntsc, SmsModel::Sms1) => {
            VdpVersion::NtscMasterSystem1
        }
        (SmsGgHardware::MasterSystem, TimingMode::Ntsc, SmsModel::Sms2) => {
            VdpVersion::NtscMasterSystem2
        }
        (SmsGgHardware::MasterSystem, TimingMode::Pal, SmsModel::Sms1) => {
            VdpVersion::PalMasterSystem1
        }
        (SmsGgHardware::MasterSystem, TimingMode::Pal, SmsModel::Sms2) => {
            VdpVersion::PalMasterSystem2
        }
        (SmsGgHardware::GameGear, _, _) => VdpVersion::GameGear,
    }
}

fn determine_psg_version(hardware: SmsGgHardware, config: &SmsGgEmulatorConfig) -> Sn76489Version {
    config.forced_psg_version.unwrap_or(match hardware {
        SmsGgHardware::MasterSystem => Sn76489Version::MasterSystem2,
        SmsGgHardware::GameGear => Sn76489Version::Standard,
    })
}

fn determine_aspect_ratio(
    hardware: SmsGgHardware,
    config: &SmsGgEmulatorConfig,
) -> Option<PixelAspectRatio> {
    match hardware {
        SmsGgHardware::MasterSystem => config.sms_aspect_ratio.to_pixel_aspect_ratio(),
        SmsGgHardware::GameGear => config.gg_aspect_ratio.to_pixel_aspect_ratio(),
    }
}

impl EmulatorTrait for SmsGgEmulator {
    type Button = SmsGgButton;
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
        let z80_t_cycles = self.z80.execute_instruction(&mut Bus::new(
            self.vdp_version,
            &mut self.memory,
            &mut self.vdp,
            &mut self.psg,
            self.ym2413.as_mut(),
            &mut self.input,
        ));

        let mclk_cycles = z80_t_cycles * self.config.z80_divider.get();
        self.vdp_mclk_counter += mclk_cycles;
        self.psg_mclk_counter += mclk_cycles;

        while self.psg_mclk_counter >= PSG_DIVIDER {
            self.psg_mclk_counter -= PSG_DIVIDER;

            if let Some(ym2413) = &mut self.ym2413 {
                ym2413.tick();
            }
            if self.psg.tick() == Sn76489TickEffect::Clocked {
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

        self.audio_resampler.output_samples(audio_output).map_err(SmsGgError::Audio)?;

        let mut frame_rendered = false;
        while self.vdp_mclk_counter >= VDP_DIVIDER {
            self.vdp_mclk_counter -= VDP_DIVIDER;

            if self.vdp.tick() == VdpTickEffect::FrameComplete {
                self.render_frame(renderer).map_err(SmsGgError::Render)?;
                frame_rendered = true;

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
        self.config = *config;

        let hardware = self.hardware();
        self.vdp_version = determine_vdp_version(hardware, config);
        self.vdp.update_config(self.vdp_version, config);

        self.psg.set_version(determine_psg_version(hardware, config));

        self.pixel_aspect_ratio = determine_aspect_ratio(hardware, config);
        self.input.set_region(config.region(&self.memory));
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

        self.memory.reset();

        self.z80 = Z80::new();
        init_z80(&mut self.z80);

        self.vdp = Vdp::new(self.vdp_version, &self.config);
        self.psg = Sn76489::new(self.psg.version());
        self.input = InputState::new(self.input.region());

        self.vdp_mclk_counter = 0;
        self.psg_mclk_counter = 0;
        self.frame_count = 0;
    }

    fn save_state_version() -> &'static str {
        "0.10.1-0"
    }

    fn target_fps(&self) -> f64 {
        let timing_mode = self.vdp.timing_mode();
        let mclk_frequency = timing_mode.mclk_frequency();
        let scanlines_per_frame = match timing_mode {
            TimingMode::Ntsc => vdp::NTSC_SCANLINES_PER_FRAME,
            TimingMode::Pal => vdp::PAL_SCANLINES_PER_FRAME,
        };

        mclk_frequency / f64::from(vdp::MCLK_CYCLES_PER_SCANLINE) / f64::from(scanlines_per_frame)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}

fn populate_frame_buffer(
    vdp_buffer: &VdpBuffer,
    viewport: ViewportSize,
    vdp_version: VdpVersion,
    crop_vertical_border: bool,
    crop_left_border: bool,
    frame_buffer: &mut [Color],
) {
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
