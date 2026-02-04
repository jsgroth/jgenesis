#![cfg(target_arch = "wasm32")]

mod audio;
mod config;
mod js;
mod save;

use crate::audio::AudioQueue;
use crate::config::{EmulatorChannel, EmulatorCommand, InputConfig, WebConfig, WebConfigRef};
use bincode::{Decode, Encode};
use gba_config::GbaInputs;
use gba_core::api::GameBoyAdvanceEmulator;
use genesis_core::{GenesisEmulator, GenesisInputs};
use jgenesis_common::audio::DynamicResamplingRate;
use jgenesis_common::frontend::{
    AudioOutput, Color, ConstantInputPoller, EmulatorTrait, FrameSize, RenderFrameOptions,
    Renderer, SaveWriter, TickEffect,
};
use jgenesis_renderer::renderer::{WgpuRenderer, WindowSize};
use js_sys::Uint8Array;
use rfd::AsyncFileDialog;
use s32x_core::api::Sega32XEmulator;
use segacd_core::api::SegaCdEmulator;
use smsgg_config::SmsGgInputs;
use smsgg_core::{SmsGgEmulator, SmsGgHardware};
use snes_core::api::{CoprocessorRoms, SnesEmulator};
use snes_core::input::SnesInputs;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::mem;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, AudioContextOptions, Performance};
use web_time::Instant;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::web::WindowExtWebSys;
use winit::window::{Fullscreen, Window, WindowAttributes};

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

        self.audio_queue
            .push_if_space((sample_l as f32, sample_r as f32))
            .map_err(|err| format!("{err:?}"))?;
        Ok(())
    }
}

// 1MB should be big enough for any save file
const SERIALIZATION_BUFFER_LEN: usize = 1024 * 1024;

macro_rules! bincode_config {
    () => {
        bincode::config::standard()
            .with_little_endian()
            .with_fixed_int_encoding()
            .with_limit::<{ 10 * 1024 * 1024 }>()
    };
}

struct IndexedDbSaveWriter {
    file_name: Rc<str>,
    extension_to_bytes: HashMap<String, Vec<u8>>,
    serialization_buffer: Box<[u8]>,
}

impl IndexedDbSaveWriter {
    fn new() -> Self {
        let serialization_buffer = vec![0; SERIALIZATION_BUFFER_LEN].into_boxed_slice();
        Self {
            file_name: String::new().into(),
            extension_to_bytes: HashMap::new(),
            serialization_buffer,
        }
    }

    fn update_file_name(
        &mut self,
        file_name: String,
        extension_to_bytes: HashMap<String, Vec<u8>>,
    ) {
        self.file_name = file_name.into();
        self.extension_to_bytes = extension_to_bytes;
    }
}

impl SaveWriter for IndexedDbSaveWriter {
    type Err = String;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        match self.extension_to_bytes.get(extension) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(format!("No save file found for extension {extension}")),
        }
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        self.extension_to_bytes.insert(extension.into(), bytes.to_vec());

        let file_name = Rc::clone(&self.file_name);
        let extension = extension.to_string();
        let bytes = bytes.to_vec();
        wasm_bindgen_futures::spawn_local(async move {
            save::write(&file_name, &extension, &bytes).await;
        });

        Ok(())
    }

    fn load_serialized<D: Decode<()>>(&mut self, extension: &str) -> Result<D, Self::Err> {
        match self.extension_to_bytes.get(extension) {
            Some(bytes) => {
                let (value, _) =
                    bincode::decode_from_slice(bytes, bincode_config!()).map_err(|err| {
                        format!("Error deserializing value for {}: {err}", self.file_name)
                    })?;
                Ok(value)
            }
            None => Err(format!("No save file found for extension {extension}")),
        }
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        let bytes_len =
            bincode::encode_into_slice(data, &mut self.serialization_buffer, bincode_config!())
                .map_err(|err| format!("Error serializing value: {err}"))?;

        let bytes = self.serialization_buffer[..bytes_len].to_vec();
        self.extension_to_bytes.insert(extension.into(), bytes.clone());

        let file_name = Rc::clone(&self.file_name);
        let extension = extension.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            save::write(&file_name, &extension, &bytes).await;
        });

        Ok(())
    }
}

