#![cfg(target_arch = "wasm32")]

mod audio;
mod config;
mod js;

use crate::audio::AudioQueue;
use crate::config::{EmulatorChannel, EmulatorCommand, WebConfig, WebConfigRef};
use base64::engine::general_purpose;
use base64::Engine;
use bincode::{Decode, Encode};
use genesis_core::{GenesisEmulator, GenesisInputs};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, FrameSize, Renderer, SaveWriter, TickEffect, TimingMode,
};
use jgenesis_renderer::renderer::WgpuRenderer;
use rfd::AsyncFileDialog;
use segacd_core::api::{SegaCdEmulator, SegaCdEmulatorConfig};
use smsgg_core::{SmsGgEmulator, SmsGgInputs};
use snes_core::api::{CoprocessorRoms, SnesEmulator};
use snes_core::input::SnesInputs;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display};
use std::path::Path;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{AudioContext, AudioContextOptions};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use winit::platform::web::WindowExtWebSys;
use winit::window::{Fullscreen, Window, WindowBuilder};

struct WebAudioOutput {
    audio_ctx: AudioContext,
    audio_queue: AudioQueue,
    audio_started: bool,
}

impl WebAudioOutput {
    fn new(audio_ctx: AudioContext) -> Self {
        Self { audio_ctx, audio_queue: AudioQueue::new(), audio_started: false }
    }

    fn suspend(&mut self) {
        // Suspending the AudioContext while loading/resetting is necessary to avoid audio delay
        // in Chrome
        let _ = self.audio_ctx.suspend();
        self.audio_started = false;
    }
}

impl AudioOutput for WebAudioOutput {
    type Err = String;

    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        if !self.audio_started {
            self.audio_started = true;
            let _ = self.audio_ctx.resume();
        }

        self.audio_queue.push_if_space(sample_l as f32).map_err(|err| format!("{err:?}"))?;
        self.audio_queue.push_if_space(sample_r as f32).map_err(|err| format!("{err:?}"))?;
        Ok(())
    }
}

// 1MB should be big enough for any save file
const SERIALIZATION_BUFFER_LEN: usize = 1024 * 1024;

struct LocalStorageSaveWriter {
    file_name: Rc<str>,
    extension_to_file_name: HashMap<String, Rc<str>>,
    serialization_buffer: Box<[u8]>,
}

impl LocalStorageSaveWriter {
    fn new() -> Self {
        let serialization_buffer = vec![0; SERIALIZATION_BUFFER_LEN].into_boxed_slice();
        Self {
            file_name: String::new().into(),
            extension_to_file_name: HashMap::new(),
            serialization_buffer,
        }
    }

    fn update_file_name(&mut self, file_name: String) {
        self.file_name = file_name.into();
        self.extension_to_file_name.clear();
    }

    fn get_file_name(&mut self, extension: &str) -> Rc<str> {
        if extension == "sav" {
            return Rc::clone(&self.file_name);
        }

        match self.extension_to_file_name.get(extension) {
            Some(file_name) => Rc::clone(file_name),
            None => {
                let mut file_name = self.file_name.to_string();
                file_name.push('.');
                file_name.push_str(extension);

                let file_name: Rc<str> = file_name.into();
                self.extension_to_file_name.insert(extension.into(), Rc::clone(&file_name));
                file_name
            }
        }
    }
}

macro_rules! bincode_config {
    () => {
        bincode::config::standard()
            .with_little_endian()
            .with_fixed_int_encoding()
            .with_limit::<{ 10 * 1024 * 1024 }>()
    };
}

impl SaveWriter for LocalStorageSaveWriter {
    type Err = String;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        let file_name = self.get_file_name(extension);
        let bytes = read_save_file(&file_name)?;

        Ok(bytes)
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        let file_name = self.get_file_name(extension);
        let bytes_b64 = general_purpose::STANDARD.encode(bytes);
        js::localStorageSet(&file_name, &bytes_b64);

