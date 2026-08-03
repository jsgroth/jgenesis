use clap::{CommandFactory, Parser};
use egui_sdl3_wgpu::{FrameOptions, FrameRunEffect};
use env_logger::Env;
use image::{DynamicImage, ImageFormat};
use jgenesis_common::frontend::EmulatorTrait;
use jgenesis_gui::GuiEmulatorRunner;
use jgenesis_gui::app::{App, ConfigInfo, LoadAtStartup};
use jgenesis_native_config::AppConfig;
use jgenesis_native_config::paths::{ConfigDirType, ConfigDirs, ConfigWithPath};
use jgenesis_native_driver::config::AppConfigExt;
use jgenesis_native_driver::extensions::Console;
use jgenesis_native_driver::{
    NativeEmulator, NativeGameBoyEmulator, NativeGbaEmulator, NativeGenesisEmulator,
    NativeNesEmulator, NativePcEngineEmulator, NativeSmsGgEmulator, NativeSnesEmulator,
    NativeTickEffect, SdlSubsystems,
};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[allow(clippy::doc_markdown)]
#[derive(Debug, Parser)]
struct Args {
    /// Use a specific config file path instead of the default
    #[arg(long = "config")]
    config_path: Option<PathBuf>,

    /// If set, the application will open this file immediately after starting up, and it will exit
    /// when the emulator window is closed (or if there is an error).
    ///
    /// For platforms with multiple file inputs (e.g. Sega CD 32X), you can set this argument
    /// multiple times, e.g. '-f /path/to/cartridge.32x -f /path/to/disc.cue'
    #[arg(long = "file-path", short = 'f')]
    startup_file_paths: Vec<PathBuf>,

    #[arg(long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    /// Run without opening the GUI window. Can only be used in combination with -f / --file-path
    no_gui: bool,

    /// In combination with -f / --file-path, specify what hardware to emulate. Will auto-detect based on file if
    /// not set
    #[arg(long = "hardware")]
    console: Option<Console>,

    /// Attempt to load the specified save state when launching the game. Can only be used in
    /// combination with -f / --file-path
    #[arg(long, value_name = "SLOT")]
    load_save_state: Option<usize>,

    /// Override an individual config setting without modifying the settings file. Can only be used
    /// in combination with -f / --file-path
    ///
    /// Parameter value should be in the form 'key=value', e.g. '--config-override genesis.remove_sprite_limits=true'.
    /// Can be set multiple times to override multiple settings
    #[arg(long = "config-override")]
    config_overrides: Vec<String>,

    /// Print version string and immediately exit
    #[arg(short = 'v', long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    version: bool,
}

impl Args {
    fn fix_appimage_relative_paths(mut self) -> Self {
        if let Some(config_path) = self.config_path {
            self.config_path = Some(jgenesis_common::fix_appimage_relative_path(config_path));
        }

        self.startup_file_paths = self
            .startup_file_paths
            .into_iter()
            .map(jgenesis_common::fix_appimage_relative_path)
            .collect();

        self
    }

    fn load_at_startup(&self) -> Option<LoadAtStartup> {
        if self.startup_file_paths.is_empty() {
            return None;
        }

        Some(LoadAtStartup {
            file_paths: self.startup_file_paths.clone(),
            console: self.console,
            load_state_slot: self.load_save_state,
            config_overrides: self.config_overrides.clone(),
        })
    }

    fn validate_or_exit(&self) {
        let file_path_present = !self.startup_file_paths.is_empty();

        if self.console.is_some() && !file_path_present {
            Self::command()
                .error(
                    clap::error::ErrorKind::ValueValidation,
                    "--hardware can only be used with the -f / --file-path arg",
                )
                .exit();
        }

        if self.load_save_state.is_some() && !file_path_present {
            Self::command()
                .error(
                    clap::error::ErrorKind::ValueValidation,
                    "--load-save-state can only be used with the -f / --file-path arg",
                )
                .exit();
        }

        if self.no_gui && !file_path_present {
            Self::command()
                .error(
                    clap::error::ErrorKind::ValueValidation,
                    "--no-gui can only be used with the -f / --file-path arg",
                )
                .exit();
        }

        if !self.config_overrides.is_empty() && !file_path_present {
            Self::command()
                .error(
                    clap::error::ErrorKind::ValueValidation,
                    "--config-override can only be used with the -f / --file-path arg",
                )
                .exit();
        }
    }
}

