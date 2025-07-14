use anyhow::anyhow;
use egui_wgpu::ScreenDescriptor;
use sdl3::event::{Event, WindowEvent};
use sdl3::video::Window;
use std::iter;
use std::time::SystemTime;

pub struct InputWindow {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    platform: egui_sdl3_platform::Platform,
    renderer: egui_wgpu::Renderer,
    start_time: SystemTime,
    // SAFETY: The window must be declared after the surface so that the surface is dropped first
    window: Window,
}

impl InputWindow {
    pub fn new(window: Window, scale_factor: f32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

        // SAFETY: The returned surface must not outlive the window
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)
        }?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..wgpu::RequestAdapterOptions::default()
        }))
        .ok_or_else(|| anyhow!("Failed to create wgpu adapter"))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))?;

        let (width, height) = window.size();
        let surface_format = surface.get_capabilities(&adapter).formats[0];
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::default(),
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let platform = egui_sdl3_platform::Platform::new(&window, scale_factor);
        let start_time = SystemTime::now();

        let renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

        Ok(Self { surface, surface_config, device, queue, platform, renderer, start_time, window })
    }

    pub fn update(&mut self, render_fn: impl FnMut(&egui::Context)) -> anyhow::Result<()> {
        let egui_input = self.platform.take_raw_input(
            SystemTime::now().duration_since(self.start_time).unwrap_or_default().as_secs_f64(),
        );

        let full_output = self.platform.context().run(egui_input, render_fn);

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Outdated) => {
                log::warn!("Skipping input window frame because wgpu surface has changed");
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => {
                log::warn!("Skipping input window frame because wgpu surface timed out");
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };
        let output_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let paint_jobs =
            self.platform.context().tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: "debugger_encoder".into(),
        });

        self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });

            // egui-wgpu requires a RenderPass with static lifetime
            let mut render_pass = render_pass.forget_lifetime();

            self.renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        Ok(())
    }

    pub fn handle_sdl_event(&mut self, event: &Event) {
        match event {
            Event::Window {
                window_id,
                win_event: WindowEvent::Resized(..) | WindowEvent::PixelSizeChanged(..),
                ..
            } if *window_id == self.window.id() => {
                let (width, height) = self.window.size();
                self.surface_config.width = width;
                self.surface_config.height = height;
                self.surface.configure(&self.device, &self.surface_config);
            }
            _ => {}
        }

        self.platform.handle_event(event);
    }
}
