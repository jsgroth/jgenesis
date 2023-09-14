#![cfg(target_arch = "wasm32")]

mod audio;
mod js;

use crate::audio::AudioQueue;
use genesis_core::{GenesisAspectRatio, GenesisEmulator, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_renderer::config::{
    FilterMode, PrescaleFactor, RendererConfig, VSyncMode, WgpuBackend,
};
use jgenesis_renderer::renderer::WgpuRenderer;
use jgenesis_traits::frontend::{
    AudioOutput, EmulatorTrait, PixelAspectRatio, Renderer, SaveWriter, TickEffect,
    TickableEmulator, TimingMode,
};
use js_sys::Promise;
use rfd::AsyncFileDialog;
use smsgg_core::psg::PsgVersion;
use smsgg_core::{SmsGgEmulator, SmsGgEmulatorConfig, SmsGgInputs, SmsRegion, VdpVersion};
use std::fmt::Debug;
use std::path::Path;
use wasm_bindgen::prelude::*;
use web_sys::{AudioContext, AudioContextOptions};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use winit::platform::web::WindowExtWebSys;
use winit::window::{Window, WindowBuilder};

struct WebAudioOutput {
    audio_queue: AudioQueue,
}

impl WebAudioOutput {
    fn new() -> Self {
        Self { audio_queue: AudioQueue::new() }
    }
}

impl AudioOutput for WebAudioOutput {
    type Err = JsValue;

    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.audio_queue.push_if_space(sample_l as f32)?;
        self.audio_queue.push_if_space(sample_r as f32)?;
        Ok(())
    }
}

struct Null;

impl SaveWriter for Null {
    type Err = ();