        Ok(())
    }

    fn load_serialized<D: Decode>(&mut self, extension: &str) -> Result<D, Self::Err> {
        let file_name = self.get_file_name(extension);
        let bytes = read_save_file(&file_name)?;
        let (value, _) = bincode::decode_from_slice(&bytes, bincode_config!())
            .map_err(|err| format!("Error serializing value into {file_name}: {err}"))?;

        Ok(value)
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        let bytes_len =
            bincode::encode_into_slice(data, &mut self.serialization_buffer, bincode_config!())
                .map_err(|err| format!("Error serializing value: {err}"))?;
        let bytes_b64 = general_purpose::STANDARD.encode(&self.serialization_buffer[..bytes_len]);

        let file_name = self.get_file_name(extension);
        js::localStorageSet(&file_name, &bytes_b64);

        Ok(())
    }
}

fn read_save_file(file_name: &str) -> Result<Vec<u8>, String> {
    js::localStorageGet(file_name)
        .and_then(|b64_bytes| general_purpose::STANDARD.decode(b64_bytes).ok())
        .ok_or_else(|| format!("No save file found for file name {file_name}"))
}

fn window_size_fn(window: &Window) -> (u32, u32) {
    let size = window.inner_size();
    (size.width, size.height)
}

/// # Panics
///
/// This function will panic if it cannot initialize the console logger.
#[wasm_bindgen(start)]
pub fn init_logger() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(log::Level::Info).expect("Unable to initialize logger");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmsGgConsole {
    MasterSystem,
    GameGear,
}

const STATIC_FRAME_SIZE: FrameSize = FrameSize { width: 878 / 4, height: 672 / 4 };
const STATIC_FRAME_LEN: usize = (STATIC_FRAME_SIZE.width * STATIC_FRAME_SIZE.height) as usize;

struct RandomNoiseGenerator {
    buffer: Vec<Color>,
}

impl RandomNoiseGenerator {
    fn new() -> Self {
        Self { buffer: vec![Color::default(); STATIC_FRAME_LEN] }
    }

    fn randomize(&mut self) {
        for color in &mut self.buffer {
            *color = Color::rgb(rand::random(), rand::random(), rand::random());
        }
    }

    fn render<R: Renderer>(&self, renderer: &mut R) -> Result<(), R::Err> {
        renderer.render_frame(&self.buffer, STATIC_FRAME_SIZE, None)
    }
}

#[allow(clippy::large_enum_variant)]
enum Emulator {
    None(RandomNoiseGenerator),
    SmsGg(SmsGgEmulator, SmsGgInputs, SmsGgConsole),
    Genesis(GenesisEmulator, GenesisInputs),
    SegaCd(SegaCdEmulator, GenesisInputs),
    Snes(SnesEmulator, SnesInputs),
}

