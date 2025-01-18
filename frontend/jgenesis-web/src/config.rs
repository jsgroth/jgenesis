use genesis_core::input::GenesisControllerType;
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig, GenesisLowPassFilter};
use jgenesis_common::frontend::TimingMode;
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use segacd_core::api::{PcmInterpolation, SegaCdEmulatorConfig};
use smsgg_core::{GgAspectRatio, SmsAspectRatio, SmsGgEmulatorConfig, SmsModel, SmsRegion};
use snes_core::api::{AudioInterpolationMode, SnesAspectRatio, SnesEmulatorConfig};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
use std::ops::Deref;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

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
            // Frame time sync does not work on web because it blocks until the next frame time
            frame_time_sync: false,
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
    gg_aspect_ratio: GgAspectRatio,
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
            gg_aspect_ratio: GgAspectRatio::default(),
            region: SmsRegion::default(),
            remove_sprite_limit: false,
            sms_crop_vertical_border: true,
            sms_crop_left_border: false,
            fm_unit_enabled: true,
        }
    }
}

impl SmsGgWebConfig {
    pub(crate) fn to_emulator_config(&self) -> SmsGgEmulatorConfig {
        SmsGgEmulatorConfig {
            sms_timing_mode: self.timing_mode,
            sms_model: SmsModel::default(),
            forced_psg_version: None,
            sms_aspect_ratio: self.sms_aspect_ratio,
            gg_aspect_ratio: self.gg_aspect_ratio,
            sms_region: self.region,
            remove_sprite_limit: self.remove_sprite_limit,
            sms_crop_left_border: self.sms_crop_left_border,
            sms_crop_vertical_border: self.sms_crop_vertical_border,
            gg_use_sms_resolution: false,
            fm_sound_unit_enabled: self.fm_unit_enabled,
            z80_divider: NonZeroU32::new(smsgg_core::NATIVE_Z80_DIVIDER).unwrap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisWebConfig {
    aspect_ratio: GenesisAspectRatio,
    remove_sprite_limits: bool,
    emulate_non_linear_vdp_dac: bool,
    low_pass: GenesisLowPassFilter,
    render_vertical_border: bool,
    render_horizontal_border: bool,
    m68k_divider: u64,
}

impl Default for GenesisWebConfig {
    fn default() -> Self {
        Self {
            aspect_ratio: GenesisAspectRatio::default(),
            remove_sprite_limits: false,
            emulate_non_linear_vdp_dac: false,
            low_pass: GenesisLowPassFilter::default(),
            render_vertical_border: false,
            render_horizontal_border: false,
            m68k_divider: genesis_core::timing::NATIVE_M68K_DIVIDER,
        }
    }
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
            m68k_clock_divider: self.m68k_divider,
            emulate_non_linear_vdp_dac: self.emulate_non_linear_vdp_dac,
            deinterlace: true,
            render_vertical_border: self.render_vertical_border,
            render_horizontal_border: self.render_horizontal_border,
            plane_a_enabled: true,
            plane_b_enabled: true,
            sprites_enabled: true,
            window_enabled: true,
            backdrop_enabled: true,
            quantize_ym2612_output: true,
            emulate_ym2612_ladder_effect: true,
            low_pass: self.low_pass,
            ym2612_enabled: true,
            psg_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnesWebConfig {
    aspect_ratio: SnesAspectRatio,
    audio_interpolation: AudioInterpolationMode,
}

impl SnesWebConfig {
    pub fn to_emulator_config(&self) -> SnesEmulatorConfig {
        SnesEmulatorConfig {
            forced_timing_mode: None,
            aspect_ratio: self.aspect_ratio,
            deinterlace: true,
            audio_interpolation: self.audio_interpolation,
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

impl WebConfig {
    pub fn to_sega_cd_config(&self) -> SegaCdEmulatorConfig {
        SegaCdEmulatorConfig {
            genesis: self.genesis.to_emulator_config(),
            pcm_interpolation: PcmInterpolation::CubicHermite,
            enable_ram_cartridge: true,
            load_disc_into_ram: true,
            disc_drive_speed: NonZeroU16::new(1).unwrap(),
            sub_cpu_divider: NonZeroU64::new(segacd_core::api::DEFAULT_SUB_CPU_DIVIDER).unwrap(),
            low_pass_cd_da: false,
            pcm_enabled: true,
            cd_audio_enabled: true,
        }
    }
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

    pub fn set_genesis_m68k_divider(&self, m68k_divider: &str) {
        let Ok(m68k_divider) = m68k_divider.parse() else { return };
        self.borrow_mut().genesis.m68k_divider = m68k_divider;
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

    pub fn set_genesis_emulate_low_pass(&self, emulate_low_pass: bool) {
        self.borrow_mut().genesis.low_pass = if emulate_low_pass {
            GenesisLowPassFilter::Model1Va2
        } else {
            GenesisLowPassFilter::None
        };
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

    pub fn set_snes_audio_interpolation(&self, audio_interpolation: &str) {
        let Ok(audio_interpolation) = audio_interpolation.parse() else { return };
        self.borrow_mut().snes.audio_interpolation = audio_interpolation;
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
