use crate::archive::{ArchiveEntry, ArchiveError};
use crate::mainloop::NativeEmulatorError;
use crate::{NativeEmulatorResult, archive, extensions};
use gb_config::GbcColorCorrection;
use gb_core::api::GameBoyEmulatorConfig;
use gba_config::GbaColorCorrection;
use gba_core::api::{GbaAudioConfig, GbaEmulatorConfig};
use genesis_config::{S32XVoidColor, S32XVoidColorType};
use genesis_core::GenesisEmulatorConfig;
use jgenesis_common::frontend::{ColorCorrection, FiniteF32};
use jgenesis_native_config::common::{
    ConfigSavePath, HideMouseCursor, PauseEmulator, SavePath, WindowSize,
};
use jgenesis_native_config::input::mappings::{
    GameBoyInputConfig, GenesisInputConfig, HotkeyConfig, NesInputConfig, SmsGgInputConfig,
    SnesInputConfig,
};
use jgenesis_native_config::{AppConfig, EguiTheme};
use jgenesis_proc_macros::ConfigDisplay;
use jgenesis_renderer::config::{PrescaleMode, RendererConfig};
use nes_core::api::NesEmulatorConfig;
use s32x_core::api::Sega32XEmulatorConfig;
use segacd_core::api::SegaCdEmulatorConfig;
use smsgg_core::{SmsGgEmulatorConfig, SmsGgHardware};
use snes_core::api::{CoprocessorRomFn, CoprocessorRoms, SnesEmulatorConfig};
use std::fs;
use std::num::NonZeroU8;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct RomReadResult {
    pub rom: Vec<u8>,
    pub extension: String,
}

struct NameWithExtension {
    file_name: String,
    extension: String,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct CommonConfig {
    #[cfg_display(path)]
    pub rom_file_path: PathBuf,
    pub mute_audio: bool,
    pub audio_output_frequency: u64,
    pub audio_sync: bool,
    pub audio_dynamic_resampling_ratio: bool,
    pub audio_buffer_size: u32,
    pub audio_hardware_queue_size: u32,
    pub audio_gain_db: f64,
    pub save_path: SavePath,
    pub state_path: SavePath,
    #[cfg_display(debug_fmt)]
    pub window_size: Option<WindowSize>,
    #[cfg_display(debug_fmt)]
    pub window_scale_factor: Option<f32>,
    #[cfg_display(indent_nested)]
    pub renderer_config: RendererConfig,
    pub fast_forward_multiplier: u64,
    pub rewind_buffer_length_seconds: u64,
    pub load_recent_state_at_launch: bool,
    pub launch_in_fullscreen: bool,
    pub initial_window_size: NonZeroU8,
    pub axis_deadzone: i16,
    #[cfg_display(indent_nested)]
    pub hotkey_config: HotkeyConfig,
    pub pause_emulator: PauseEmulator,
    pub hide_mouse_cursor: HideMouseCursor,
    pub egui_theme: EguiTheme,
}

impl CommonConfig {
    pub(crate) fn read_rom_file(
        &self,
        supported_extensions: &[&str],
    ) -> NativeEmulatorResult<RomReadResult> {
        #[derive(Default)]
        struct ArchiveListCallback {
            first_supported_file: Option<NameWithExtension>,
        }

        impl ArchiveListCallback {
            fn as_fn_mut<'ext>(
                &mut self,
                supported_extensions: &'ext [&str],
            ) -> impl FnMut(ArchiveEntry<'_>) + use<'_, 'ext> {
                |entry| {
                    if self.first_supported_file.is_some() {
                        return;
                    }

                    let Some(extension) = extensions::from_path(entry.file_name) else { return };
                    if supported_extensions.contains(&extension.as_str()) {
                        self.first_supported_file = Some(NameWithExtension {
                            file_name: entry.file_name.into(),
                            extension,
                        });
                    }
                }
            }

            fn open_file(
                self,
                archive_path: &Path,
                read_fn: fn(&Path, &str) -> Result<Vec<u8>, ArchiveError>,
            ) -> NativeEmulatorResult<RomReadResult> {
                let first_supported_file = self.first_supported_file.ok_or_else(|| {
                    NativeEmulatorError::Archive(ArchiveError::NoSupportedFiles {
                        path: archive_path.display().to_string(),
                    })
                })?;

                let contents = read_fn(archive_path, &first_supported_file.file_name)
                    .map_err(NativeEmulatorError::Archive)?;
                Ok(RomReadResult { rom: contents, extension: first_supported_file.extension })
            }
        }

