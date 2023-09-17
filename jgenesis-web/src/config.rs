use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, RendererConfig, Scanlines, VSyncMode, WgpuBackend,
};
use jgenesis_traits::frontend::{PixelAspectRatio, TimingMode};
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulatorConfig, SmsRegion, VdpVersion};
use std::cell::RefCell;
use std::collections::VecDeque;
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
            prescale_factor: self.prescale_factor,
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
    pub fn vdp_version(&self, file_extension: &str) -> VdpVersion {
        if file_extension == "gg" {
            VdpVersion::GameGear
        } else {
            match self.timing_mode {
                TimingMode::Ntsc => VdpVersion::NtscMasterSystem2,
                TimingMode::Pal => VdpVersion::PalMasterSystem2,
            }
        }
    }

    pub fn to_emulator_config(&self, vdp_version: VdpVersion) -> SmsGgEmulatorConfig {
        let (psg_version, pixel_aspect_ratio) = if vdp_version.is_master_system() {
            (PsgVersion::MasterSystem2, self.sms_aspect_ratio.to_pixel_aspect_ratio())
        } else {
            (PsgVersion::Standard, self.gg_aspect_ratio.to_pixel_aspect_ratio())
        };

        SmsGgEmulatorConfig {
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
}

impl GenesisWebConfig {
    pub fn to_emulator_config(&self) -> GenesisEmulatorConfig {
        GenesisEmulatorConfig {
            forced_timing_mode: None,
            forced_region: None,
            aspect_ratio: self.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: true,
            remove_sprite_limits: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebConfig {
    pub common: CommonWebConfig,
    pub smsgg: SmsGgWebConfig,
    pub genesis: GenesisWebConfig,
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
    Reset,
}

#[wasm_bindgen]
pub struct EmulatorChannel(Rc<RefCell<VecDeque<EmulatorCommand>>>);

#[wasm_bindgen]
impl EmulatorChannel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(Rc::default())
    }

    pub fn request_open_file(&self) {
        self.0.borrow_mut().push_back(EmulatorCommand::OpenFile);
    }

    pub fn request_reset(&self) {
        self.0.borrow_mut().push_back(EmulatorCommand::Reset);
    }

    pub fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl EmulatorChannel {
    pub fn pop_command(&self) -> Option<EmulatorCommand> {
        self.0.borrow_mut().pop_front()
    }
}

impl Default for EmulatorChannel {
    fn default() -> Self {
        Self::new()
    }
}