fn initial_gui_size(config: &AppConfig) -> (f32, f32) {
    (
        f32_max(jgenesis_native_config::DEFAULT_GUI_WIDTH, config.gui_window_width),
        f32_max(jgenesis_native_config::DEFAULT_GUI_HEIGHT, config.gui_window_height),
    )
}

fn f32_max(value: f32, max: f32) -> f32 {
    if value < max { max } else { value }
}

fn load_icon() -> DynamicImage {
    const ICON: &[u8] = include_bytes!("../../256x256.png");

    image::load_from_memory_with_format(ICON, ImageFormat::Png).expect("Failed to load GUI icon")
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        Env::default().default_filter_or(jgenesis_common::DEFAULT_LOG_FILTER),
    )
    .init();

    let args = Args::parse().fix_appimage_relative_paths();

    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    args.validate_or_exit();

    let config_dirs = ConfigDirs::new();
    let config_dir_type = config_dirs.default_dir_type(args.config_path.clone());
    let config_with_path = ConfigWithPath::load_from_dir_or_default(
        &config_dirs,
        &config_dir_type,
        AppConfig::default,
    );

    if let Some(file_path) = args.startup_file_paths.first() {
        log::info!("Will open file '{}' after starting", file_path.display());
    }

    let sdl = SdlSubsystems::init()?;

    let load_at_startup = args.load_at_startup();

    if args.no_gui
        && let Some(load_at_startup) = &load_at_startup
    {
        run_no_gui(sdl, config_with_path, load_at_startup)
    } else {
        run_with_gui(sdl, config_with_path, config_dirs, config_dir_type, load_at_startup)
    }
}

fn run_with_gui(
    sdl: SdlSubsystems,
    config_with_path: ConfigWithPath,
    config_dirs: ConfigDirs,
    config_dir_type: ConfigDirType,
    load_at_startup: Option<LoadAtStartup>,
) -> anyhow::Result<()> {
    let (gui_width, gui_height) = initial_gui_size(&config_with_path.config);

    let icon = load_icon();

    let config_info = ConfigInfo {
        initial_config: config_with_path.config,
        config_path: config_with_path.path,
        config_dirs,
        config_dir_type,
    };

    let options = FrameOptions {
        window_width: gui_width.round() as u32,
        window_height: gui_height.round() as u32,
        icon: Some(icon),
        ..FrameOptions::default()
    };

    let mut frame = egui_sdl3_wgpu::Frame::new("jgenesis", &sdl.video.borrow(), options)?;

    let (mut emu_runner, emu_runner_handle) =
        GuiEmulatorRunner::new(sdl.clone(), frame.egui_ctx().clone());

    let mut app =
        App::new(config_info, load_at_startup, frame.egui_ctx().clone(), &sdl, emu_runner_handle);

    loop {
        emu_runner.run(&mut |event| {
            frame.handle_sdl_event(event);
            app.handle_sdl_event(event, frame.egui_ctx(), frame.window_id());
        });

        if frame.run(&mut |ui, _ctx| app.ui(ui))? == FrameRunEffect::Closed {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(1));
    }
}

struct RunEmulatorArgs<'a> {
    sdl: SdlSubsystems,
    config: AppConfig,
    config_path: &'a Path,
    console: Console,
    primary_path: PathBuf,
    secondary_paths: &'a [PathBuf],
    load_state_slot: Option<usize>,
}

