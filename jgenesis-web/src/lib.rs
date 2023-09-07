use genesis_core::{GenesisAspectRatio, GenesisEmulator, GenesisEmulatorConfig, GenesisInputs};
use jgenesis_renderer::config::{
    FilterMode, PrescaleFactor, RendererConfig, VSyncMode, WgpuBackend,
};
use jgenesis_renderer::renderer::WgpuRenderer;
use jgenesis_traits::frontend::{AudioOutput, SaveWriter, TickEffect, TickableEmulator};
use rfd::AsyncFileDialog;
use wasm_bindgen::prelude::*;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use winit::platform::web::WindowExtWebSys;
use winit::window::{Window, WindowBuilder};

struct Null;

impl AudioOutput for Null {
    type Err = ();

    fn push_sample(&mut self, _sample_l: f64, _sample_r: f64) -> Result<(), Self::Err> {
        Ok(())
    }
}

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
#[wasm_bindgen(start)]
pub async fn run() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_log::init_with_level(log::Level::Info).expect("Unable to initialize logger");

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
        filter_mode: FilterMode::Linear,
        use_webgl2_limits: true,
    };
    let renderer = WgpuRenderer::new(window, window_size_fn, renderer_config)
        .await
        .expect("Unable to create wgpu renderer");

    let file = AsyncFileDialog::new()
        .add_filter("md", &["md", "bin"])
        .pick_file()
        .await
        .expect("No file selected");
    let rom = file.read().await;

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

    run_event_loop(event_loop, renderer, emulator);
}

fn run_event_loop(
    event_loop: EventLoop<()>,
    mut renderer: WgpuRenderer<Window>,
    mut emulator: GenesisEmulator,
) {
    let mut inputs = GenesisInputs::default();

    event_loop.run(move |event, _, control_flow| match event {
        Event::MainEventsCleared => {
            while emulator
                .tick(&mut renderer, &mut Null, &inputs, &mut Null)
                .expect("Emulator error")
                != TickEffect::FrameRendered
            {}
        }
        Event::WindowEvent { event: window_event, window_id }
            if window_id == renderer.window().id() =>
        {
            match window_event {
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::KeyboardInput {
                    input: KeyboardInput { virtual_keycode: Some(keycode), state, .. },
                    ..
                } => {
                    let pressed = state == ElementState::Pressed;

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
                        VirtualKeyCode::Return => {
                            inputs.p1.start = pressed;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        _ => {}
    });
}