/// # Panics
///
/// This function will panic if it cannot initialize the console logger.
#[wasm_bindgen(start)]
pub fn init_logger() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(log::Level::Info).expect("Unable to initialize logger");
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
        renderer.render_frame(&self.buffer, STATIC_FRAME_SIZE, 60.0, RenderFrameOptions::default())
    }
}

struct QueuedFrame {
    buffer: Vec<Color>,
    size: FrameSize,
    target_fps: f64,
    options: RenderFrameOptions,
    queued: bool,
}

impl QueuedFrame {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(320 * 224),
            size: FrameSize { width: 320, height: 224 },
            target_fps: 60.0,
            options: RenderFrameOptions::default(),
            queued: false,
        }
    }
}

impl Renderer for QueuedFrame {
    type Err = String;

    fn render_frame(
        &mut self,
        frame_buffer: &[Color],
        frame_size: FrameSize,
        target_fps: f64,
        options: RenderFrameOptions,
    ) -> Result<(), Self::Err> {
        self.buffer.clear();
        self.buffer.extend(&frame_buffer[..(frame_size.width * frame_size.height) as usize]);

        self.size = frame_size;
        self.target_fps = target_fps;
        self.options = options;
        self.queued = true;

        Ok(())
    }
}

#[allow(clippy::large_enum_variant)]
enum Emulator {
    None(RandomNoiseGenerator),
    SmsGg(SmsGgEmulator, SmsGgInputs),
    Genesis(GenesisEmulator, GenesisInputs),
    SegaCd(SegaCdEmulator, GenesisInputs),
    Sega32X(Sega32XEmulator, GenesisInputs),
    Snes(SnesEmulator, SnesInputs),
    Gba(GameBoyAdvanceEmulator, GbaInputs),
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
            Self::SmsGg(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Genesis(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::SegaCd(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Sega32X(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Snes(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
            Self::Gba(emulator, inputs) => {
                while emulator
                    .tick(renderer, audio_output, &mut ConstantInputPoller(inputs), save_writer)
                    .expect("Emulator error")
                    != TickEffect::FrameRendered
                {}
            }
        }
    }

    fn reset(&mut self, save_writer: &mut IndexedDbSaveWriter) {
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
            Self::Sega32X(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
            Self::Snes(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
            Self::Gba(emulator, ..) => {
                emulator.hard_reset(save_writer);
            }
        }
    }

    fn target_fps(&self) -> f64 {
        match self {
            Self::None(..) => 30.0,
            Self::SmsGg(emulator, ..) => emulator.target_fps(),
            Self::Genesis(emulator, ..) => emulator.target_fps(),
            Self::SegaCd(emulator, ..) => emulator.target_fps(),
            Self::Sega32X(emulator, ..) => emulator.target_fps(),
            Self::Snes(emulator, ..) => emulator.target_fps(),
            Self::Gba(emulator, ..) => emulator.target_fps(),
        }
    }

    fn handle_window_event(&mut self, input_config: &InputConfig, event: &WindowEvent) {
        let WindowEvent::KeyboardInput {
            event: KeyEvent { physical_key: PhysicalKey::Code(keycode), state, .. },
            ..
        } = event
        else {
            return;
        };
        let pressed = *state == ElementState::Pressed;

        match self {
            Self::None(..) => {}
            Self::SmsGg(_, inputs) => {
                input_config.smsgg.handle_input(*keycode, pressed, inputs);
            }
            Self::Genesis(_, inputs) | Self::SegaCd(_, inputs) | Self::Sega32X(_, inputs) => {
                input_config.genesis.handle_input(*keycode, pressed, inputs);
            }
            Self::Snes(_, inputs) => {
                input_config.snes.handle_input(*keycode, pressed, inputs);
            }
            Self::Gba(_, inputs) => {
                input_config.gba.handle_input(*keycode, pressed, inputs);
            }
        }
    }

    fn reload_config(&mut self, config: &WebConfig) {
        match self {
            Self::None(..) => {}
            Self::SmsGg(emulator, ..) => {
                emulator.reload_config(&config.smsgg.to_emulator_config());
            }
            Self::Genesis(emulator, ..) => {
                emulator.reload_config(&config.genesis.to_emulator_config());
            }
            Self::SegaCd(emulator, ..) => {
                emulator.reload_config(&config.to_sega_cd_config());
            }
            Self::Sega32X(emulator, ..) => {
                emulator.reload_config(&config.to_32x_config());
            }
            Self::Snes(emulator, ..) => {
                emulator.reload_config(&config.snes.to_emulator_config());
            }
            Self::Gba(emulator, ..) => {
                emulator.reload_config(&config.gba.to_emulator_config());
            }
        }
    }

    fn rom_title(&mut self, current_file_name: &str) -> String {
        match self {
            Self::None(..) => "(No ROM loaded)".into(),
            Self::SmsGg(..) | Self::Gba(..) => current_file_name.into(),
            Self::Genesis(emulator, ..) => emulator.cartridge_title(),
            Self::SegaCd(emulator, ..) => emulator.disc_title().into(),
            Self::Sega32X(emulator, ..) => emulator.cartridge_title(),
            Self::Snes(emulator, ..) => emulator.cartridge_title(),
        }
    }

    fn has_persistent_save(&self) -> bool {
        match self {
            Self::None(..) => false,
            Self::SmsGg(emulator, ..) => emulator.has_sram(),
            Self::Genesis(emulator, ..) => emulator.has_sram(),
            Self::SegaCd(..) => true,
            Self::Sega32X(emulator, ..) => emulator.has_sram(),
            Self::Snes(emulator, ..) => emulator.has_sram(),
            Self::Gba(emulator, ..) => emulator.has_save_memory(),
        }
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        match self {
            Self::None(..) => {}
            Self::SmsGg(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
            Self::Genesis(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
            Self::SegaCd(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
            Self::Sega32X(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
            Self::Snes(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
            Self::Gba(emulator, ..) => emulator.update_audio_output_frequency(output_frequency),
        }
    }
}

#[derive(Debug, Clone)]
enum JgenesisUserEvent {
    FileOpen {
        rom: Vec<u8>,
        bios_rom: Option<Vec<u8>>,
        rom_file_name: String,
        save_files: HashMap<String, Vec<u8>>,
    },
    UploadSaveFile {
        contents: Vec<u8>,
    },
}

const CANVAS_WIDTH: u32 = 878;
const CANVAS_HEIGHT: u32 = 672;
const CANVAS_SIZE: WindowSize = WindowSize { width: CANVAS_WIDTH, height: CANVAS_HEIGHT };

/// # Panics
#[wasm_bindgen]
pub async fn run_emulator(config_ref: WebConfigRef, emulator_channel: EmulatorChannel) {
    let event_loop = EventLoop::<JgenesisUserEvent>::with_user_event()
        .build()
        .expect("Failed to create winit event loop");

    #[allow(deprecated)]
    let window =
        event_loop.create_window(WindowAttributes::default()).expect("Unable to create window");

    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            let dst = document.get_element_by_id("jgenesis-wasm")?;
            let canvas = web_sys::Element::from(window.canvas()?);
            dst.append_child(&canvas).ok()?;
            Some(())
        })
        .expect("Unable to append canvas to document");

    let renderer_config = config_ref.borrow().to_renderer_config();
    let mut renderer = WgpuRenderer::new(window, CANVAS_SIZE, renderer_config)
        .await
        .expect("Unable to create wgpu renderer");

    // Render a blank gray frame
    renderer
        .render_frame(
            &[Color::rgb(128, 128, 128)],
            FrameSize { width: 1, height: 1 },
            60.0,
            RenderFrameOptions::default(),
        )
        .expect("Unable to render blank frame");

    let audio_ctx_options = AudioContextOptions::new();
    audio_ctx_options.set_sample_rate(audio::SAMPLE_RATE as f32);

    let audio_ctx = AudioContext::new_with_context_options(&audio_ctx_options)
        .expect("Unable to create audio context");
    let audio_output = WebAudioOutput::new(audio_ctx);
    let _audio_worklet =
        audio::initialize_audio_worklet(&audio_output.audio_ctx, &audio_output.audio_queue)
            .await
            .expect("Unable to initialize audio worklet");

    let save_writer = IndexedDbSaveWriter::new();

    js::showUi();

    run_event_loop(
        event_loop,
        AppState::new(renderer, audio_output, save_writer, config_ref, emulator_channel),
    );
}

struct AppState {
    renderer: WgpuRenderer<Window>,
    audio_output: WebAudioOutput,
    save_writer: IndexedDbSaveWriter,
    config_ref: WebConfigRef,
    current_config: WebConfig,
    emulator_channel: EmulatorChannel,
    emulator: Emulator,
    dynamic_resampling_rate: DynamicResamplingRate,
    queued_frame: QueuedFrame,
    performance: Performance,
    next_frame_time_ms: f64,
    waiting_for_input: Option<String>,
}

impl AppState {
    fn new(
        renderer: WgpuRenderer<Window>,
        audio_output: WebAudioOutput,
        save_writer: IndexedDbSaveWriter,
        config_ref: WebConfigRef,
        emulator_channel: EmulatorChannel,
    ) -> Self {
        let current_config = config_ref.borrow().clone();
        let emulator = Emulator::None(RandomNoiseGenerator::new());
        let dynamic_resampling_rate =
            DynamicResamplingRate::new(audio::SAMPLE_RATE, audio::BUFFER_LEN_SAMPLES / 2);
        let queued_frame = QueuedFrame::new();
        let performance = web_sys::window()
            .and_then(|window| window.performance())
            .expect("Unable to get window.performance");
        let next_frame_time_ms = performance.now();

        Self {
            renderer,
            audio_output,
            save_writer,
            config_ref,
            current_config,
            emulator_channel,
            emulator,
            dynamic_resampling_rate,
            queued_frame,
            performance,
            next_frame_time_ms,
            waiting_for_input: None,
        }
    }
}

impl AppState {
    fn handle_file_open(
        &mut self,
        rom: Vec<u8>,
        bios_rom: Option<Vec<u8>>,
        save_files: HashMap<String, Vec<u8>>,
        rom_file_name: String,
    ) {
        self.audio_output.suspend();
        self.dynamic_resampling_rate =
            DynamicResamplingRate::new(audio::SAMPLE_RATE, audio::BUFFER_LEN_SAMPLES / 2);

        let prev_file_name = Rc::clone(&self.save_writer.file_name);
        let prev_save_files = mem::take(&mut self.save_writer.extension_to_bytes);
        self.save_writer.update_file_name(rom_file_name.clone(), save_files);
        self.emulator = match open_emulator(
            rom,
            bios_rom,
            &rom_file_name,
            &self.config_ref,
            &mut self.save_writer,
        ) {
            Ok(emulator) => emulator,
            Err(err) => {
                js::alert(&format!("Error opening ROM file: {err}"));
                self.save_writer.update_file_name(prev_file_name.to_string(), prev_save_files);
                return;
            }
        };

        self.emulator_channel.set_current_file_name(rom_file_name.clone());

        self.renderer.reload_config(self.config_ref.borrow().to_renderer_config());

        js::setRomTitle(&self.emulator.rom_title(&rom_file_name));
        js::setSaveUiEnabled(self.emulator.has_persistent_save());

        js::focusCanvas();
    }

    fn handle_upload_save_file(&mut self, contents: Vec<u8>) {
        if matches!(self.emulator, Emulator::None(..)) {
            return;
        }

        self.audio_output.suspend();

        self.save_writer.extension_to_bytes.insert("sav".into(), contents.clone());

        // Immediately persist save file because it won't get written again until the game writes to SRAM
        let file_name = self.emulator_channel.current_file_name();
        wasm_bindgen_futures::spawn_local(async move {
            save::write(&file_name, "sav", &contents).await;
        });

        self.emulator.reset(&mut self.save_writer);

        js::focusCanvas();
    }

    fn handle_about_to_wait(
        &mut self,
        event_loop_proxy: &EventLoopProxy<JgenesisUserEvent>,
        elwt: &ActiveEventLoop,
    ) {
        if self.waiting_for_input.is_some() {
            elwt.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(1),
            ));
            return;
        }

        if !self.queued_frame.queued {
            // No frame queued; run emulator until it renders the next frame
            self.emulator.render_frame(
                &mut self.queued_frame,
                &mut self.audio_output,
                &mut self.save_writer,
            );
        }

        let now = self.performance.now();
        if now < self.next_frame_time_ms {
            let wait_until_duration =
                Duration::from_micros((1000.0 * (self.next_frame_time_ms - now)).ceil() as u64);
            elwt.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait_until_duration));
            return;
        }

        let fps = self.emulator.target_fps();
        while now >= self.next_frame_time_ms {
            self.next_frame_time_ms += 1000.0 / fps;
        }

        self.renderer
            .render_frame(
                &self.queued_frame.buffer,
                self.queued_frame.size,
                self.queued_frame.target_fps,
                self.queued_frame.options,
            )
            .expect("Frame render error");
        self.queued_frame.queued = false;

        // GBA may not detect persistent memory until the emulator has been running for a bit, so
        // call this after every frame
        js::setSaveUiEnabled(self.emulator.has_persistent_save());

        self.dynamic_resampling_rate.adjust(self.audio_output.audio_queue.len().unwrap());
        self.emulator.update_audio_output_frequency(
            self.dynamic_resampling_rate.current_output_frequency().into(),
        );

        let config = self.config_ref.borrow().clone();
        if config != self.current_config {
            config.save_to_local_storage();

            self.renderer.reload_config(config.to_renderer_config());
            self.emulator.reload_config(&config);
            self.current_config = config;
        }

        self.drain_emulator_channel(event_loop_proxy);
    }

    fn drain_emulator_channel(&mut self, event_loop_proxy: &EventLoopProxy<JgenesisUserEvent>) {
        while let Some(command) = self.emulator_channel.pop_command() {
            match command {
                EmulatorCommand::OpenFile => {
                    wasm_bindgen_futures::spawn_local(open_file(event_loop_proxy.clone()));
                }
                EmulatorCommand::OpenSegaCdBios => {
                    wasm_bindgen_futures::spawn_local(open_bios(SCD_BIOS_KEY, &["bin"]));
                }
                EmulatorCommand::OpenGbaBios => {
                    wasm_bindgen_futures::spawn_local(open_bios(GBA_BIOS_KEY, &["bin"]));
                }
                EmulatorCommand::UploadSaveFile => {
                    wasm_bindgen_futures::spawn_local(upload_save_file(event_loop_proxy.clone()));
                }
                EmulatorCommand::Reset => {
                    self.audio_output.suspend();

                    self.emulator.reset(&mut self.save_writer);

                    js::focusCanvas();
                }
                EmulatorCommand::ConfigureInput { name } => {
                    self.waiting_for_input = Some(name);

                    js::beforeInputConfigure();
                    js::focusCanvas();
                }
            }
        }
    }

    fn handle_window_event(&mut self, window_event: WindowEvent, elwt: &ActiveEventLoop) {
        match &self.waiting_for_input {
            Some(button) => {
                if let WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key: PhysicalKey::Code(keycode),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } = &window_event
                {
                    let input_config = &mut self.config_ref.borrow_mut().inputs;
                    match &self.emulator {
                        Emulator::None(..) => {}
                        Emulator::SmsGg(..) => input_config.smsgg.update_field(button, *keycode),
                        Emulator::Genesis(..) | Emulator::SegaCd(..) | Emulator::Sega32X(..) => {
                            input_config.genesis.update_field(button, *keycode);
                        }
                        Emulator::Snes(..) => input_config.snes.update_field(button, *keycode),
                        Emulator::Gba(..) => input_config.gba.update_field(button, *keycode),
                    }

                    js::afterInputConfigure(button, &format!("{keycode:?}"));
                    self.waiting_for_input = None;
                }
            }
            None => {
                self.emulator.handle_window_event(&self.config_ref.borrow().inputs, &window_event);
            }
        }

        match window_event {
            WindowEvent::CloseRequested => {
                elwt.exit();
            }
            WindowEvent::Resized(_) => {
                self.renderer.handle_resize(CANVAS_SIZE);

                // Show cursor only when not fullscreen
                js::setCursorVisible(self.renderer.window().fullscreen().is_none());

                js::setFullscreen(self.renderer.window().fullscreen().is_some());
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::F8),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // Toggle fullscreen
                let new_fullscreen = match self.renderer.window().fullscreen() {
                    None => Some(Fullscreen::Borderless(None)),
                    Some(_) => None,
                };
                // SAFETY: Not reassigning the window
                unsafe {
                    self.renderer.window_mut().set_fullscreen(new_fullscreen);
                }
            }
            _ => {}
        }
    }
}

fn run_event_loop(event_loop: EventLoop<JgenesisUserEvent>, mut state: AppState) {
    event_loop.set_control_flow(ControlFlow::Poll);

    let event_loop_proxy = event_loop.create_proxy();

    #[allow(deprecated)]
    event_loop
        .run(move |event, elwt| match event {
            Event::UserEvent(user_event) => match user_event {
                JgenesisUserEvent::FileOpen { rom, bios_rom, rom_file_name, save_files } => {
                    state.handle_file_open(rom, bios_rom, save_files, rom_file_name);
                }
                JgenesisUserEvent::UploadSaveFile { contents } => {
                    state.handle_upload_save_file(contents);
                }
            },
            Event::AboutToWait => {
                state.handle_about_to_wait(&event_loop_proxy, elwt);
            }
            Event::WindowEvent { event: window_event, window_id }
                if window_id == state.renderer.window().id() =>
            {
                state.handle_window_event(window_event, elwt);
            }
            _ => {}
        })
        .unwrap();
}

async fn open_file(event_loop_proxy: EventLoopProxy<JgenesisUserEvent>) {
    let file = AsyncFileDialog::new()
        .add_filter(
            "Supported Files",
            &["sms", "gg", "gen", "md", "bin", "smd", "chd", "32x", "sfc", "smc", "gba"],
        )
        .add_filter("All Types", &["*"])
        .pick_file()
        .await;
    let Some(file) = file else { return };

    let file_name = file.file_name();
    let extension =
        Path::new(&file_name).extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase());

