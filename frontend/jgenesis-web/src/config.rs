use crate::SmsGgConsole;
use genesis_core::input::GenesisControllerType;
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig};
use jgenesis_common::frontend::{PixelAspectRatio, TimingMode};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulatorConfig, SmsRegion, VdpVersion};
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio, SnesEmulatorConfig};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::ops::Deref;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumDisplay, EnumFromStr)]
enum SmsAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
}

impl SmsAspectRatio {
    fn to_pixel_aspect_ratio(self) -> PixelAspectRatio {
        match self {
            Self::Ntsc => PixelAspectRatio::try_from(smsgg_core::SMS_NTSC_ASPECT_RATIO).unwrap(),
            Self::Pal => PixelAspectRatio::try_from(smsgg_core::SMS_PAL_ASPECT_RATIO).unwrap(),
            Self::SquarePixels => PixelAspectRatio::SQUARE,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumDisplay, EnumFromStr)]
enum GameGearAspectRatio {
    #[default]
    GameGearLcd,
    SquarePixels,
}

impl GameGearAspectRatio {
    fn to_pixel_aspect_ratio(self) -> PixelAspectRatio {
        match self {
            Self::GameGearLcd => {
                PixelAspectRatio::try_from(smsgg_core::GAME_GEAR_LCD_ASPECT_RATIO).unwrap()
            }
            Self::SquarePixels => PixelAspectRatio::SQUARE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonWebConfig {
    pub filter_mode: FilterMode,
    pub preprocess_shader: PreprocessShader,
    pub prescale_factor: PrescaleFactor,
}

impl Default for CommonWebConfig {
    fn default() -> Self {
        Self {
            filter_mode: FilterMode::default(),
            preprocess_shader: PreprocessShader::default(),
            prescale_factor: PrescaleFactor::try_from(3).unwrap(),
        }
    }
}

impl CommonWebConfig {
    pub fn to_renderer_config(&self) -> RendererConfig {
        RendererConfig {
            wgpu_backend: WgpuBackend::OpenGl,
            vsync_mode: VSyncMode::Enabled,
            prescale_mode: PrescaleMode::Manual(self.prescale_factor),
            scanlines: Scanlines::default(),
            force_integer_height_scaling: false,
            filter_mode: self.filter_mode,
            preprocess_shader: self.preprocess_shader,
            use_webgl2_limits: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsGgWebConfig {
    timing_mode: TimingMode,
    sms_aspect_ratio: SmsAspectRatio,
    gg_aspect_ratio: GameGearAspectRatio,
    region: SmsRegion,
    remove_sprite_limit: bool,
    sms_crop_vertical_border: bool,
    sms_crop_left_border: bool,
    fm_unit_enabled: bool,
}

impl Default for SmsGgWebConfig {
    fn default() -> Self {
        Self {
            timing_mode: TimingMode::default(),
            sms_aspect_ratio: SmsAspectRatio::default(),
            gg_aspect_ratio: GameGearAspectRatio::default(),
            region: SmsRegion::default(),
            remove_sprite_limit: false,
            sms_crop_vertical_border: false,
            sms_crop_left_border: false,
            fm_unit_enabled: true,
        }
    }
}

impl SmsGgWebConfig {
    fn vdp_version(&self, console: SmsGgConsole) -> VdpVersion {
        match (console, self.timing_mode) {
            (SmsGgConsole::GameGear, _) => VdpVersion::GameGear,
            (SmsGgConsole::MasterSystem, TimingMode::Ntsc) => VdpVersion::NtscMasterSystem2,
            (SmsGgConsole::MasterSystem, TimingMode::Pal) => VdpVersion::PalMasterSystem2,
        }
    }

    pub(crate) fn to_emulator_config(&self, console: SmsGgConsole) -> SmsGgEmulatorConfig {
        let vdp_version = self.vdp_version(console);
        let (psg_version, pixel_aspect_ratio) = if vdp_version.is_master_system() {
            (PsgVersion::MasterSystem2, self.sms_aspect_ratio.to_pixel_aspect_ratio())
        } else {
            (PsgVersion::Standard, self.gg_aspect_ratio.to_pixel_aspect_ratio())
        };

        SmsGgEmulatorConfig {
            vdp_version,
            psg_version,
            pixel_aspect_ratio: Some(pixel_aspect_ratio),
            sms_region: self.region,
            remove_sprite_limit: self.remove_sprite_limit,
            sms_crop_left_border: self.sms_crop_left_border,
            sms_crop_vertical_border: self.sms_crop_vertical_border,
            fm_sound_unit_enabled: self.fm_unit_enabled,
            overclock_z80: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GenesisWebConfig {
    aspect_ratio: GenesisAspectRatio,
    remove_sprite_limits: bool,
    emulate_non_linear_vdp_dac: bool,
    render_vertical_border: bool,
    render_horizontal_border: bool,
}

impl GenesisWebConfig {
    pub fn to_emulator_config(&self) -> GenesisEmulatorConfig {
        GenesisEmulatorConfig {
            p1_controller_type: GenesisControllerType::default(),
            p2_controller_type: GenesisControllerType::default(),
            forced_timing_mode: None,
            forced_region: None,
            aspect_ratio: self.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: true,
            remove_sprite_limits: self.remove_sprite_limits,
            emulate_non_linear_vdp_dac: self.emulate_non_linear_vdp_dac,
            render_vertical_border: self.render_vertical_border,
            render_horizontal_border: self.render_horizontal_border,
            quantize_ym2612_output: true,
            emulate_ym2612_ladder_effect: true,
            ym2612_enabled: true,
            psg_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnesWebConfig {
    aspect_ratio: SnesAspectRatio,
}

impl SnesWebConfig {
    pub fn to_emulator_config(&self) -> SnesEmulatorConfig {
        SnesEmulatorConfig {
            forced_timing_mode: None,
            aspect_ratio: self.aspect_ratio,
            audio_interpolation: AudioInterpolationMode::default(),
            audio_60hz_hack: true,
            gsu_overclock_factor: NonZeroU64::new(1).unwrap(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebConfig {
    pub common: CommonWebConfig,
    pub smsgg: SmsGgWebConfig,
    pub genesis: GenesisWebConfig,
    pub snes: SnesWebConfig,
}

#[wasm_bindgen]
pub struct WebConfigRef(Rc<RefCell<WebConfig>>);

#[wasm_bindgen]
impl WebConfigRef {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(Rc::default())
    }

    pub fn set_filter_mode(&self, filter_mode: &str) {
        let Ok(filter_mode) = filter_mode.parse() else { return };
        self.borrow_mut().common.filter_mode = filter_mode;
    }

    pub fn set_preprocess_shader(&self, preprocess_shader: &str) {
        let Ok(preprocess_shader) = preprocess_shader.parse() else { return };
        self.borrow_mut().common.preprocess_shader = preprocess_shader;
    }

    pub fn set_prescale_factor(&self, prescale_factor: u32) {
        let Ok(prescale_factor) = prescale_factor.try_into() else { return };
        self.borrow_mut().common.prescale_factor = prescale_factor;
    }

    pub fn set_sms_timing_mode(&self, timing_mode: &str) {
        let Ok(timing_mode) = timing_mode.parse() else { return };
        self.borrow_mut().smsgg.timing_mode = timing_mode;
    }

    pub fn set_sms_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().smsgg.sms_aspect_ratio = aspect_ratio;
    }

    pub fn set_gg_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().smsgg.gg_aspect_ratio = aspect_ratio;
    }

    pub fn set_sms_region(&self, region: &str) {
        let Ok(region) = region.parse() else { return };
        self.borrow_mut().smsgg.region = region;
    }

    pub fn set_sms_remove_sprite_limit(&self, remove_sprite_limit: bool) {
        self.borrow_mut().smsgg.remove_sprite_limit = remove_sprite_limit;
    }

    pub fn set_sms_crop_vertical_border(&self, crop: bool) {
        self.borrow_mut().smsgg.sms_crop_vertical_border = crop;
    }

    pub fn set_sms_crop_left_border(&self, crop: bool) {
        self.borrow_mut().smsgg.sms_crop_left_border = crop;
    }

    pub fn set_sms_fm_enabled(&self, enabled: bool) {
        self.borrow_mut().smsgg.fm_unit_enabled = enabled;
    }

    pub fn set_genesis_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().genesis.aspect_ratio = aspect_ratio;
    }

    pub fn set_genesis_remove_sprite_limits(&self, remove_sprite_limits: bool) {
        self.borrow_mut().genesis.remove_sprite_limits = remove_sprite_limits;
    }

    pub fn set_genesis_emulate_non_linear_dac(&self, emulate_non_linear_dac: bool) {
        self.borrow_mut().genesis.emulate_non_linear_vdp_dac = emulate_non_linear_dac;
    }

    pub fn set_genesis_render_vertical_border(&self, render_vertical_border: bool) {
        self.borrow_mut().genesis.render_vertical_border = render_vertical_border;
    }

    pub fn set_genesis_render_horizontal_border(&self, render_horizontal_border: bool) {
        self.borrow_mut().genesis.render_horizontal_border = render_horizontal_border;
    }

    pub fn set_snes_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().snes.aspect_ratio = aspect_ratio;
    }

    pub fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl Deref for WebConfigRef {
    type Target = Rc<RefCell<WebConfig>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for WebConfigRef {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulatorCommand {
    OpenFile,
    OpenSegaCd,
    Reset,
    UploadSaveFile,
}

#[wasm_bindgen]
#[derive(Clone, Default)]
pub struct EmulatorChannel {
    commands: Rc<RefCell<VecDeque<EmulatorCommand>>>,
    current_file_name: Rc<RefCell<String>>,
}

#[wasm_bindgen]
impl EmulatorChannel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_open_file(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::OpenFile);
    }

    pub fn request_open_sega_cd(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::OpenSegaCd);
    }

    pub fn request_reset(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::Reset);
    }

    pub fn request_upload_save_file(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::UploadSaveFile);
    }

    pub fn current_file_name(&self) -> String {
        self.current_file_name.borrow().clone()
    }

    pub fn clone(&self) -> Self {
        <Self as Clone>::clone(self)
    }
}

impl EmulatorChannel {
    pub fn pop_command(&self) -> Option<EmulatorCommand> {
        self.commands.borrow_mut().pop_front()
    }

    pub fn set_current_file_name(&self, current_file_name: String) {
        *self.current_file_name.borrow_mut() = current_file_name;
    }
}