        let path = &self.rom_file_path;
        let extension = extensions::from_path(path).unwrap_or_default();
        match extension.as_str() {
            "zip" => {
                let mut callback = ArchiveListCallback::default();
                archive::list_files_zip(path, callback.as_fn_mut(supported_extensions))
                    .map_err(NativeEmulatorError::Archive)?;
                callback.open_file(path, archive::read_file_zip)
            }
            "7z" => {
                let mut callback = ArchiveListCallback::default();
                archive::list_files_7z(path, callback.as_fn_mut(supported_extensions))
                    .map_err(NativeEmulatorError::Archive)?;
                callback.open_file(path, archive::read_file_7z)
            }
            _ => {
                let contents = fs::read(path).map_err(|source| NativeEmulatorError::RomRead {
                    path: path.display().to_string(),
                    source,
                })?;

                Ok(RomReadResult { rom: contents, extension })
            }
        }
    }
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SmsGgConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: SmsGgInputConfig,
    #[cfg_display(debug_fmt)]
    pub hardware: Option<SmsGgHardware>,
    #[cfg_display(indent_nested)]
    pub emulator_config: SmsGgEmulatorConfig,
    pub sms_boot_from_bios: bool,
    pub gg_boot_from_bios: bool,
    pub run_without_cartridge: bool,
    #[cfg_display(path)]
    pub sms_bios_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub gg_bios_path: Option<PathBuf>,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GenesisConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: GenesisInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: GenesisEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SegaCdConfig {
    #[cfg_display(indent_nested)]
    pub genesis: GenesisConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: SegaCdEmulatorConfig,
    #[cfg_display(path)]
    pub bios_file_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub eu_bios_file_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub jp_bios_file_path: Option<PathBuf>,
    pub per_region_bios: bool,
    pub run_without_disc: bool,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct Sega32XConfig {
    #[cfg_display(indent_nested)]
    pub genesis: GenesisConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: Sega32XEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct NesConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: NesInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: NesEmulatorConfig,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct SnesConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: SnesInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: SnesEmulatorConfig,
    #[cfg_display(path)]
    pub dsp1_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp2_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp3_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub dsp4_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub st010_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub st011_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub st018_rom_path: Option<PathBuf>,
}

impl SnesConfig {
    pub(crate) fn to_coprocessor_roms(&self) -> CoprocessorRoms {
        let dsp1 = self.dsp1_rom_path.clone().map(coprocessor_read_fn);
        let dsp2 = self.dsp2_rom_path.clone().map(coprocessor_read_fn);
        let dsp3 = self.dsp3_rom_path.clone().map(coprocessor_read_fn);
        let dsp4 = self.dsp4_rom_path.clone().map(coprocessor_read_fn);
        let st010 = self.st010_rom_path.clone().map(coprocessor_read_fn);
        let st011 = self.st011_rom_path.clone().map(coprocessor_read_fn);
        let st018 = self.st018_rom_path.clone().map(coprocessor_read_fn);

        CoprocessorRoms { dsp1, dsp2, dsp3, dsp4, st010, st011, st018 }
    }
}

fn coprocessor_read_fn(path: PathBuf) -> Box<CoprocessorRomFn> {
    Box::new(move || fs::read(&path).map_err(|err| (err, path.display().to_string())))
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GameBoyConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: GameBoyInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: GameBoyEmulatorConfig,
    pub dmg_boot_rom: bool,
    pub cgb_boot_rom: bool,
    #[cfg_display(path)]
    pub dmg_boot_rom_path: Option<PathBuf>,
    #[cfg_display(path)]
    pub cgb_boot_rom_path: Option<PathBuf>,
}

#[derive(Debug, Clone, ConfigDisplay)]
pub struct GameBoyAdvanceConfig {
    #[cfg_display(indent_nested)]
    pub common: CommonConfig,
    #[cfg_display(indent_nested)]
    pub inputs: jgenesis_native_config::input::mappings::GbaInputConfig,
    #[cfg_display(indent_nested)]
    pub emulator_config: GbaEmulatorConfig,
    #[cfg_display(path)]
    pub bios_path: Option<PathBuf>,
    pub solar_brightness_step: u8,
    pub solar_min_brightness: u8,
    pub solar_max_brightness: u8,
}

pub trait AppConfigExt {
    #[must_use]
    fn common_config(&self, path: PathBuf) -> CommonConfig;

    #[must_use]
    fn genesis_config(&self, path: PathBuf) -> Box<GenesisConfig>;

    #[must_use]
    fn sega_cd_config(&self, path: PathBuf) -> Box<SegaCdConfig>;

    #[must_use]
    fn sega_32x_config(&self, path: PathBuf) -> Box<Sega32XConfig>;

    #[must_use]
    fn smsgg_config(&self, path: PathBuf, hardware: Option<SmsGgHardware>) -> Box<SmsGgConfig>;

    #[must_use]
    fn nes_config(&self, path: PathBuf) -> Box<NesConfig>;

    #[must_use]
    fn snes_config(&self, path: PathBuf) -> Box<SnesConfig>;

    #[must_use]
    fn gb_config(&self, path: PathBuf) -> Box<GameBoyConfig>;

    #[must_use]
    fn gba_config(&self, path: PathBuf) -> Box<GameBoyAdvanceConfig>;
}

impl AppConfigExt for AppConfig {
    fn common_config(&self, path: PathBuf) -> CommonConfig {
        fn save_path(path: ConfigSavePath, custom_path: &Path) -> SavePath {
            match path {
                ConfigSavePath::RomFolder => SavePath::RomFolder,
                ConfigSavePath::EmulatorFolder => SavePath::EmulatorFolder,
                ConfigSavePath::Custom => SavePath::Custom(custom_path.into()),
            }
        }

        CommonConfig {
            rom_file_path: path,
            mute_audio: self.common.mute_audio,
            audio_output_frequency: self.common.audio_output_frequency,
            audio_sync: self.common.audio_sync,
            audio_dynamic_resampling_ratio: self.common.audio_dynamic_resampling_ratio,
            audio_buffer_size: self.common.audio_buffer_size,
            audio_hardware_queue_size: self.common.audio_hardware_queue_size,
            audio_gain_db: self.common.audio_gain_db,
            save_path: save_path(self.common.save_path, &self.common.custom_save_path),
            state_path: save_path(self.common.state_path, &self.common.custom_state_path),
            window_size: self.common.window_size(),
            window_scale_factor: self.common.window_scale_factor,
            renderer_config: RendererConfig {
                wgpu_backend: self.common.wgpu_backend,
                vsync_mode: self.common.vsync_mode,
                frame_time_sync: self.common.frame_time_sync,
                prescale_mode: if self.common.auto_prescale {
                    PrescaleMode::Auto
                } else {
                    PrescaleMode::Manual(self.common.prescale_factor)
                },
                scanlines: self.common.scanlines,
                force_integer_height_scaling: self.common.force_integer_height_scaling,
                filter_mode: self.common.filter_mode,
                preprocess_shader: self.common.preprocess_shader,
                use_webgl2_limits: false,
            },
            fast_forward_multiplier: self.common.fast_forward_multiplier,
            rewind_buffer_length_seconds: self.common.rewind_buffer_length_seconds,
            load_recent_state_at_launch: self.common.load_recent_state_at_launch,
            launch_in_fullscreen: self.common.launch_in_fullscreen,
            initial_window_size: self.common.initial_window_size,
            axis_deadzone: self.input.axis_deadzone,
            hotkey_config: self.input.hotkeys.clone(),
            pause_emulator: self.common.pause_emulator,
            hide_mouse_cursor: self.common.hide_mouse_cursor,
            egui_theme: self.egui_theme,
        }
    }

    fn genesis_config(&self, path: PathBuf) -> Box<GenesisConfig> {
        Box::new(GenesisConfig {
            common: self.common_config(path),
            inputs: self.input.genesis.clone(),
            emulator_config: GenesisEmulatorConfig {
                p1_controller_type: self.input.genesis.p1_type,
                p2_controller_type: self.input.genesis.p2_type,
                forced_timing_mode: self.genesis.forced_timing_mode,
                forced_region: self.genesis.forced_region,
                aspect_ratio: self.genesis.aspect_ratio,
                force_square_pixels_in_h40: self.genesis.force_square_pixels_in_h40,
                adjust_aspect_ratio_in_2x_resolution: self
                    .genesis
                    .adjust_aspect_ratio_in_2x_resolution,
                remove_sprite_limits: self.genesis.remove_sprite_limits,
                m68k_clock_divider: self.genesis.m68k_clock_divider,
                non_linear_color_scale: self.genesis.non_linear_color_scale,
                deinterlace: self.genesis.deinterlace,
                render_vertical_border: self.genesis.render_vertical_border,
                render_horizontal_border: self.genesis.render_horizontal_border,
                plane_a_enabled: self.genesis.plane_a_enabled,
                plane_b_enabled: self.genesis.plane_b_enabled,
                sprites_enabled: self.genesis.sprites_enabled,
                window_enabled: self.genesis.window_enabled,
                backdrop_enabled: self.genesis.backdrop_enabled,
                quantize_ym2612_output: self.genesis.quantize_ym2612_output,
                emulate_ym2612_ladder_effect: self.genesis.emulate_ym2612_ladder_effect,
                opn2_busy_behavior: self.genesis.opn2_busy_behavior,
                genesis_lpf_enabled: self.genesis.genesis_lpf_enabled,
                genesis_lpf_cutoff: self.genesis.genesis_lpf_cutoff,
                ym2612_2nd_lpf_enabled: self.genesis.ym2612_2nd_lpf_enabled,
                ym2612_2nd_lpf_cutoff: self.genesis.ym2612_2nd_lpf_cutoff,
                ym2612_channels_enabled: self.genesis.ym2612_channels_enabled,
                ym2612_enabled: self.genesis.ym2612_enabled,
                psg_enabled: self.genesis.psg_enabled,
                ym2612_volume_adjustment_db: self.genesis.ym2612_volume_adjustment_db,
                psg_volume_adjustment_db: self.genesis.psg_volume_adjustment_db,
            },
        })
    }

    fn sega_cd_config(&self, path: PathBuf) -> Box<SegaCdConfig> {
        let genesis_config = *self.genesis_config(path);
        let genesis_emu_config = genesis_config.emulator_config;
        Box::new(SegaCdConfig {
            genesis: genesis_config,
            bios_file_path: self.sega_cd.bios_path.clone(),
            eu_bios_file_path: self.sega_cd.eu_bios_path.clone(),
            jp_bios_file_path: self.sega_cd.jp_bios_path.clone(),
            per_region_bios: self.sega_cd.per_region_bios,
            run_without_disc: false,
            emulator_config: SegaCdEmulatorConfig {
                genesis: genesis_emu_config,
                pcm_interpolation: self.sega_cd.pcm_interpolation,
                enable_ram_cartridge: self.sega_cd.enable_ram_cartridge,
                load_disc_into_ram: self.sega_cd.load_disc_into_ram,
                disc_drive_speed: self.sega_cd.disc_drive_speed,
                sub_cpu_divider: self.sega_cd.sub_cpu_divider,
                pcm_lpf_enabled: self.sega_cd.pcm_lpf_enabled,
                pcm_lpf_cutoff: self.sega_cd.pcm_lpf_cutoff,
                apply_genesis_lpf_to_pcm: self.sega_cd.apply_genesis_lpf_to_pcm,
                apply_genesis_lpf_to_cd_da: self.sega_cd.apply_genesis_lpf_to_cd_da,
                pcm_enabled: self.sega_cd.pcm_enabled,
                cd_audio_enabled: self.sega_cd.cd_audio_enabled,
                pcm_volume_adjustment_db: self.sega_cd.pcm_volume_adjustment_db,
                cd_volume_adjustment_db: self.sega_cd.cd_volume_adjustment_db,
            },
        })
    }

    fn sega_32x_config(&self, path: PathBuf) -> Box<Sega32XConfig> {
        let genesis_config = *self.genesis_config(path);
        let genesis_emu_config = genesis_config.emulator_config;
        Box::new(Sega32XConfig {
            genesis: genesis_config,
            emulator_config: Sega32XEmulatorConfig {
                genesis: genesis_emu_config,
                sh2_clock_multiplier: self.sega_32x.sh2_clock_multiplier,
                video_out: self.sega_32x.video_out,
                darken_genesis_colors: self.sega_32x.darken_genesis_colors,
                color_tint: self.sega_32x.color_tint,
                show_high_priority: self.sega_32x.show_high_priority,
                show_low_priority: self.sega_32x.show_low_priority,
                void_color: match self.sega_32x.void_color_type {
                    S32XVoidColorType::PaletteRam => {
                        S32XVoidColor::PaletteRam { idx: self.sega_32x.void_palette_index }
                    }
                    S32XVoidColorType::Direct => {
                        let [r, g, b] = self.sega_32x.void_direct;
                        S32XVoidColor::Direct { r, g, b, a: self.sega_32x.void_direct_priority }
                    }
                },
                emulate_pixel_switch_delay: self.sega_32x.emulate_pixel_switch_delay,
                apply_genesis_lpf_to_pwm: self.sega_32x.apply_genesis_lpf_to_pwm,
                pwm_resampling: self.sega_32x.pwm_resampling,
                pwm_enabled: self.sega_32x.pwm_enabled,
                pwm_volume_adjustment_db: self.sega_32x.pwm_volume_adjustment_db,
            },
        })
    }

    fn smsgg_config(&self, path: PathBuf, hardware: Option<SmsGgHardware>) -> Box<SmsGgConfig> {
        Box::new(SmsGgConfig {
            common: self.common_config(path),
            inputs: self.input.smsgg.clone(),
            hardware,
            emulator_config: SmsGgEmulatorConfig {
                sms_timing_mode: self.smsgg.sms_timing_mode,
                sms_model: self.smsgg.sms_model,
                forced_psg_version: self.smsgg.psg_version,
                remove_sprite_limit: self.smsgg.remove_sprite_limit,
                sms_aspect_ratio: self.smsgg.sms_aspect_ratio,
                gg_aspect_ratio: self.smsgg.gg_aspect_ratio,
                forced_region: self.smsgg.forced_region,
                sms_crop_vertical_border: self.smsgg.sms_crop_vertical_border,
                sms_crop_left_border: self.smsgg.sms_crop_left_border,
                gg_frame_blending: self.smsgg.gg_frame_blending,
                gg_use_sms_resolution: self.smsgg.gg_use_sms_resolution,
                fm_sound_unit_enabled: self.smsgg.fm_sound_unit_enabled,
                z80_divider: self.smsgg.z80_divider,
            },
            sms_boot_from_bios: self.smsgg.sms_boot_from_bios,
            gg_boot_from_bios: self.smsgg.gg_boot_from_bios,
            run_without_cartridge: false,
            sms_bios_path: self.smsgg.sms_bios_path.clone(),
            gg_bios_path: self.smsgg.gg_bios_path.clone(),
        })
    }

    fn nes_config(&self, path: PathBuf) -> Box<NesConfig> {
        Box::new(NesConfig {
            common: self.common_config(path),
            inputs: self.input.nes.clone(),
            emulator_config: NesEmulatorConfig {
                forced_timing_mode: self.nes.forced_timing_mode,
                aspect_ratio: self.nes.aspect_ratio,
                palette: self.nes.palette,
                ntsc_crop_vertical_overscan: self.nes.ntsc_crop_vertical_overscan,
                overscan: self.nes.overscan,
                remove_sprite_limit: self.nes.remove_sprite_limit,
                pal_black_border: self.nes.pal_black_border,
                silence_ultrasonic_triangle_output: self.nes.silence_ultrasonic_triangle_output,
                audio_resampler: self.nes.audio_resampler,
                audio_refresh_rate_adjustment: self.nes.audio_60hz_hack,
                allow_opposing_joypad_inputs: self.nes.allow_opposing_joypad_inputs,
                dma_dummy_joy_reads: self.nes.dma_dummy_joy_reads,
            },
        })
    }

    fn snes_config(&self, path: PathBuf) -> Box<SnesConfig> {
        Box::new(SnesConfig {
            common: self.common_config(path),
            inputs: self.input.snes.clone(),
            emulator_config: SnesEmulatorConfig {
                forced_timing_mode: self.snes.forced_timing_mode,
                aspect_ratio: self.snes.aspect_ratio,
                deinterlace: self.snes.deinterlace,
                audio_interpolation: self.snes.audio_interpolation,
                audio_60hz_hack: self.snes.audio_60hz_hack,
                gsu_overclock_factor: self.snes.gsu_overclock_factor,
            },
            dsp1_rom_path: self.snes.dsp1_rom_path.clone(),
            dsp2_rom_path: self.snes.dsp2_rom_path.clone(),
            dsp3_rom_path: self.snes.dsp3_rom_path.clone(),
            dsp4_rom_path: self.snes.dsp4_rom_path.clone(),
            st010_rom_path: self.snes.st010_rom_path.clone(),
            st011_rom_path: self.snes.st011_rom_path.clone(),
            st018_rom_path: self.snes.st018_rom_path.clone(),
        })
    }

    fn gb_config(&self, path: PathBuf) -> Box<GameBoyConfig> {
        let color_correction = match self.game_boy.gbc_color_correction {
            GbcColorCorrection::None => ColorCorrection::None,
            GbcColorCorrection::GbcLcd => ColorCorrection::GbcLcd {
                screen_gamma: convert_color_correct_gamma(self.game_boy.gbc_correction_gamma),
            },
            GbcColorCorrection::GbaLcd => ColorCorrection::GbaLcd {
                screen_gamma: convert_color_correct_gamma(self.game_boy.gba_correction_gamma),
            },
        };

        Box::new(GameBoyConfig {
            common: self.common_config(path),
            inputs: self.input.game_boy.clone(),
            emulator_config: GameBoyEmulatorConfig {
                force_dmg_mode: self.game_boy.force_dmg_mode,
                force_cgb_mode: self.game_boy.force_cgb_mode,
                pretend_to_be_gba: self.game_boy.pretend_to_be_gba,
                aspect_ratio: self.game_boy.aspect_ratio,
                gb_palette: self.game_boy.gb_palette,
                gb_custom_palette: self.game_boy.gb_custom_palette,
                gbc_color_correction: color_correction,
                frame_blending: self.game_boy.frame_blending,
                audio_resampler: self.game_boy.audio_resampler,
                audio_60hz_hack: self.game_boy.audio_60hz_hack,
            },
            dmg_boot_rom: self.game_boy.dmg_boot_rom,
            cgb_boot_rom: self.game_boy.cgb_boot_rom,
            dmg_boot_rom_path: self.game_boy.dmg_boot_rom_path.clone(),
            cgb_boot_rom_path: self.game_boy.cgb_boot_rom_path.clone(),
        })
    }

    fn gba_config(&self, path: PathBuf) -> Box<GameBoyAdvanceConfig> {
        let color_correction = match self.game_boy_advance.color_correction {
            GbaColorCorrection::None => ColorCorrection::None,
            GbaColorCorrection::GbaLcd => ColorCorrection::GbaLcd {
                screen_gamma: convert_color_correct_gamma(
                    self.game_boy_advance.color_correction_gamma,
                ),
            },
        };

        Box::new(GameBoyAdvanceConfig {
            common: self.common_config(path),
            inputs: self.input.game_boy_advance.clone(),
            emulator_config: GbaEmulatorConfig {
                skip_bios_animation: self.game_boy_advance.skip_bios_animation,
                aspect_ratio: self.game_boy_advance.aspect_ratio,
                color_correction,
                frame_blending: self.game_boy_advance.frame_blending,
                forced_save_memory_type: self.game_boy_advance.forced_save_memory_type,
                audio: GbaAudioConfig {
                    audio_interpolation: self.game_boy_advance.audio_interpolation,
                    psg_low_pass: self.game_boy_advance.psg_low_pass,
                    pulse_1_enabled: self.game_boy_advance.pulse_1_enabled,
                    pulse_2_enabled: self.game_boy_advance.pulse_2_enabled,
                    wavetable_enabled: self.game_boy_advance.wavetable_enabled,
                    noise_enabled: self.game_boy_advance.noise_enabled,
                    pcm_a_enabled: self.game_boy_advance.pcm_a_enabled,
                    pcm_b_enabled: self.game_boy_advance.pcm_b_enabled,
                },
            },
            bios_path: self.game_boy_advance.bios_path.clone(),
            solar_brightness_step: self.game_boy_advance.solar_brightness_step,
            solar_min_brightness: self.game_boy_advance.solar_min_brightness,
            solar_max_brightness: self.game_boy_advance.solar_max_brightness,
        })
    }
}

fn convert_color_correct_gamma(gamma: f64) -> FiniteF32 {
    FiniteF32::try_from(gamma as f32).unwrap_or(fallback_color_correct_gamma())
}

fn fallback_color_correct_gamma() -> FiniteF32 {
    FiniteF32::try_from(2.2).unwrap()
}