    let bios_rom = match extension.as_deref() {
        Some("chd") => {
            let Some(bios_rom) = read_bios_from_idb(SCD_BIOS_KEY).await else {
                js::alert("Sega CD emulation requires a Sega CD BIOS ROM to be configured");
                return;
            };
            Some(bios_rom)
        }
        Some("gba") => {
            let Some(bios_rom) = read_bios_from_idb(GBA_BIOS_KEY).await else {
                js::alert("GBA emulation requires a GBA BIOS ROM to be configured");
                return;
            };
            Some(bios_rom)
        }
        _ => None,
    };

    let contents = file.read().await;

    let save_files = save::load_all(&file_name).await;

    event_loop_proxy
        .send_event(JgenesisUserEvent::FileOpen {
            rom: contents,
            bios_rom,
            rom_file_name: file_name,
            save_files,
        })
        .expect("Unable to send file opened event");
}

async fn open_bios(key: &str, supported_extensions: &[&str]) {
    let file = AsyncFileDialog::new()
        .add_filter("Supported Files", supported_extensions)
        .add_filter("All Types", &["*"])
        .pick_file()
        .await;
    let Some(file) = file else { return };

    let contents = file.read().await;
    write_bios_to_idb(key, &contents).await;
}

async fn upload_save_file(event_loop_proxy: EventLoopProxy<JgenesisUserEvent>) {
    let file = AsyncFileDialog::new().add_filter("sav", &["sav", "srm"]).pick_file().await;
    let Some(file) = file else { return };

    let contents = file.read().await;

    event_loop_proxy
        .send_event(JgenesisUserEvent::UploadSaveFile { contents })
        .expect("Unable to send upload save file event");
}