fn run_no_gui(
    sdl: SdlSubsystems,
    ConfigWithPath { mut config, path: config_path }: ConfigWithPath,
    load_at_startup: &LoadAtStartup,
) -> anyhow::Result<()> {
    if let Err(err) = config.apply_overrides(&load_at_startup.config_overrides) {
        log::error!("Error applying config overrides: {err}");
    }

    let primary_path = load_at_startup.file_paths.first().unwrap();
    let secondary_paths = &load_at_startup.file_paths[1..];

    let console = load_at_startup
        .console
        .or_else(|| Console::from_file(primary_path).map(|console| console.console))
        .unwrap_or_else(|| {
            log::error!(
                "Unable to determine console for file '{}'; defaulting to Genesis",
                primary_path.display()
            );
            Console::Genesis
        });

    let run_args = RunEmulatorArgs {
        sdl,
        config,
        config_path: config_path.as_ref(),
        console,
        primary_path: primary_path.clone(),
        secondary_paths,
        load_state_slot: load_at_startup.load_state_slot,
    };

    match console {
        Console::MasterSystem | Console::GameGear | Console::Sg1000 => run_smsgg(run_args),
        Console::Genesis | Console::SegaCd | Console::Sega32X | Console::SegaCd32X => {
            run_genesis(run_args)
        }
        Console::Nes => run_nes(run_args),
        Console::Snes => run_snes(run_args),
        Console::GameBoy | Console::GameBoyColor => run_gb(run_args),
        Console::GameBoyAdvance => run_gba(run_args),
        Console::PcEngine => run_pce(run_args),
    }
}

fn run_smsgg(
    RunEmulatorArgs { sdl, config, config_path, console, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let cheats = config
        .try_load_cheats_if_enabled(config_path, &primary_path, console.standard_extension())
        .unwrap_or_default();
    let emulator = NativeSmsGgEmulator::create(
        sdl,
        config.smsgg_config(primary_path, console.to_smsgg_hardware(), &cheats),
    )?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_genesis(
    RunEmulatorArgs {
        sdl,
        config,
        config_path,
        console,
        primary_path,
        secondary_paths,
        load_state_slot,
    }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let cheats = config
        .try_load_cheats_if_enabled(config_path, &primary_path, console.standard_extension())
        .unwrap_or_default();
    let emulator = NativeGenesisEmulator::create(
        sdl,
        config.genesis_config(
            primary_path,
            secondary_paths.first().cloned(),
            console.to_genesis_hardware(),
            &cheats,
        ),
    )?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_nes(
    RunEmulatorArgs { sdl, config, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let emulator = NativeNesEmulator::create(sdl, config.nes_config(primary_path))?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_snes(
    RunEmulatorArgs { sdl, config, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let emulator = NativeSnesEmulator::create(sdl, config.snes_config(primary_path))?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_gb(
    RunEmulatorArgs { sdl, mut config, console, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    // TODO this should be an arg to gb_config() somehow
    match console {
        Console::GameBoy => {
            config.game_boy.force_dmg_mode = true;
        }
        Console::GameBoyColor => {
            config.game_boy.force_cgb_mode = true;
        }
        _ => {}
    }

    let emulator = NativeGameBoyEmulator::create(sdl, config.gb_config(primary_path.clone()))?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_gba(
    RunEmulatorArgs { sdl, config, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let emulator = NativeGbaEmulator::create(sdl, config.gba_config(primary_path))?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_pce(
    RunEmulatorArgs { sdl, config, primary_path, load_state_slot, .. }: RunEmulatorArgs<'_>,
) -> anyhow::Result<()> {
    let emulator = NativePcEngineEmulator::create(sdl, config.pce_config(primary_path))?;
    run_emulator_no_gui(emulator, load_state_slot)
}

fn run_emulator_no_gui<Emulator: EmulatorTrait>(
    mut emulator: NativeEmulator<Emulator>,
    load_state_slot: Option<usize>,
) -> anyhow::Result<()> {
    if let Some(slot) = load_state_slot {
        emulator.load_state(slot)?;
    }

    loop {
        match emulator.run(&mut |_event| {})? {
            Some(NativeTickEffect::PowerOff | NativeTickEffect::Exit) => return Ok(()),
            None => {}
        }

        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_icon_does_not_panic() {
        let _ = load_icon();
    }
}
