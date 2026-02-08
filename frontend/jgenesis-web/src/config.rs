use crate::js;
use gba_config::{GbaAspectRatio, GbaAudioInterpolation, GbaButton, GbaColorCorrection, GbaInputs};
use gba_core::api::{GbaAudioConfig, GbaEmulatorConfig};
use genesis_config::{
    GenesisAspectRatio, GenesisButton, GenesisControllerType, GenesisInputs, Opn2BusyBehavior,
    PcmInterpolation, S32XColorTint, S32XPwmResampling, S32XVideoOut, S32XVoidColor,
};
use genesis_core::GenesisEmulatorConfig;
use jgenesis_common::frontend::{ColorCorrection, FiniteF32, MappableInputs, TimingMode};
use jgenesis_common::input::Player;
use jgenesis_renderer::config::{
    FilterMode, PreprocessShader, PrescaleFactor, PrescaleMode, RendererConfig, Scanlines,
    VSyncMode, WgpuBackend,
};
use s32x_core::api::Sega32XEmulatorConfig;
use segacd_core::api::SegaCdEmulatorConfig;
use serde::{Deserialize, Serialize};
use smsgg_config::{GgAspectRatio, SmsAspectRatio, SmsGgButton, SmsGgInputs, SmsModel};
use smsgg_core::SmsGgEmulatorConfig;
use snes_config::{AudioInterpolationMode, SnesAspectRatio, SnesButton};
use snes_core::api::SnesEmulatorConfig;
use snes_core::input::SnesInputs;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
use std::ops::Deref;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsGgWebConfig {
    timing_mode: TimingMode,
    sms_aspect_ratio: SmsAspectRatio,
    gg_aspect_ratio: GgAspectRatio,
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
            forced_region: None,
            remove_sprite_limit: self.remove_sprite_limit,
            sms_crop_left_border: self.sms_crop_left_border,
            sms_crop_vertical_border: self.sms_crop_vertical_border,
            gg_frame_blending: false,
            gg_use_sms_resolution: false,
            fm_sound_unit_enabled: self.fm_unit_enabled,
            z80_divider: NonZeroU32::new(smsgg_core::NATIVE_Z80_DIVIDER).unwrap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisWebConfig {
    aspect_ratio: GenesisAspectRatio,
    remove_sprite_limits: bool,
    non_linear_color_scale: bool,
    lpf_enabled: bool,
    render_vertical_border: bool,
    render_horizontal_border: bool,
    m68k_divider: u64,
}

impl Default for GenesisWebConfig {
    fn default() -> Self {
        Self {
            aspect_ratio: GenesisAspectRatio::default(),
            remove_sprite_limits: false,
            non_linear_color_scale: true,
            lpf_enabled: true,
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
            force_square_pixels_in_h40: false,
            adjust_aspect_ratio_in_2x_resolution: true,
            anamorphic_widescreen: false,
            remove_sprite_limits: self.remove_sprite_limits,
            m68k_clock_divider: self.m68k_divider,
            non_linear_color_scale: self.non_linear_color_scale,
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
            opn2_busy_behavior: Opn2BusyBehavior::default(),
            genesis_lpf_enabled: self.lpf_enabled,
            genesis_lpf_cutoff: genesis_config::MODEL_1_VA2_LPF_CUTOFF,
            ym2612_2nd_lpf_enabled: false,
            ym2612_2nd_lpf_cutoff: genesis_config::MODEL_2_2ND_LPF_CUTOFF,
            ym2612_channels_enabled: [true; 6],
            ym2612_enabled: true,
            psg_enabled: true,
            ym2612_volume_adjustment_db: 0.0,
            psg_volume_adjustment_db: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GbaWebConfig {
    skip_bios_intro_animation: bool,
    color_correction: GbaColorCorrection,
    frame_blending: bool,
    audio_interpolation: GbaAudioInterpolation,
    psg_low_pass: bool,
}

impl Default for GbaWebConfig {
    fn default() -> Self {
        Self {
            skip_bios_intro_animation: false,
            color_correction: GbaColorCorrection::default(),
            frame_blending: true,
            audio_interpolation: GbaAudioInterpolation::default(),
            psg_low_pass: false,
        }
    }
}

impl GbaWebConfig {
    pub fn to_emulator_config(&self) -> GbaEmulatorConfig {
        GbaEmulatorConfig {
            skip_bios_animation: self.skip_bios_intro_animation,
            aspect_ratio: GbaAspectRatio::SquarePixels,
            color_correction: match self.color_correction {
                GbaColorCorrection::None => ColorCorrection::None,
                GbaColorCorrection::GbaLcd => {
                    ColorCorrection::GbaLcd { screen_gamma: FiniteF32::try_from(3.2).unwrap() }
                }
            },
            frame_blending: self.frame_blending,
            forced_save_memory_type: None,
            audio: GbaAudioConfig {
                audio_interpolation: self.audio_interpolation,
                psg_low_pass: self.psg_low_pass,
                ..GbaAudioConfig::default()
            },
        }
    }
}

macro_rules! define_input_config {
    (
        config = $config:ident,
        button = $button_t:ident,
        inputs = $inputs:ident,
        fields = {
            $($field:ident: name $name:literal button $button:ident default $default:ident),* $(,)?
        } $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $config {
            $(
                $field: KeyCode,
            )*
        }

        impl Default for $config {
            fn default() -> Self {
                Self {
                    $(
                        $field: KeyCode::$default,
                    )*
                }
            }
        }

        impl $config {
            pub fn fields_iter(&self) -> impl Iterator<Item = (&'static str, KeyCode)> {
                [
                    $(
                        ($name, self.$field),
                    )*
                ]
                    .into_iter()
            }

            pub fn update_field(&mut self, name: &str, key: KeyCode) {
                match name {
                    $(
                        $name => self.$field = key,
                    )*
                    _ => {}
                }
            }

            pub fn handle_input(&self, key: KeyCode, pressed: bool, inputs: &mut $inputs) {
                $(
                    if key == self.$field {
                        inputs.set_field($button_t::$button, Player::One, pressed);
                    }
                )*
            }
        }
    }
}

fn split_input_iterator<'a>(
    iter: impl Iterator<Item = (&'a str, KeyCode)>,
) -> (Vec<String>, Vec<String>) {
    iter.map(|(name, key)| (String::from(name), format!("{key:?}"))).unzip()
}

define_input_config! {
    config = SmsGgInputConfig,
    button = SmsGgButton,
    inputs = SmsGgInputs,
    fields = {
        up: name "Up" button Up default ArrowUp,
        left: name "Left" button Left default ArrowLeft,
        right: name "Right" button Right default ArrowRight,
        down: name "Down" button Down default ArrowDown,
        button1: name "Button 1" button Button1 default KeyS,
        button2: name "Button 2" button Button2 default KeyA,
        pause: name "Start/Pause" button Pause default Enter,
    }
}

define_input_config! {
    config = GenesisInputConfig,
    button = GenesisButton,
    inputs = GenesisInputs,
    fields = {
        up: name "Up" button Up default ArrowUp,
        left: name "Left" button Left default ArrowLeft,
        right: name "Right" button Right default ArrowRight,
        down: name "Down" button Down default ArrowDown,
        a: name "A" button A default KeyA,
        b: name "B" button B default KeyS,
        c: name "C" button C default KeyD,
        x: name "X" button X default KeyQ,
        y: name "Y" button Y default KeyW,
        z: name "Z" button Z default KeyE,
        start: name "Start" button Start default Enter,
        mode: name "Mode" button Mode default ShiftRight,
    }
}

define_input_config! {
    config = SnesInputConfig,
    button = SnesButton,
    inputs = SnesInputs,
    fields = {
        up: name "Up" button Up default ArrowUp,
        left: name "Left" button Left default ArrowLeft,
        right: name "Right" button Right default ArrowRight,
        down: name "Down" button Down default ArrowDown,
        a: name "A" button A default KeyS,
        b: name "B" button B default KeyX,
        x: name "X" button X default KeyA,
        y: name "Y" button Y default KeyZ,
        l: name "L" button L default KeyD,
        r: name "R" button R default KeyC,
        start: name "Start" button Start default Enter,
        select: name "Select" button Select default ShiftRight,
    }
}

define_input_config! {
    config = GbaInputConfig,
    button = GbaButton,
    inputs = GbaInputs,
    fields = {
        up: name "Up" button Up default ArrowUp,
        left: name "Left" button Left default ArrowLeft,
        right: name "Right" button Right default ArrowRight,
        down: name "Down" button Down default ArrowDown,
        a: name "A" button A default KeyA,
        b: name "B" button B default KeyS,
        l: name "L" button L default KeyQ,
        r: name "R" button R default KeyW,
        start: name "Start" button Start default Enter,
        select: name "Select" button Select default ShiftRight,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputConfig {
    pub smsgg: SmsGgInputConfig,
    pub genesis: GenesisInputConfig,
    pub snes: SnesInputConfig,
    pub gba: GbaInputConfig,
}

impl InputConfig {
    pub fn smsgg_inputs(&self) -> (Vec<String>, Vec<String>) {
        split_input_iterator(self.smsgg.fields_iter())
    }

    pub fn genesis_inputs(&self) -> (Vec<String>, Vec<String>) {
        split_input_iterator(self.genesis.fields_iter())
    }

    pub fn snes_inputs(&self) -> (Vec<String>, Vec<String>) {
        split_input_iterator(self.snes.fields_iter())
    }

    pub fn gba_inputs(&self) -> (Vec<String>, Vec<String>) {
        split_input_iterator(self.gba.fields_iter())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebConfig {
    pub common: CommonWebConfig,
    pub smsgg: SmsGgWebConfig,
    pub genesis: GenesisWebConfig,
    pub snes: SnesWebConfig,
    pub gba: GbaWebConfig,
    pub inputs: InputConfig,
}

impl WebConfig {
    const LOCAL_STORAGE_KEY: &str = "config";

    pub fn read_from_local_storage() -> Option<Self> {
        let config_str = js::localStorageGet(Self::LOCAL_STORAGE_KEY)?;
        serde_json::from_str(&config_str).ok()
    }

    pub fn save_to_local_storage(&self) {
        let config_str = match serde_json::to_string(self) {
            Ok(config_str) => config_str,
            Err(err) => {
                log::error!("Error serializing config: {err}");
                return;
            }
        };

        js::localStorageSet(Self::LOCAL_STORAGE_KEY, &config_str);
    }

    pub fn to_sega_cd_config(&self) -> SegaCdEmulatorConfig {
        SegaCdEmulatorConfig {
            genesis: self.genesis.to_emulator_config(),
            pcm_interpolation: PcmInterpolation::CubicHermite6Point,
            enable_ram_cartridge: true,
            load_disc_into_ram: true,
            disc_drive_speed: NonZeroU16::new(1).unwrap(),
            sub_cpu_divider: NonZeroU64::new(segacd_core::api::DEFAULT_SUB_CPU_DIVIDER).unwrap(),
            pcm_lpf_enabled: true,
            pcm_lpf_cutoff: segacd_core::DEFAULT_PCM_LPF_CUTOFF,
            apply_genesis_lpf_to_pcm: false,
            apply_genesis_lpf_to_cd_da: false,
            pcm_enabled: true,
            cd_audio_enabled: true,
            pcm_volume_adjustment_db: 0.0,
            cd_volume_adjustment_db: 0.0,
        }
    }

    pub fn to_32x_config(&self) -> Sega32XEmulatorConfig {
        Sega32XEmulatorConfig {
            genesis: self.genesis.to_emulator_config(),
            sh2_clock_multiplier: NonZeroU64::new(genesis_config::NATIVE_SH2_MULTIPLIER).unwrap(),
            video_out: S32XVideoOut::default(),
            darken_genesis_colors: true,
            color_tint: S32XColorTint::default(),
            show_high_priority: true,
            show_low_priority: true,
            void_color: S32XVoidColor::default(),
            emulate_pixel_switch_delay: false,
            apply_genesis_lpf_to_pwm: true,
            pwm_resampling: S32XPwmResampling::CubicHermite,
            pwm_enabled: true,
            pwm_volume_adjustment_db: 0.0,
        }
    }

    pub fn to_renderer_config(&self) -> RendererConfig {
        self.common.to_renderer_config()
    }
}

#[wasm_bindgen]
pub struct WebConfigRef(Rc<RefCell<WebConfig>>);

#[wasm_bindgen]
impl WebConfigRef {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let config = WebConfig::read_from_local_storage().unwrap_or_default();
        Self(Rc::new(RefCell::new(config)))
    }

    pub fn filter_mode(&self) -> String {
        self.borrow().common.filter_mode.to_string()
    }

    pub fn set_filter_mode(&self, filter_mode: &str) {
        let Ok(filter_mode) = filter_mode.parse() else { return };
        self.borrow_mut().common.filter_mode = filter_mode;
    }

    pub fn preprocess_shader(&self) -> String {
        self.borrow().common.preprocess_shader.to_string()
    }

    pub fn set_preprocess_shader(&self, preprocess_shader: &str) {
        let Ok(preprocess_shader) = preprocess_shader.parse() else { return };
        self.borrow_mut().common.preprocess_shader = preprocess_shader;
    }

    pub fn prescale_factor(&self) -> u32 {
        self.borrow().common.prescale_factor.get()
    }

    pub fn set_prescale_factor(&self, prescale_factor: u32) {
        let Ok(prescale_factor) = prescale_factor.try_into() else { return };
        self.borrow_mut().common.prescale_factor = prescale_factor;
    }

    pub fn sms_timing_mode(&self) -> String {
        self.borrow().smsgg.timing_mode.to_string()
    }

    pub fn set_sms_timing_mode(&self, timing_mode: &str) {
        let Ok(timing_mode) = timing_mode.parse() else { return };
        self.borrow_mut().smsgg.timing_mode = timing_mode;
    }

    pub fn sms_aspect_ratio(&self) -> String {
        self.borrow().smsgg.sms_aspect_ratio.to_string()
    }

    pub fn set_sms_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().smsgg.sms_aspect_ratio = aspect_ratio;
    }

    pub fn gg_aspect_ratio(&self) -> String {
        self.borrow().smsgg.gg_aspect_ratio.to_string()
    }

    pub fn set_gg_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().smsgg.gg_aspect_ratio = aspect_ratio;
    }

    pub fn sms_remove_sprite_limit(&self) -> bool {
        self.borrow().smsgg.remove_sprite_limit
    }

    pub fn set_sms_remove_sprite_limit(&self, remove_sprite_limit: bool) {
        self.borrow_mut().smsgg.remove_sprite_limit = remove_sprite_limit;
    }

    pub fn sms_crop_vertical_border(&self) -> bool {
        self.borrow().smsgg.sms_crop_vertical_border
    }

    pub fn set_sms_crop_vertical_border(&self, crop: bool) {
        self.borrow_mut().smsgg.sms_crop_vertical_border = crop;
    }

    pub fn sms_crop_left_border(&self) -> bool {
        self.borrow().smsgg.sms_crop_left_border
    }

    pub fn set_sms_crop_left_border(&self, crop: bool) {
        self.borrow_mut().smsgg.sms_crop_left_border = crop;
    }

    pub fn sms_fm_enabled(&self) -> bool {
        self.borrow().smsgg.fm_unit_enabled
    }

    pub fn set_sms_fm_enabled(&self, enabled: bool) {
        self.borrow_mut().smsgg.fm_unit_enabled = enabled;
    }

    pub fn genesis_m68k_divider(&self) -> u32 {
        self.borrow().genesis.m68k_divider as u32
    }

    pub fn set_genesis_m68k_divider(&self, m68k_divider: &str) {
        let Ok(m68k_divider) = m68k_divider.parse() else { return };
        self.borrow_mut().genesis.m68k_divider = m68k_divider;
    }

    pub fn genesis_aspect_ratio(&self) -> String {
        self.borrow().genesis.aspect_ratio.to_string()
    }

    pub fn set_genesis_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().genesis.aspect_ratio = aspect_ratio;
    }

    pub fn genesis_remove_sprite_limits(&self) -> bool {
        self.borrow().genesis.remove_sprite_limits
    }

    pub fn set_genesis_remove_sprite_limits(&self, remove_sprite_limits: bool) {
        self.borrow_mut().genesis.remove_sprite_limits = remove_sprite_limits;
    }

    pub fn genesis_non_linear_color_scale(&self) -> bool {
        self.borrow().genesis.non_linear_color_scale
    }

    pub fn set_genesis_non_linear_color_scale(&self, non_linear_color_scale: bool) {
        self.borrow_mut().genesis.non_linear_color_scale = non_linear_color_scale;
    }

    pub fn genesis_emulate_low_pass(&self) -> bool {
        self.borrow().genesis.lpf_enabled
    }

    pub fn set_genesis_emulate_low_pass(&self, emulate_low_pass: bool) {
        self.borrow_mut().genesis.lpf_enabled = emulate_low_pass;
    }

    pub fn genesis_render_vertical_border(&self) -> bool {
        self.borrow().genesis.render_vertical_border
    }

    pub fn set_genesis_render_vertical_border(&self, render_vertical_border: bool) {
        self.borrow_mut().genesis.render_vertical_border = render_vertical_border;
    }

    pub fn genesis_render_horizontal_border(&self) -> bool {
        self.borrow().genesis.render_horizontal_border
    }

    pub fn set_genesis_render_horizontal_border(&self, render_horizontal_border: bool) {
        self.borrow_mut().genesis.render_horizontal_border = render_horizontal_border;
    }

    pub fn snes_aspect_ratio(&self) -> String {
        self.borrow().snes.aspect_ratio.to_string()
    }

    pub fn set_snes_aspect_ratio(&self, aspect_ratio: &str) {
        let Ok(aspect_ratio) = aspect_ratio.parse() else { return };
        self.borrow_mut().snes.aspect_ratio = aspect_ratio;
    }

    pub fn snes_audio_interpolation(&self) -> String {
        self.borrow().snes.audio_interpolation.to_string()
    }

    pub fn set_snes_audio_interpolation(&self, audio_interpolation: &str) {
        let Ok(audio_interpolation) = audio_interpolation.parse() else { return };
        self.borrow_mut().snes.audio_interpolation = audio_interpolation;
    }

    pub fn gba_skip_bios_animation(&self) -> bool {
        self.borrow().gba.skip_bios_intro_animation
    }

    pub fn set_gba_skip_bios_animation(&self, skip_bios_animation: bool) {
        self.borrow_mut().gba.skip_bios_intro_animation = skip_bios_animation;
    }

    pub fn gba_color_correction(&self) -> String {
        self.borrow().gba.color_correction.to_string()
    }

    pub fn set_gba_color_correction(&self, color_correction: &str) {
        let Ok(color_correction) = color_correction.parse() else { return };
        self.borrow_mut().gba.color_correction = color_correction;
    }

    pub fn gba_frame_blending(&self) -> bool {
        self.borrow().gba.frame_blending
    }

    pub fn set_gba_frame_blending(&self, frame_blending: bool) {
        self.borrow_mut().gba.frame_blending = frame_blending;
    }

    pub fn gba_audio_interpolation(&self) -> String {
        self.borrow().gba.audio_interpolation.to_string()
    }

    pub fn set_gba_audio_interpolation(&self, audio_interpolation: &str) {
        let Ok(audio_interpolation) = audio_interpolation.parse() else { return };
        self.borrow_mut().gba.audio_interpolation = audio_interpolation;
    }

    pub fn gba_psg_low_pass(&self) -> bool {
        self.borrow().gba.psg_low_pass
    }

    pub fn set_gba_psg_low_pass(&self, psg_low_pass: bool) {
        self.borrow_mut().gba.psg_low_pass = psg_low_pass;
    }

    pub fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }

    pub fn restore_defaults(&self) {
        let mut config = self.borrow_mut();
        *config = WebConfig::default();
        config.save_to_local_storage();
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulatorCommand {
    OpenFile,
    OpenSegaCdBios,
    OpenGbaBios,
    Reset,
    UploadSaveFile,
    ConfigureInput { name: String },
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

    pub fn request_open_sega_cd_bios(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::OpenSegaCdBios);
    }

    pub fn request_open_gba_bios(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::OpenGbaBios);
    }

    pub fn request_reset(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::Reset);
    }

    pub fn request_upload_save_file(&self) {
        self.commands.borrow_mut().push_back(EmulatorCommand::UploadSaveFile);
    }

    pub fn request_configure_input(&self, name: &str) {
        self.commands.borrow_mut().push_back(EmulatorCommand::ConfigureInput { name: name.into() });
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