enum OpenEmulatorError {
    NoSegaCdBios,
    NoGbaBios,
    Other(Box<dyn Error>),
}

impl Display for OpenEmulatorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSegaCdBios => {
                write!(f, "No Sega CD BIOS is configured; required for Sega CD emulation")
            }
            Self::NoGbaBios => write!(f, "No GBA BIOS is configured; required for GBA emulation"),
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

const SCD_BIOS_KEY: &str = "segacd-bios-rom";
const GBA_BIOS_KEY: &str = "gba-bios-rom";

async fn read_bios_from_idb(key: &str) -> Option<Vec<u8>> {
    try_read_bios_from_idb(key).await.unwrap_or_else(|err| {
        log::error!(
            "Error reading BIOS ROM from IndexedDB: {}",
            err.as_string().unwrap_or_default()
        );
        None
    })
}

async fn try_read_bios_from_idb(key: &str) -> Result<Option<Vec<u8>>, JsValue> {
    let object = JsFuture::from(js::loadBios(key)).await?;
    if object.is_null() {
        return Ok(None);
    }

    let array = object.dyn_into::<Uint8Array>()?;
    Ok(Some(array.to_vec()))
}

async fn write_bios_to_idb(key: &str, bytes: &[u8]) {
    let array = Uint8Array::new_from_slice(bytes);
    if let Err(err) = JsFuture::from(js::writeBios(key, array)).await {
        log::error!("Error saving BIOS ROM to IndexedDB: {}", err.as_string().unwrap_or_default());
    }
}

