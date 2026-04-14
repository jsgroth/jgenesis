mod constants;

use crate::renderer::{PipelineShader, Shaders};
use jgenesis_common::frontend::{CompositeParams, SamplesPerColorCycle};
use std::sync::Arc;
use wgpu::util::DeviceExt;

const BACKDROP_PIXELS: u32 = 6;

const FIR_BUFFER_LEN: usize = 84;

struct NtscFilters {
    luma_bsf: &'static [f32],
    chroma_bpf: &'static [f32],
    y_encode_lpf: &'static [f32],
    y_decode_lpf: &'static [f32],
    iq_lpf: &'static [f32],
}

impl NtscFilters {
    fn from(samples_per_color_cycle: SamplesPerColorCycle) -> Self {
        match samples_per_color_cycle {
            SamplesPerColorCycle::Fifteen => Self {
                luma_bsf: constants::LUMA_BSF_15_COEFFICIENTS,
                chroma_bpf: constants::CHROMA_BPF_15_COEFFICIENTS,
                y_encode_lpf: constants::Y_ENCODE_LPF_15_COEFFICIENTS,
                y_decode_lpf: constants::Y_DECODE_LPF_15_COEFFICIENTS,
                iq_lpf: constants::IQ_LPF_15_COEFFICIENTS,
            },
            SamplesPerColorCycle::Twelve => Self {
                luma_bsf: constants::LUMA_BSF_12_COEFFICIENTS,
                chroma_bpf: constants::CHROMA_BPF_12_COEFFICIENTS,
                y_encode_lpf: constants::Y_ENCODE_LPF_12_COEFFICIENTS,
                y_decode_lpf: constants::Y_DECODE_LPF_12_COEFFICIENTS,
                iq_lpf: constants::IQ_LPF_12_COEFFICIENTS,
            },
        }
    }
}

pub struct NtscShader {
    output: Arc<wgpu::Texture>,
    ntsc_frame_size: wgpu::Extent3d,
    rgb_to_ntsc_bind_group: wgpu::BindGroup,
    rgb_to_ntsc_pipeline: wgpu::ComputePipeline,
    separate_luma_chroma_bind_group: wgpu::BindGroup,
    separate_luma_chroma_pipeline: wgpu::ComputePipeline,
    luma_chroma_to_rgb_bind_group: wgpu::BindGroup,
    luma_chroma_to_rgb_pipeline: wgpu::ComputePipeline,
}

impl NtscShader {
    pub fn create(
        device: &wgpu::Device,
        shaders: &Shaders,
        input: &wgpu::Texture,
        params: CompositeParams,
    ) -> Self {
        let ntsc_texture_descriptor = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: params.upscale_factor * (input.width() + 2 * BACKDROP_PIXELS),
                height: input.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        };

        let ntsc_frame = device.create_texture(&ntsc_texture_descriptor);
        let ntsc_pass = device.create_texture(&ntsc_texture_descriptor);
        let ntsc_stop = device.create_texture(&ntsc_texture_descriptor);