impl Emulator {
    fn render_frame<R: Renderer, A: AudioOutput, S: SaveWriter>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        save_writer: &mut S,
    ) where
        R::Err: Debug + Display + Send + Sync + 'static,
        A::Err: Debug + Display + Send + Sync + 'static,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        match self {
            Self::None(noise_generator) => {
                noise_generator.randomize();
                noise_generator.render(renderer).expect("Failed to render random noise");
            }
            Self::SmsGg(emulator, inputs, _) => {
                while emulator
                    .tick(renderer, audio_output, inputs, save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Genesis(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, inputs, save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::SegaCd(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, inputs, save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Snes(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, inputs, save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
        }
    }

    fn reset(&mut self, save_writer: &mut LocalStorageSaveWriter) {
        match self {
            Self::None(..) => {}
            Self::SmsGg(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
            Self::Genesis(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
            Self::SegaCd(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
            Self::Snes(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
        }
    }

    fn target_fps(&self) -> f64 {
        // ~59.9 FPS
        let sega_ntsc_fps = 53_693_175.0 / 3420.0 / 262.0;
        // ~49.7 FPS
        let sega_pal_fps = 53_203_424.0 / 3420.0 / 313.0;

        match self {
            Self::None(..) => 30.0,
            Self::SmsGg(emulator, ..) => match emulator.timing_mode() {
                TimingMode::Ntsc => sega_ntsc_fps,
                TimingMode::Pal => sega_pal_fps,
            },
            Self::Genesis(emulator, ..) => match emulator.timing_mode() {
                TimingMode::Ntsc => sega_ntsc_fps,
                TimingMode::Pal => sega_pal_fps,
            },
            Self::SegaCd(emulator, ..) => match emulator.timing_mode() {
                TimingMode::Ntsc => sega_ntsc_fps,
                TimingMode::Pal => sega_pal_fps,
            },
            Self::Snes(emulator, ..) => match emulator.timing_mode() {
                TimingMode::Ntsc => 60.0,
                TimingMode::Pal => 50.0,
            },
        }
    }

    fn handle_window_event(&mut self, event: &WindowEvent<'_>) {
        match self {
            Self::None(..) => {}
            Self::SmsGg(_, inputs, _) => {
                handle_smsgg_input(inputs, event);
            }
            Self::Genesis(_, inputs) | Self::SegaCd(_, inputs) => {
                handle_genesis_input(inputs, event);
            }
            Self::Snes(_, inputs) => {
                handle_snes_input(inputs, event);
            }
        }
    }

    fn reload_config(&mut self, config: &WebConfig) {
        match self {
            Self::None(..) => {}
            Self::SmsGg(emulator, _, console) => {
                emulator.reload_config(&config.smsgg.to_emulator_config(*console));
            }
            Self::Genesis(emulator, ..) => {
                emulator.reload_config(&config.genesis.to_emulator_config());
            }
            Self::SegaCd(emulator, ..) => {
                emulator.reload_config(&SegaCdEmulatorConfig {
                    genesis: config.genesis.to_emulator_config(),
                    enable_ram_cartridge: true,
                });
            }
            Self::Snes(emulator, ..) => {
                emulator.reload_config(&config.snes.to_emulator_config());
            }
        }
    }

    fn rom_title(&mut self, current_file_name: &str) -> String {
        match self {
            Self::None(..) => "(No ROM loaded)".into(),
            Self::SmsGg(..) => current_file_name.into(),
            Self::Genesis(emulator, ..) => emulator.cartridge_title(),
            Self::SegaCd(emulator, ..) => emulator.disc_title().into(),
            Self::Snes(emulator, ..) => emulator.cartridge_title(),
        }
    }

    fn has_persistent_save(&self) -> bool {
        match self {
            Self::None(..) => false,
            Self::SmsGg(emulator, ..) => emulator.has_sram(),
            Self::Genesis(emulator, ..) => emulator.has_sram(),
            Self::SegaCd(..) => true,
            Self::Snes(emulator, ..) => emulator.has_sram(),
        }
    }
}

fn handle_smsgg_input(inputs: &mut SmsGgInputs, event: &WindowEvent<'_>) {
    let WindowEvent::KeyboardInput {
        input: KeyboardInput { virtual_keycode: Some(keycode), state, .. },
        ..
    } = event
    else {
        return;
    };
    let pressed = *state == ElementState::Pressed;

    match keycode {
        VirtualKeyCode::Up => inputs.p1.up = pressed,
        VirtualKeyCode::Left => inputs.p1.left = pressed,
        VirtualKeyCode::Right => inputs.p1.right = pressed,
        VirtualKeyCode::Down => inputs.p1.down = pressed,
        VirtualKeyCode::A => inputs.p1.button2 = pressed,
        VirtualKeyCode::S => inputs.p1.button1 = pressed,
        VirtualKeyCode::Return => inputs.pause = pressed,
        _ => {}
    }
}

fn handle_genesis_input(inputs: &mut GenesisInputs, event: &WindowEvent<'_>) {
    let WindowEvent::KeyboardInput {
        input: KeyboardInput { virtual_keycode: Some(keycode), state, .. },
        ..
    } = event
    else {
        return;
    };
    let pressed = *state == ElementState::Pressed;

    match keycode {
        VirtualKeyCode::Up => inputs.p1.up = pressed,
        VirtualKeyCode::Left => inputs.p1.left = pressed,
        VirtualKeyCode::Right => inputs.p1.right = pressed,
        VirtualKeyCode::Down => inputs.p1.down = pressed,
        VirtualKeyCode::A => inputs.p1.a = pressed,
        VirtualKeyCode::S => inputs.p1.b = pressed,
        VirtualKeyCode::D => inputs.p1.c = pressed,
        VirtualKeyCode::Q => inputs.p1.x = pressed,
        VirtualKeyCode::W => inputs.p1.y = pressed,
        VirtualKeyCode::E => inputs.p1.z = pressed,
        VirtualKeyCode::Return => inputs.p1.start = pressed,
        VirtualKeyCode::RShift => inputs.p1.mode = pressed,
        _ => {}
    }
}

fn handle_snes_input(inputs: &mut SnesInputs, event: &WindowEvent<'_>) {
    let WindowEvent::KeyboardInput {
        input: KeyboardInput { virtual_keycode: Some(keycode), state, .. },
        ..
    } = event
    else {
        return;
    };
    let pressed = *state == ElementState::Pressed;

    match keycode {
        VirtualKeyCode::Up => inputs.p1.up = pressed,
        VirtualKeyCode::Left => inputs.p1.left = pressed,
        VirtualKeyCode::Right => inputs.p1.right = pressed,
        VirtualKeyCode::Down => inputs.p1.down = pressed,
        VirtualKeyCode::S => inputs.p1.a = pressed,
        VirtualKeyCode::X => inputs.p1.b = pressed,
        VirtualKeyCode::A => inputs.p1.x = pressed,
        VirtualKeyCode::Z => inputs.p1.y = pressed,
        VirtualKeyCode::D => inputs.p1.l = pressed,
        VirtualKeyCode::C => inputs.p1.r = pressed,
        VirtualKeyCode::Return => inputs.p1.start = pressed,
        VirtualKeyCode::RShift => inputs.p1.select = pressed,
        _ => {}
    }
}

#[derive(Debug, Clone)]
enum JgenesisUserEvent {
    FileOpen { rom: Vec<u8>, bios: Option<Vec<u8>>, rom_file_name: String },
    UploadSaveFile { contents_base64: String },
}

/// # Panics
#[wasm_bindgen]
pub async fn run_emulator(config_ref: WebConfigRef, emulator_channel: EmulatorChannel) {
    let event_loop = EventLoopBuilder::<JgenesisUserEvent>::with_user_event().build();
    let window = WindowBuilder::new().build(&event_loop).expect("Unable to create window");

    window.set_inner_size(LogicalSize::new(878, 672));

    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            let dst = document.get_element_by_id("jgenesis-wasm")?;
            let canvas = web_sys::Element::from(window.canvas());
            dst.append_child(&canvas).ok()?;
            Some(())
        })
        .expect("Unable to append canvas to document");

    let renderer_config = config_ref.borrow().common.to_renderer_config();
    let mut renderer = WgpuRenderer::new(window, window_size_fn, renderer_config)
        .await
        .expect("Unable to create wgpu renderer");

    // Render a blank gray frame
    renderer
        .render_frame(&[Color::rgb(128, 128, 128)], FrameSize { width: 1, height: 1 }, None)
        .expect("Unable to render blank frame");

    let audio_ctx =
        AudioContext::new_with_context_options(AudioContextOptions::new().sample_rate(48000.0))
            .expect("Unable to create audio context");
    let audio_output = WebAudioOutput::new(audio_ctx);
    let _audio_worklet =
        audio::initialize_audio_worklet(&audio_output.audio_ctx, &audio_output.audio_queue)
            .await
            .expect("Unable to initialize audio worklet");

    let save_writer = LocalStorageSaveWriter::new();

    js::showUi();

    run_event_loop(event_loop, renderer, audio_output, save_writer, config_ref, emulator_channel);
}

fn run_event_loop(
    event_loop: EventLoop<JgenesisUserEvent>,
    mut renderer: WgpuRenderer<Window>,
    mut audio_output: WebAudioOutput,
    mut save_writer: LocalStorageSaveWriter,
    config_ref: WebConfigRef,
    emulator_channel: EmulatorChannel,
) {
    let performance = web_sys::window()
        .and_then(|window| window.performance())
        .expect("Unable to get window.performance");
    let mut next_frame_time = performance.now();

    let mut emulator = Emulator::None(RandomNoiseGenerator::new());
    let mut current_config = config_ref.borrow().clone();

    let event_loop_proxy = event_loop.create_proxy();
    event_loop.run(move |event, _, control_flow| match event {
        Event::UserEvent(user_event) => match user_event {
            JgenesisUserEvent::FileOpen { rom, bios, rom_file_name } => {
                audio_output.suspend();

                let prev_file_name = Rc::clone(&save_writer.file_name);
                save_writer.update_file_name(rom_file_name.clone());
                emulator =
                    match open_emulator(rom, bios, &rom_file_name, &config_ref, &mut save_writer) {
                        Ok(emulator) => emulator,
                        Err(err) => {
                            js::alert(&format!("Error opening ROM file: {err}"));
                            save_writer.update_file_name(prev_file_name.to_string());
                            return;
                        }
                    };

                emulator_channel.set_current_file_name(rom_file_name.clone());

                js::setRomTitle(&emulator.rom_title(&rom_file_name));
                js::setSaveUiEnabled(emulator.has_persistent_save());

                js::focusCanvas();
            }
            JgenesisUserEvent::UploadSaveFile { contents_base64 } => {
                if matches!(emulator, Emulator::None(..)) {
                    return;
                }

                audio_output.suspend();

                // Immediately persist save file because it won't get written again until the game writes to SRAM
                let file_name = emulator_channel.current_file_name();
                js::localStorageSet(&file_name, &contents_base64);

                emulator.reset(&mut save_writer);

                js::focusCanvas();
            }
        },
        Event::MainEventsCleared => {
            let now = performance.now();
            if now < next_frame_time {
                *control_flow = ControlFlow::Poll;
                return;
            }

            let fps = emulator.target_fps();
            while now >= next_frame_time {
                next_frame_time += 1000.0 / fps;
            }

            emulator.render_frame(&mut renderer, &mut audio_output, &mut save_writer);

            let config = config_ref.borrow().clone();
            if config != current_config {
                renderer.reload_config(config.common.to_renderer_config());
                emulator.reload_config(&config);
                current_config = config;
            }

            while let Some(command) = emulator_channel.pop_command() {
                match command {
                    EmulatorCommand::OpenFile => {
                        wasm_bindgen_futures::spawn_local(open_file(event_loop_proxy.clone()));
                    }
                    EmulatorCommand::OpenSegaCd => {
                        wasm_bindgen_futures::spawn_local(open_sega_cd(event_loop_proxy.clone()));
                    }
                    EmulatorCommand::UploadSaveFile => {
                        wasm_bindgen_futures::spawn_local(upload_save_file(
                            event_loop_proxy.clone(),
                        ));
                    }
                    EmulatorCommand::Reset => {
                        audio_output.suspend();

                        emulator.reset(&mut save_writer);

                        js::focusCanvas();
                    }
                }
            }
        }
        Event::WindowEvent { event: window_event, window_id }
            if window_id == renderer.window().id() =>
        {
            emulator.handle_window_event(&window_event);

            match window_event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::Resized(_) => {
                    renderer.handle_resize();

                    // Show cursor only when not fullscreen
                    js::setCursorVisible(renderer.window().fullscreen().is_none());
                }
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            virtual_keycode: Some(VirtualKeyCode::F8),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    // Toggle fullscreen
                    let new_fullscreen = match renderer.window().fullscreen() {
                        None => Some(Fullscreen::Borderless(None)),
                        Some(_) => None,
                    };
                    // SAFETY: Not reassigning the window
                    unsafe {
                        renderer.window_mut().set_fullscreen(new_fullscreen);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    });
}

async fn open_file(event_loop_proxy: EventLoopProxy<JgenesisUserEvent>) {
    let file = AsyncFileDialog::new()
        .add_filter("sms/gg/md", &["sms", "gg", "md", "bin", "sfc", "smc"])
        .pick_file()
        .await;
    let Some(file) = file else { return };

    let contents = file.read().await;
    let file_name = file.file_name();

    event_loop_proxy
        .send_event(JgenesisUserEvent::FileOpen {
            rom: contents,
            bios: None,
            rom_file_name: file_name,
        })
        .expect("Unable to send file opened event");
}

async fn open_sega_cd(event_loop_proxy: EventLoopProxy<JgenesisUserEvent>) {
    let bios_file = AsyncFileDialog::new()
        .set_title("Sega CD BIOS")
        .add_filter("bin", &["bin"])
        .pick_file()
        .await;

    let Some(bios_file) = bios_file else { return };
    let bios_contents = bios_file.read().await;

    let chd_file = AsyncFileDialog::new()
        .set_title("CD-ROM image (only CHD supported)")
        .add_filter("chd", &["chd"])
        .pick_file()
        .await;

    let Some(chd_file) = chd_file else { return };
    let chd_contents = chd_file.read().await;
    let chd_file_name = chd_file.file_name();

    event_loop_proxy
        .send_event(JgenesisUserEvent::FileOpen {
            rom: chd_contents,
            bios: Some(bios_contents),
            rom_file_name: chd_file_name,
        })
        .expect("Unable to send Sega CD BIOS/CHD opened event");
}

async fn upload_save_file(event_loop_proxy: EventLoopProxy<JgenesisUserEvent>) {
    let file = AsyncFileDialog::new().add_filter("sav", &["sav", "srm"]).pick_file().await;
    let Some(file) = file else { return };

    let contents = file.read().await;
    let contents_base64 = general_purpose::STANDARD.encode(contents);

    event_loop_proxy
        .send_event(JgenesisUserEvent::UploadSaveFile { contents_base64 })
        .expect("Unable to send upload save file event");
}

#[allow(clippy::map_unwrap_or)]
fn open_emulator(
    rom: Vec<u8>,
    bios: Option<Vec<u8>>,
    rom_file_name: &str,
    config_ref: &WebConfigRef,
    save_writer: &mut LocalStorageSaveWriter,
) -> Result<Emulator, Box<dyn Error>> {
    let file_ext = Path::new(rom_file_name).extension().map(|ext| ext.to_string_lossy().to_string()).unwrap_or_else(|| {
        log::warn!("Unable to determine file extension of uploaded file; defaulting to Genesis emulator");
        "md".into()
    });

    match file_ext.as_str() {
        file_ext @ ("sms" | "gg") => {
            js::showSmsGgConfig();

            let console = match file_ext {
                "sms" => SmsGgConsole::MasterSystem,
                "gg" => SmsGgConsole::GameGear,
                _ => unreachable!("nested match expressions"),
            };
            let emulator = SmsGgEmulator::create(
                rom,
                config_ref.borrow().smsgg.to_emulator_config(console),
                save_writer,
            );
            Ok(Emulator::SmsGg(emulator, SmsGgInputs::default(), console))
        }
        "md" | "bin" => {
            js::showGenesisConfig();

            let emulator = GenesisEmulator::create(
                rom,
                config_ref.borrow().genesis.to_emulator_config(),
                save_writer,
            );
            Ok(Emulator::Genesis(emulator, GenesisInputs::default()))
        }
        "chd" => {
            let Some(bios) = bios else { return Err("No SEGA CD BIOS supplied".into()) };

            let emulator = SegaCdEmulator::create_in_memory(
                bios,
                rom,
                SegaCdEmulatorConfig {
                    genesis: config_ref.borrow().genesis.to_emulator_config(),
                    enable_ram_cartridge: true,
                },
                save_writer,
            )?;

            js::showGenesisConfig();

            Ok(Emulator::SegaCd(emulator, GenesisInputs::default()))
        }
        "sfc" | "smc" => {
            let emulator = SnesEmulator::create(
                rom,
                config_ref.borrow().snes.to_emulator_config(),
                CoprocessorRoms::none(),
                save_writer,
            )?;

            js::showSnesConfig();

            Ok(Emulator::Snes(emulator, SnesInputs::default()))
        }
        _ => Err(format!("Unsupported file extension: {file_ext}").into()),
    }
}

#[must_use]
#[wasm_bindgen]
pub fn build_commit_hash() -> Option<String> {
    option_env!("JGENESIS_COMMIT").map(String::from)
}

#[must_use]
#[wasm_bindgen]
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    general_purpose::STANDARD.decode(s).ok()
}
