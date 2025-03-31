use crate::vdp::colors::ColorModifier;
use crate::vdp::fifo::VdpFifo;
use crate::vdp::registers::{RIGHT_BORDER, Registers};
use crate::vdp::render::RasterLine;
use crate::vdp::{MAX_SCREEN_WIDTH, Vdp, colors};
use bincode::{Decode, Encode};
use std::num::NonZeroU16;
use std::ops::Range;
use std::{array, mem};

// Use NonZeroU16 to cut the buffer sizes in half
type LineDotBuffer = [Option<NonZeroU16>; MAX_SCREEN_WIDTH];

#[derive(Debug, Clone, Encode, Decode)]
struct LineBuffer {
    dots: Box<LineDotBuffer>,
    any: bool,
}

impl LineBuffer {
    fn new() -> Self {
        Self { dots: Box::new(array::from_fn(|_| None)), any: false }
    }

    fn clear(&mut self) {
        self.dots.fill(None);
        self.any = false;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CramDotBuffer {
    prev_line: LineBuffer,
    current_line: LineBuffer,
}

impl CramDotBuffer {
    pub fn new() -> Self {
        Self { prev_line: LineBuffer::new(), current_line: LineBuffer::new() }
    }

    pub fn check_for_dot(
        &mut self,
        registers: &Registers,
        fifo: &VdpFifo,
        pixel: u16,
        cram_addr: u32,
        color: u16,
    ) {
        let h_display_size = registers.horizontal_display_size;
        let active_display_range = h_display_size.active_display_h_range();
        let left_border = h_display_size.left_border();

        // +4 is necessary for correct alignment of Direct Color DMA demos
        // Also to avoid CRAM dots being visible within active display on some screens in Overdrive 1
        let left = active_display_range.start - left_border + 4;
        let right = active_display_range.end + RIGHT_BORDER + 4;

        self.check_for_dot_inner(left..right, pixel, color);

        // Hack: Use "CRAM dots" to make Direct Color DMA demos work
        // If the backdrop color is changed while display is disabled, fill in some following pixels
        // with the new backdrop color
        if !registers.display_enabled
            && (cram_addr >> 4) as u8 == registers.background_palette
            && (cram_addr & 0xF) as u8 == registers.background_color_id
        {
            let n = if fifo.len() > 1 {
                // Filling in 3 pixels is necessary to avoid vertical lines around refresh slots
                3
            } else {
                right.saturating_sub(pixel)
            };
            for i in 1..=n {
                self.check_for_dot_inner(left..right, pixel + i, color);
            }
        }
    }

    fn check_for_dot_inner(&mut self, range: Range<u16>, pixel: u16, color: u16) {
        if !range.contains(&pixel) {
            return;
        }

        let idx = (pixel - range.start) as usize;
        self.current_line.dots[idx] = Some(NonZeroU16::new(color | 0x8000).unwrap());
        self.current_line.any = true;
    }

    pub fn swap_buffers_if_needed(&mut self) {
        if !self.current_line.any && !self.prev_line.any {
            return;
        }

        mem::swap(&mut self.current_line.dots, &mut self.prev_line.dots);
        self.prev_line.any = self.current_line.any;
        self.current_line.clear();
    }
}

impl Vdp {
    pub(super) fn apply_cram_dots_previous_line(&mut self, line: RasterLine) {
        if !self.cram_dots.prev_line.any {
            return;
        }

        let timing_mode = self.timing_mode;
        let h_display_size = self.registers.horizontal_display_size;
        let v_display_size = self.registers.vertical_display_size;

        let prev_line = line.previous_line(v_display_size);
        let Some(mut fb_row) = prev_line.to_frame_buffer_row(
            v_display_size.top_border(timing_mode),
            timing_mode,
            self.config.render_vertical_border,
        ) else {
            return;
        };

        let interlaced = self.state.interlaced_frame;
        if interlaced {
            fb_row *= 2;
            fb_row += u32::from(self.state.interlaced_odd);
        }

        let screen_width = self.screen_width();
        let left_border_offset = if self.config.render_horizontal_border {
            0
        } else {
            -i32::from(h_display_size.left_border())
        };

        self.apply_cram_dots_inner(fb_row, screen_width, left_border_offset);
        if interlaced && self.config.deinterlace {
            self.apply_cram_dots_inner(fb_row ^ 1, screen_width, left_border_offset);
        }
    }

    fn apply_cram_dots_inner(&mut self, fb_row: u32, screen_width: u32, left_border_offset: i32) {
        for (x, color) in self.cram_dots.prev_line.dots.iter().copied().enumerate() {
            let Some(color) = color.map(NonZeroU16::get) else { continue };

            let fb_col = x as i32 + left_border_offset;
            if !(0..screen_width as i32).contains(&fb_col) {
                continue;
            }
            let fb_col = fb_col as u32;

            let fb_idx = (fb_row * screen_width + fb_col) as usize;

            let r = ((color >> 1) & 7) as u8;
            let g = ((color >> 5) & 7) as u8;
            let b = ((color >> 9) & 7) as u8;

            // Reuse "alpha" from the existing pixel; some 32X games depend on this (e.g. Toughman Contest)
            let a = self.frame_buffer[fb_idx].a;

            let rgba_color = colors::gen_to_rgba(
                r,
                g,
                b,
                a,
                ColorModifier::None,
                self.config.non_linear_color_scale,
            );

            self.frame_buffer[fb_idx] = rgba_color;
        }
    }
}
