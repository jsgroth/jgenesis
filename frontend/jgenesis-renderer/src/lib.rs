pub mod config;
pub mod renderer;
#[cfg(feature = "ttf")]
mod ttf;

use config::RendererConfig;

/// Attempts to determine the max 2D texture dimension supported by the adapter that wgpu will
/// return for the given renderer config.
///
/// Returns `None` if unable to obtain a wgpu adapter.
#[must_use]
pub async fn try_get_max_texture_dimension_2d(config: RendererConfig) -> Option<u32> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: config.wgpu_backend.to_wgpu(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: config.wgpu_power_preference.to_wgpu(),
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        })
        .await
        .ok()?;

    Some(adapter.limits().max_texture_dimension_2d)
}
