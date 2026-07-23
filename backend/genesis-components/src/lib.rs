pub mod audio;
pub mod cartridge;
pub mod debug;
pub mod input;
pub mod memory;
pub(crate) mod svp;
pub mod timing;
pub mod vdp;
pub mod ym2612;

use crate::timing::CycleCounters;
use crate::vdp::{DarkenColors, Vdp, VdpConfig};
use genesis_config::{GenParParams, GenesisEmulatorConfig};
use jgenesis_common::frontend::{RenderFrameOptions, Renderer, TimingMode};
use std::cmp;

pub const SPRITE_LIMITS_MODAL_MESSAGE: &str = "Sprite limits are disabled; may cause glitches";

pub trait GenesisEmulatorConfigExt {
    #[must_use]
    fn to_vdp_config(&self, color_adjustment: DarkenColors) -> VdpConfig;

    #[must_use]
    fn to_gen_par_params(&self) -> GenParParams;
}

impl GenesisEmulatorConfigExt for GenesisEmulatorConfig {
    fn to_vdp_config(&self, color_adjustment: DarkenColors) -> VdpConfig {
        VdpConfig {
            enforce_sprite_limits: !self.remove_sprite_limits,
            non_linear_color_scale: self.non_linear_color_scale,
            deinterlace: self.deinterlace,
            render_vertical_border: self.render_vertical_border,
            render_horizontal_border: self.render_horizontal_border,
            plane_a_enabled: self.plane_a_enabled,
            plane_b_enabled: self.plane_b_enabled,
            sprites_enabled: self.sprites_enabled,
            window_enabled: self.window_enabled,
            color_adjustment,
        }
    }

    fn to_gen_par_params(&self) -> GenParParams {
        GenParParams {
            force_square_in_h40: self.force_square_pixels_in_h40,
            adjust_for_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
            anamorphic_widescreen: self.anamorphic_widescreen,
        }
    }
}

#[inline]
#[must_use]
pub fn target_framerate(vdp: &Vdp, timing_mode: TimingMode) -> f64 {
    let mclk_frequency = match timing_mode {
        TimingMode::Ntsc => crate::audio::NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => crate::audio::PAL_GENESIS_MCLK_FREQUENCY,
    };

    mclk_frequency / (vdp::MCLK_CYCLES_PER_SCANLINE as f64) / vdp.average_scanlines_per_frame()
}

/// Render the current VDP frame buffer.
///
/// # Errors
///
/// This function will propagate any error returned by the renderer.
pub fn render_frame<R: Renderer>(
    timing_mode: TimingMode,
    vdp: &Vdp,
    config: &GenesisEmulatorConfig,
    renderer: &mut R,
) -> Result<(), R::Err> {
    let frame_size = vdp.frame_size();
    let pixel_aspect_ratio = config.aspect_ratio.to_pixel_aspect_ratio(
        timing_mode,
        frame_size,
        config.to_gen_par_params(),
    );
    let target_fps = target_framerate(vdp, timing_mode);

    renderer.render_frame(
        vdp.frame_buffer(),
        frame_size,
        target_fps,
        RenderFrameOptions {
            pixel_aspect_ratio,
            composite_params: Some(vdp.composite_params()),
            ..RenderFrameOptions::default()
        },
    )
}

// If a long DMA is in progress (i.e. the DMA will not finish on this line), preemptively skip the
// 68000 forward by a large number of mclk cycles (up to 1250).
//
// This function is public so that it can be used by the Sega CD core
#[inline]
pub fn check_for_long_dma_skip<const REFRESH_INTERVAL: u32>(
    vdp: &Vdp,
    cycles: &mut CycleCounters<REFRESH_INTERVAL>,
) {
    if !vdp.long_halting_dma_in_progress() {
        return;
    }

    if !cycles.z80_halt {
        // Don't advance for very long time slices if the Z80 is still active; doing so causes
        // video/audio desync in Overdrive 2.
        // 8 68K cycles is slightly less than 4 Z80 cycles
        cycles.m68k_wait_cpu_cycles = 8;
        return;
    }

    // Skip as close as possible to the end of the current scanline
    let wait_cycles = cmp::max(
        cycles.m68k_wait_cpu_cycles,
        cmp::min(
            cycles.max_wait_cpu_cycles,
            (vdp::MCLK_CYCLES_PER_SCANLINE - vdp.scanline_mclk()) as u32
                / cycles.m68k_divider_u32.get(),
        ),
    );
    cycles.m68k_wait_cpu_cycles = wait_cycles;

    log::trace!(
        "Skipping {wait_cycles} 68000 CPU cycles in long DMA optimization, scanline mclk is {}",
        vdp.scanline_mclk()
    );
}