    fn persist_save(&mut self, _save_bytes: &[u8]) -> Result<(), Self::Err> {
        Ok(())
    }
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

#[allow(clippy::large_enum_variant)]
enum Emulator {
    SmsGg(SmsGgEmulator, SmsGgInputs),
    Genesis(GenesisEmulator, GenesisInputs),
}

impl Emulator {
    fn render_frame<R: Renderer, A: AudioOutput, S: SaveWriter>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        save_writer: &mut S,
    ) where
        R::Err: Debug,
        A::Err: Debug,
        S::Err: Debug,
    {
        match self {
            Self::SmsGg(emulator, inputs) => {
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
        }
    }

    fn timing_mode(&self) -> TimingMode {
        match self {
            Self::SmsGg(emulator, ..) => emulator.timing_mode(),
            Self::Genesis(emulator, ..) => emulator.timing_mode(),
        }
    }

    fn handle_window_event(&mut self, event: &WindowEvent<'_>) {
        match self {
            Self::SmsGg(_, inputs) => {
                handle_smsgg_input(inputs, event);
            }
            Self::Genesis(_, inputs) => {
                handle_genesis_input(inputs, event);
            }
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
        VirtualKeyCode::Up => {
            inputs.p1.up = pressed;
        }
        VirtualKeyCode::Left => {
            inputs.p1.left = pressed;
        }
        VirtualKeyCode::Right => {
            inputs.p1.right = pressed;
        }
        VirtualKeyCode::Down => {
            inputs.p1.down = pressed;
        }
        VirtualKeyCode::A => {
            inputs.p1.button_2 = pressed;
        }
        VirtualKeyCode::S => {
            inputs.p1.button_1 = pressed;
        }
        VirtualKeyCode::Return => {
            inputs.pause = pressed;
        }
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
        VirtualKeyCode::Up => {
            inputs.p1.up = pressed;
        }
        VirtualKeyCode::Left => {
            inputs.p1.left = pressed;
        }
        VirtualKeyCode::Right => {
            inputs.p1.right = pressed;
        }
        VirtualKeyCode::Down => {
            inputs.p1.down = pressed;
        }
        VirtualKeyCode::A => {
            inputs.p1.a = pressed;
        }
        VirtualKeyCode::S => {
            inputs.p1.b = pressed;
        }
        VirtualKeyCode::D => {
            inputs.p1.c = pressed;
        }
        VirtualKeyCode::Q => {
            inputs.p1.x = pressed;
        }
        VirtualKeyCode::W => {
            inputs.p1.y = pressed;
        }
        VirtualKeyCode::E => {
            inputs.p1.z = pressed;
        }
        VirtualKeyCode::Return => {
            inputs.p1.start = pressed;
        }
        VirtualKeyCode::RShift => {
            inputs.p1.mode = pressed;
        }
        _ => {}
    }
}

/// # Panics
#[wasm_bindgen]
pub async fn run_emulator() {
    let event_loop = EventLoopBuilder::<()>::default().build();
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

    let renderer_config = RendererConfig {
        wgpu_backend: WgpuBackend::OpenGl,
        vsync_mode: VSyncMode::Enabled,
        prescale_factor: PrescaleFactor::try_from(3).unwrap(),
        force_integer_height_scaling: false,
        filter_mode: FilterMode::Linear,
        use_webgl2_limits: true,
    };
    let renderer = WgpuRenderer::new(window, window_size_fn, renderer_config)
        .await
        .expect("Unable to create wgpu renderer");

    let audio_ctx =
        AudioContext::new_with_context_options(AudioContextOptions::new().sample_rate(48000.0))
            .expect("Unable to create audio context");
    let audio_output = WebAudioOutput::new();
    let _audio_worklet = audio::initialize_audio_worklet(&audio_ctx, &audio_output.audio_queue)
        .await
        .expect("Unable to initialize audio worklet");

    let file = AsyncFileDialog::new()
        .add_filter("sms/gg/md", &["sms", "gg", "md", "bin"])
        .pick_file()
        .await
        .expect("No file selected");
    let rom = file.read().await;

    #[allow(clippy::map_unwrap_or)]
    let file_ext = Path::new(&file.file_name()).extension().map(|ext| ext.to_string_lossy().to_string()).unwrap_or_else(|| {
        log::warn!("Unable to determine file extension of uploaded file; defaulting to Genesis emulator");
        "md".into()
    });

    let emulator = match file_ext.as_str() {
        "sms" | "gg" => {
            let (vdp_version, psg_version) = if file_ext == "sms" {
                (VdpVersion::NtscMasterSystem2, PsgVersion::MasterSystem2)
            } else {
                (VdpVersion::GameGear, PsgVersion::Standard)
            };
            let emulator = SmsGgEmulator::create(
                rom,
                None,
                vdp_version,
                SmsGgEmulatorConfig {
                    psg_version,
                    pixel_aspect_ratio: Some(PixelAspectRatio::try_from(8.0 / 7.0).unwrap()),
                    remove_sprite_limit: false,
                    sms_region: SmsRegion::International,
                    sms_crop_vertical_border: false,
                    sms_crop_left_border: false,
                    fm_sound_unit_enabled: true,
                    overclock_z80: false,
                },
            );
            Emulator::SmsGg(emulator, SmsGgInputs::default())
        }
        "md" | "bin" => {
            let emulator = GenesisEmulator::create(
                rom,
                None,
                GenesisEmulatorConfig {
                    forced_timing_mode: None,
                    forced_region: None,
                    aspect_ratio: GenesisAspectRatio::Ntsc,
                    adjust_aspect_ratio_in_2x_resolution: true,
                },
            );
            Emulator::Genesis(emulator, GenesisInputs::default())
        }
        _ => panic!("Unsupported extension: {file_ext}"),
    };

    js::focusCanvas();

    run_event_loop(event_loop, renderer, audio_output, audio_ctx, emulator);
}

fn run_event_loop(
    event_loop: EventLoop<()>,
    mut renderer: WgpuRenderer<Window>,
    mut audio_output: WebAudioOutput,
    audio_ctx: AudioContext,
    mut emulator: Emulator,
) {
    let mut audio_started = false;

    let performance = web_sys::window()
        .and_then(|window| window.performance())
        .expect("Unable to get window.performance");
    let mut next_frame_time = performance.now();

    event_loop.run(move |event, _, control_flow| match event {
        Event::MainEventsCleared => {
            let now = performance.now();
            if now < next_frame_time {
                *control_flow = ControlFlow::Poll;
                return;
            }

            let fps = match emulator.timing_mode() {
                TimingMode::Ntsc => 60.0,
                TimingMode::Pal => 50.0,
            };
            while now >= next_frame_time {
                next_frame_time += 1000.0 / fps;
            }

            if !audio_started {
                audio_started = true;
                let _: Promise = audio_ctx.resume().expect("Unable to start audio playback");
            }

            emulator.render_frame(&mut renderer, &mut audio_output, &mut Null);
        }
        Event::WindowEvent { event: window_event, window_id }
            if window_id == renderer.window().id() =>
        {
            emulator.handle_window_event(&window_event);

            if let WindowEvent::CloseRequested = window_event {
                *control_flow = ControlFlow::Exit;
            }
        }
        _ => {}
    });
}