#[allow(clippy::map_unwrap_or)]
fn open_emulator(
    rom: Vec<u8>,
    bios_rom: Option<Vec<u8>>,
    rom_file_name: &str,
    config_ref: &WebConfigRef,
    save_writer: &mut IndexedDbSaveWriter,
) -> Result<Emulator, OpenEmulatorError> {
    let file_ext = Path::new(rom_file_name).extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| {
        log::warn!("Unable to determine file extension of uploaded file; defaulting to Genesis emulator");
        "md".into()
    });

    match file_ext.as_str() {
        file_ext @ ("sms" | "gg") => {
            let inputs = config_ref.borrow().inputs.smsgg_inputs();
            js::showSmsGgConfig(inputs.0, inputs.1);

            let hardware = match file_ext {
                "sms" => SmsGgHardware::MasterSystem,
                "gg" => SmsGgHardware::GameGear,
                _ => unreachable!("nested match expressions"),
            };
            let emulator = SmsGgEmulator::create(
                Some(rom),
                None,
                hardware,
                config_ref.borrow().smsgg.to_emulator_config(),
                save_writer,
            );
            Ok(Emulator::SmsGg(emulator, SmsGgInputs::default()))
        }
        "gen" | "md" | "bin" | "smd" => {
            let inputs = config_ref.borrow().inputs.genesis_inputs();
            js::showGenesisConfig(inputs.0, inputs.1);

            let emulator = GenesisEmulator::create(
                rom,
                config_ref.borrow().genesis.to_emulator_config(),
                save_writer,
            );
            Ok(Emulator::Genesis(emulator, GenesisInputs::default()))
        }
        "chd" => {
            let Some(bios) = bios_rom else {
                return Err(OpenEmulatorError::NoSegaCdBios);
            };

            let emulator = SegaCdEmulator::create_in_memory(
                bios,
                rom,
                config_ref.borrow().to_sega_cd_config(),
                save_writer,
            )
            .map_err(|err| OpenEmulatorError::Other(err.into()))?;

            let inputs = config_ref.borrow().inputs.genesis_inputs();
            js::showGenesisConfig(inputs.0, inputs.1);

            Ok(Emulator::SegaCd(emulator, GenesisInputs::default()))
        }
        "32x" => {
            let emulator =
                Sega32XEmulator::create(rom, config_ref.borrow().to_32x_config(), save_writer);

            let inputs = config_ref.borrow().inputs.genesis_inputs();
            js::showGenesisConfig(inputs.0, inputs.1);

            Ok(Emulator::Sega32X(emulator, GenesisInputs::default()))
        }
        "sfc" | "smc" => {
            let emulator = SnesEmulator::create(
                rom,
                config_ref.borrow().snes.to_emulator_config(),
                CoprocessorRoms::none(),
                save_writer,
            )
            .map_err(|err| OpenEmulatorError::Other(err.into()))?;

            let inputs = config_ref.borrow().inputs.snes_inputs();
            js::showSnesConfig(inputs.0, inputs.1);

            Ok(Emulator::Snes(emulator, SnesInputs::default()))
        }
        "gba" => {
            let Some(bios) = bios_rom else {
                return Err(OpenEmulatorError::NoGbaBios);
            };

            let emulator = GameBoyAdvanceEmulator::create(
                rom,
                bios,
                config_ref.borrow().gba.to_emulator_config(),
                save_writer,
            )
            .map_err(|err| OpenEmulatorError::Other(err.into()))?;

            let inputs = config_ref.borrow().inputs.gba_inputs();
            js::showGbaConfig(inputs.0, inputs.1);

            Ok(Emulator::Gba(emulator, GbaInputs::default()))
        }
        _ => {
            Err(OpenEmulatorError::Other(format!("Unsupported file extension: {file_ext}").into()))
        }
    }
}

#[must_use]
#[wasm_bindgen]
pub fn build_commit_hash() -> Option<String> {
    option_env!("JGENESIS_COMMIT").map(String::from)
}
