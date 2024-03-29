use crate::config::{SmsGgConfig, WindowSize};
use crate::input::{HotkeyMapper, InputMapper};
use crate::mainloop::audio::SdlAudioOutput;
use crate::mainloop::save::FsSaveWriter;
use crate::mainloop::{
    create_window, debug, file_name_no_ext, init_sdl, parse_file_ext, HotkeyState,
    NativeEmulatorError,
};
use crate::{config, AudioError, NativeEmulator, NativeEmulatorResult};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_renderer::renderer::WgpuRenderer;
use sdl2::video::Window;
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgButton, SmsGgEmulator, SmsGgEmulatorConfig, SmsGgInputs};
use std::fs;
use std::path::Path;

pub type NativeSmsGgEmulator =
    NativeEmulator<SmsGgInputs, SmsGgButton, SmsGgEmulatorConfig, SmsGgEmulator>;

impl NativeSmsGgEmulator {
    /// # Errors
    ///
    /// This method will return an error if it is unable to reload audio config.
    pub fn reload_smsgg_config(&mut self, config: Box<SmsGgConfig>) -> Result<(), AudioError> {
        log::info!("Reloading config: {config}");

        self.reload_common_config(&config.common)?;

        let vdp_version = config.vdp_version.unwrap_or_else(|| self.emulator.vdp_version());
        let psg_version = config.psg_version.unwrap_or_else(|| {
            if vdp_version.is_master_system() {
                PsgVersion::MasterSystem2
            } else {
                PsgVersion::Standard
            }
        });

        let emulator_config = config.to_emulator_config(vdp_version, psg_version);
        self.emulator.reload_config(&emulator_config);
        self.config = emulator_config;

        if let Err(err) = self.input_mapper.reload_config(
            config.common.keyboard_inputs,
            config.common.joystick_inputs,
            config.common.axis_deadzone,
            &SmsGgButton::ALL,
        ) {
            log::error!("Error reloading input config: {err}");
        }

        Ok(())
    }
}

/// Create an emulator with the SMS/GG core with the given config.
///
/// # Errors
///
/// This function will propagate any video, audio, or disk errors encountered.
pub fn create_smsgg(config: Box<SmsGgConfig>) -> NativeEmulatorResult<NativeSmsGgEmulator> {
    log::info!("Running with config: {config}");

    let rom_file_path = Path::new(&config.common.rom_file_path);
    let file_ext = parse_file_ext(rom_file_path)?;

    let save_state_path = rom_file_path.with_extension("ss0");

    let rom = fs::read(rom_file_path).map_err(|source| NativeEmulatorError::RomRead {
        path: rom_file_path.display().to_string(),
        source,
    })?;

    let save_path = rom_file_path.with_extension("sav");
    let mut save_writer = FsSaveWriter::new(save_path);

    let vdp_version =
        config.vdp_version.unwrap_or_else(|| config::default_vdp_version_for_ext(file_ext));
    let psg_version =
        config.psg_version.unwrap_or_else(|| config::default_psg_version_for_ext(file_ext));

    log::info!("VDP version: {vdp_version:?}");
    log::info!("PSG version: {psg_version:?}");

    let (sdl, video, audio, joystick, event_pump) =
        init_sdl(config.common.hide_cursor_over_window)?;

    let WindowSize { width: window_width, height: window_height } =
        config.common.window_size.unwrap_or_else(|| config::default_smsgg_window_size(vdp_version));

    let rom_title = file_name_no_ext(rom_file_path)?;
    let window = create_window(
        &video,
        &format!("smsgg - {rom_title}"),
        window_width,
        window_height,
        config.common.launch_in_fullscreen,
    )?;

    let emulator_config = config.to_emulator_config(vdp_version, psg_version);

    let renderer =
        pollster::block_on(WgpuRenderer::new(window, Window::size, config.common.renderer_config))?;
    let audio_output = SdlAudioOutput::create_and_init(&audio, &config.common)?;
    let input_mapper = InputMapper::new(
        joystick,
        config.common.keyboard_inputs.clone(),
        config.common.joystick_inputs.clone(),
        config.common.axis_deadzone,
        &SmsGgButton::ALL,
    )?;
    let hotkey_mapper = HotkeyMapper::from_config(&config.common.hotkeys)?;

    let emulator = SmsGgEmulator::create(rom, emulator_config, &mut save_writer);

    Ok(NativeEmulator {
        emulator,
        config: emulator_config,
        renderer,
        audio_output,
        input_mapper,
        hotkey_mapper,
        save_writer,
        sdl,
        event_pump,
        video,
        hotkey_state: HotkeyState::new(&config.common, save_state_path, debug::smsgg::render_fn),
    })
}