        let output_frame = device.create_texture(&wgpu::TextureDescriptor {
            label: "ntsc_output_texture".into(),
            size: wgpu::Extent3d {
                width: params.upscale_factor * input.width(),
                height: input.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });

        let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let ntsc_view = ntsc_frame.create_view(&wgpu::TextureViewDescriptor::default());
        let ntsc_pass_view = ntsc_pass.create_view(&wgpu::TextureViewDescriptor::default());
        let ntsc_stop_view = ntsc_stop.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output_frame.create_view(&wgpu::TextureViewDescriptor::default());

        let filters = NtscFilters::from(params.samples_per_color_cycle);

        let luma_bsf_fir_buffer = create_fir_buffer(device, filters.luma_bsf);
        let chroma_bpf_fir_buffer = create_fir_buffer(device, filters.chroma_bpf);
        let y_encode_lpf_fir_buffer = create_fir_buffer(device, filters.y_encode_lpf);
        let y_decode_lpf_fir_buffer = create_fir_buffer(device, filters.y_decode_lpf);
        let iq_lpf_fir_buffer = create_fir_buffer(device, filters.iq_lpf);

        let fir_len = match params.samples_per_color_cycle {
            SamplesPerColorCycle::Fifteen => constants::FIR_LEN_15,
            SamplesPerColorCycle::Twelve => constants::FIR_LEN_12,
        };
        let pipeline_compilation_options = wgpu::PipelineCompilationOptions {
            constants: &[
                ("samples_per_color_cycle", u32::from(params.samples_per_color_cycle).into()),
                ("fir_len", fir_len.into()),
                ("upscale_factor", params.upscale_factor.into()),
            ],
            ..wgpu::PipelineCompilationOptions::default()
        };

        let rgb_to_ntsc_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: "rgb_to_ntsc_pipeline".into(),
                layout: None,
                module: &shaders.ntsc,
                entry_point: Some("rgb_to_ntsc"),
                compilation_options: pipeline_compilation_options.clone(),
                cache: None,
            });

        let rgb_to_ntsc_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "rgb_to_ntsc_bind_group".into(),
            layout: &rgb_to_ntsc_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        y_encode_lpf_fir_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        iq_lpf_fir_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&ntsc_view),
                },
            ],
        });

        let separate_luma_chroma_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: "separate_luma_chroma_pipeline".into(),
                layout: None,
                module: &shaders.ntsc,
                entry_point: Some("separate_luma_chroma"),
                compilation_options: pipeline_compilation_options.clone(),
                cache: None,
            });

        let separate_luma_chroma_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: "separate_luma_chroma_bind_group".into(),
                layout: &separate_luma_chroma_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Buffer(
                            luma_bsf_fir_buffer.as_entire_buffer_binding(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Buffer(
                            chroma_bpf_fir_buffer.as_entire_buffer_binding(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&ntsc_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(&ntsc_pass_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::TextureView(&ntsc_stop_view),
                    },
                ],
            });

        let luma_chroma_to_rgb_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: "luma_chroma_to_rgb_pipeline".into(),
                layout: None,
                module: &shaders.ntsc,
                entry_point: Some("luma_chroma_to_rgb"),
                compilation_options: pipeline_compilation_options.clone(),
                cache: None,
            });

        let luma_chroma_to_rgb_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: "luma_chroma_to_rgb_bind_group".into(),
            layout: &luma_chroma_to_rgb_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Buffer(
                        y_decode_lpf_fir_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::Buffer(
                        iq_lpf_fir_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::TextureView(&ntsc_pass_view),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::TextureView(&ntsc_stop_view),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
            ],
        });

        Self {
            output: Arc::new(output_frame),
            ntsc_frame_size: ntsc_texture_descriptor.size,
            rgb_to_ntsc_bind_group,
            rgb_to_ntsc_pipeline,
            separate_luma_chroma_bind_group,
            separate_luma_chroma_pipeline,
            luma_chroma_to_rgb_bind_group,
            luma_chroma_to_rgb_pipeline,
        }
    }
}

impl PipelineShader for NtscShader {
    fn draw(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: "ntsc_compute_pass".into(),
            ..wgpu::ComputePassDescriptor::default()
        });

        let ntsc_x_workgroups = self.ntsc_frame_size.width / 16
            + u32::from(!self.ntsc_frame_size.width.is_multiple_of(16));
        let output_x_workgroups =
            self.output.width() / 16 + u32::from(!self.output.width().is_multiple_of(16));
        let y_workgroups =
            self.output.height() / 16 + u32::from(!self.output.height().is_multiple_of(16));

        compute_pass.set_bind_group(0, &self.rgb_to_ntsc_bind_group, &[]);
        compute_pass.set_pipeline(&self.rgb_to_ntsc_pipeline);
        compute_pass.dispatch_workgroups(ntsc_x_workgroups, y_workgroups, 1);

        compute_pass.set_bind_group(0, &self.separate_luma_chroma_bind_group, &[]);
        compute_pass.set_pipeline(&self.separate_luma_chroma_pipeline);
        compute_pass.dispatch_workgroups(ntsc_x_workgroups, y_workgroups, 1);

        compute_pass.set_bind_group(0, &self.luma_chroma_to_rgb_bind_group, &[]);
        compute_pass.set_pipeline(&self.luma_chroma_to_rgb_pipeline);
        compute_pass.dispatch_workgroups(output_x_workgroups, y_workgroups, 1);
    }

    fn output_texture(&self) -> &Arc<wgpu::Texture> {
        &self.output
    }
}

fn create_fir_buffer(device: &wgpu::Device, coefficients: &[f32]) -> wgpu::Buffer {
    let mut fir: Vec<f32> = vec![0.0; FIR_BUFFER_LEN];

    let slice_len = coefficients.len().min(FIR_BUFFER_LEN);
    fir[..slice_len].copy_from_slice(&coefficients[..slice_len]);

    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&fir),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}
