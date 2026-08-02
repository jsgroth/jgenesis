pub mod cartridge;
pub mod debug;
pub mod memory;
pub(crate) mod svp;
pub mod vdp;
pub mod ym2612;

use crate::vdp::{DarkenColors, Vdp, VdpConfig};
use genesis_config::{GenParParams, GenesisEmulatorConfig};
use jgenesis_common::frontend::TimingMode;

pub const NTSC_GENESIS_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_GENESIS_MCLK_FREQUENCY: f64 = 53_203_424.0;

pub const SPRITE_LIMITS_MODAL_MESSAGE: &str = "Sprite limits are disabled; may cause glitches";

pub trait GenesisEmulatorConfigExt {
    #[must_use]
    fn to_vdp_config(&self, s32x_present: bool) -> VdpConfig;

    #[must_use]
    fn to_gen_par_params(&self) -> GenParParams;
}

impl GenesisEmulatorConfigExt for GenesisEmulatorConfig {
    fn to_vdp_config(&self, s32x_present: bool) -> VdpConfig {
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
            color_adjustment: if s32x_present && self.sega_32x.darken_genesis_colors {
                DarkenColors::Yes
            } else {
                DarkenColors::No
            },
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
        TimingMode::Ntsc => NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => PAL_GENESIS_MCLK_FREQUENCY,
    };

    mclk_frequency / (vdp::MCLK_CYCLES_PER_SCANLINE as f64) / vdp.average_scanlines_per_frame()
}
