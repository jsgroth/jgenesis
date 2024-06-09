use crate::vdp::colors::ColorModifier;
use crate::vdp::registers::{
    DebugRegister, HorizontalDisplaySize, HorizontalScrollMode, InterlacingMode, Plane, Registers,
    ScrollSize, VerticalDisplaySize, VerticalScrollMode, RIGHT_BORDER,
};
use crate::vdp::sprites::SpritePixel;
use crate::vdp::{colors, Cram, FrameBuffer, TimingModeExt, Vdp, Vram, Vsram};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;
use std::cmp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterLine {
    pub line: u16,
    pub in_v_border: bool,
}

impl RasterLine {
    pub fn from_scanline(scanline: u16, registers: &Registers, timing_mode: TimingMode) -> Self {
        let v_display_size = registers.vertical_display_size;
        let active_scanlines = v_display_size.active_scanlines();
        let scanlines_per_frame = timing_mode.scanlines_per_frame();
        let top_border = v_display_size.top_border(timing_mode);

        if scanline < active_scanlines {
            // Active display
            Self { line: scanline, in_v_border: false }
        } else if scanline >= scanlines_per_frame - top_border {
            // Top border; bottom line is raster line 511
            let line = 512 - (scanlines_per_frame - scanline);
            Self { line, in_v_border: true }
        } else {
            // Bottom border and VBlank
            Self { line: scanline, in_v_border: true }
        }
    }

    pub fn to_interlaced_even(self) -> Self {
        Self { line: (2 * self.line) & 0x1FF, in_v_border: self.in_v_border }
    }

    pub fn to_interlaced_odd(self) -> Self {
        Self { line: (2 * self.line + 1) & 0x1FF, in_v_border: self.in_v_border }
    }

    pub fn to_frame_buffer_row(
        self,
        top_border: u16,
        timing_mode: TimingMode,
        render_vertical_border: bool,
    ) -> Option<u32> {
        if render_vertical_border {
            if self.line >= 512 - top_border {
                // Top border
                Some((self.line - (512 - top_border)).into())
            } else if self.line < timing_mode.rendered_lines_per_frame() - top_border {
                // Active display or bottom border
                Some((self.line + top_border).into())
            } else {
                // VBlank
                None
            }
        } else {
            // If not rendering the vertical border, frame buffer row == raster line
            (!self.in_v_border).then_some(self.line.into())
        }
    }

    pub fn previous_line(self, v_display_size: VerticalDisplaySize) -> Self {
        if self.line == 0 {
            Self { line: 511, in_v_border: true }
        } else {
            let line = self.line - 1;
            Self { line, in_v_border: line >= v_display_size.active_scanlines() }
        }
    }
}

impl Vdp {
    pub(super) fn render_scanline(&mut self, scanline: u16, starting_pixel: u16) {
        if starting_pixel
            >= self.latched_registers.horizontal_display_size.active_display_pixels() - 10
        {
            // Don't re-render for mid-scanline writes that occur very near the end of a scanline; this can cause visual
            // glitches due to some underlying issues in how timing is handled between the 68000 and VDP
            return;
        }

        let raster_line =
            RasterLine::from_scanline(scanline, &self.latched_registers, self.timing_mode);
        let frame_buffer_row = raster_line.to_frame_buffer_row(
            self.state.top_border,
            self.timing_mode,
            self.config.render_vertical_border,
        );

        match self.latched_registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                self.do_render_scanline(
                    scanline,
                    raster_line,
                    starting_pixel,
                    frame_buffer_row,
                    false,
                );
            }
            InterlacingMode::InterlacedDouble => {
                self.do_render_scanline(
                    scanline,
                    raster_line.to_interlaced_even(),
                    starting_pixel,
                    frame_buffer_row.map(|row| 2 * row),
                    false,
                );
                self.do_render_scanline(
                    scanline,
                    raster_line.to_interlaced_odd(),
                    starting_pixel,
                    frame_buffer_row.map(|row| 2 * row + 1),
                    true,
                );
            }
        }
    }

    fn do_render_scanline(
        &mut self,
        scanline: u16,
        raster_line: RasterLine,
        starting_pixel: u16,
        frame_buffer_row: Option<u32>,
        interlaced_odd_line: bool,
    ) {
        if !self.registers.display_enabled {
            let Some(frame_buffer_row) = frame_buffer_row else { return };

            let bg_color = colors::resolve_color(
                &self.cram,
                self.registers.background_palette,
                self.registers.background_color_id,
            );
            self.fill_frame_buffer_row(frame_buffer_row, starting_pixel, bg_color);

            // Clear sprite pixel buffer in case display is enabled during active display
            if interlaced_odd_line {
                self.interlaced_sprite_buffers.pixels.fill(SpritePixel::default());
            } else {
                self.sprite_buffers.pixels.fill(SpritePixel::default());
            }

            return;
        }

        // Only perform sprite pixel and/or right border rendering if rendering from the start of the line
        if starting_pixel == 0 {
            // Sprite pixel rendering + tile fetching is not performed inside the non-forgotten vertical border except
            // on the line immediately following the end of active display
            if !raster_line.in_v_border
                || self.state.v_border_forgotten
                || raster_line.line
                    == self.latched_registers.vertical_display_size.active_scanlines()
            {
                self.render_sprite_pixels(raster_line, interlaced_odd_line);
            }

            // Check if the previous line's right border should be rendered
            // This needs to happen after the previous line is rendered because it depends on which sprite tiles were
            // fetched for the next/current line
            if self.config.render_horizontal_border {
                let prev_raster_line =
                    raster_line.previous_line(self.latched_registers.vertical_display_size);
                if !prev_raster_line.in_v_border
                    || self.state.v_border_forgotten
                    || prev_raster_line.line == 511
                {
                    if let Some(right_border_row) = prev_raster_line.to_frame_buffer_row(
                        self.state.top_border,
                        self.timing_mode,
                        self.config.render_vertical_border,
                    ) {
                        self.render_right_border(
                            right_border_row,
                            self.state.last_h_scroll_a,
                            self.state.last_h_scroll_b,
                        );
                    }
                }
            }
        }

        if raster_line.in_v_border && !self.state.v_border_forgotten && raster_line.line != 511 {
            if let Some(frame_buffer_row) = frame_buffer_row {
                self.render_vertical_border_line(scanline, frame_buffer_row, starting_pixel);
            }
            return;
        }

        let Some(frame_buffer_row) = frame_buffer_row else { return };

        self.render_pixels_in_scanline(
            raster_line,
            starting_pixel,
            frame_buffer_row,
            interlaced_odd_line,
        );
    }

    fn fill_frame_buffer_row(&mut self, row: u32, starting_pixel: u16, color: u16) {
        let screen_width = self.screen_width();

        let left_border = self.latched_registers.horizontal_display_size.left_border();
        let starting_col =
            if starting_pixel == 0 { 0 } else { u32::from(starting_pixel + left_border) };

        for pixel in starting_col..screen_width {
            set_in_frame_buffer(
                &mut self.frame_buffer,
                row,
                pixel,
                color,
                ColorModifier::None,
                screen_width,
                self.config.emulate_non_linear_dac,
            );
        }
    }

    #[allow(clippy::identity_op)]
    fn render_pixels_in_scanline(
        &mut self,
        raster_line: RasterLine,
        starting_pixel: u16,
        frame_buffer_row: u32,
        interlaced_odd_line: bool,
    ) {
        let sprite_buffers = if interlaced_odd_line {
            &self.interlaced_sprite_buffers
        } else {
            &self.sprite_buffers
        };

        let bg_color = colors::resolve_color(
            &self.cram,
            self.registers.background_palette,
            self.registers.background_color_id,
        );

        let screen_width = self.screen_width();

        let cell_height = self.latched_registers.interlacing_mode.cell_height();
        let v_scroll_size = self.latched_registers.vertical_scroll_size;
        let h_scroll_size = self.latched_registers.horizontal_scroll_size;

        let (h_scroll_size_pixels, v_scroll_size_pixels) = match (h_scroll_size, v_scroll_size) {
            // An invalid H scroll size always produces 32x1 scroll planes
            (ScrollSize::Invalid, _) => (32 * 8, 1 * 8),
            // An invalid V scroll size with valid H scroll size functions as a size of 32
            (_, ScrollSize::Invalid) => (h_scroll_size.to_pixels(), 32 * 8),
            _ => (h_scroll_size.to_pixels(), v_scroll_size.to_pixels()),
        };

        let scroll_line_bit_mask = match self.latched_registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => v_scroll_size_pixels - 1,
            InterlacingMode::InterlacedDouble => ((v_scroll_size_pixels - 1) << 1) | 0x01,
        };

        let h_scroll_scanline = match self.latched_registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => raster_line.line,
            InterlacingMode::InterlacedDouble => raster_line.line / 2,
        };
        let (h_scroll_a, h_scroll_b) = read_h_scroll(
            &self.vram,
            self.latched_registers.h_scroll_table_base_addr,
            self.latched_registers.horizontal_scroll_mode,
            // Only the lowest 8 bits of raster line are used for H scroll lookups
            h_scroll_scanline & 0xFF,
        );
        self.state.last_h_scroll_a = h_scroll_a;
        self.state.last_h_scroll_b = h_scroll_b;

        let mut scroll_a_nt_row = u16::MAX;
        let mut scroll_a_nt_col = u16::MAX;
        let mut scroll_a_nt_word = NameTableWord::default();

        let mut scroll_b_nt_row = u16::MAX;
        let mut scroll_b_nt_col = u16::MAX;
        let mut scroll_b_nt_word = NameTableWord::default();

        let active_display_pixels =
            self.latched_registers.horizontal_display_size.active_display_pixels();
        let active_display_cells = active_display_pixels / 8;

        let (start_col, end_col, pixel_offset) = if self.config.render_horizontal_border {
            let left_border: u32 =
                self.latched_registers.horizontal_display_size.left_border().into();
            let start_col =
                if starting_pixel == 0 { 0 } else { u32::from(starting_pixel) + left_border };
            let end_col = left_border + u32::from(active_display_pixels) + u32::from(RIGHT_BORDER);

            (start_col, end_col, left_border as i16)
        } else {
            (starting_pixel.into(), active_display_pixels.into(), 0)
        };

        for frame_buffer_col in start_col..end_col {
            let pixel = frame_buffer_col as i16 - pixel_offset;

            // If fine horizontal scroll is used (H scroll % 16 != 0), all columns are offset by the fine H scroll value
            // for V scroll lookup purposes. The leftmost 1 to 15 pixels will display from column -1, then columns 0-19
            // will display as normal after that (or columns 0-16 in H32 mode).
            let h_cell_a = div_floor(pixel - (h_scroll_a & 15) as i16, 8);
            let h_cell_b = div_floor(pixel - (h_scroll_b & 15) as i16, 8);

            let (v_scroll_a, v_scroll_b) = read_v_scroll(
                h_cell_a,
                h_cell_b,
                &self.vsram,
                &self.latched_registers,
                self.latched_full_screen_v_scroll,
            );

            let scrolled_scanline_a =
                raster_line.line.wrapping_add(v_scroll_a) & scroll_line_bit_mask;
            let scroll_a_v_cell = scrolled_scanline_a / cell_height;

            let scrolled_scanline_b =
                raster_line.line.wrapping_add(v_scroll_b) & scroll_line_bit_mask;
            let scroll_b_v_cell = scrolled_scanline_b / cell_height;

            let scrolled_pixel_a =
                (pixel as u16).wrapping_sub(h_scroll_a) & (h_scroll_size_pixels - 1);
            let scroll_a_h_cell = scrolled_pixel_a / 8;

            let scrolled_pixel_b =
                (pixel as u16).wrapping_sub(h_scroll_b) & (h_scroll_size_pixels - 1);
            let scroll_b_h_cell = scrolled_pixel_b / 8;

            if scroll_a_v_cell != scroll_a_nt_row || scroll_a_h_cell != scroll_a_nt_col {
                scroll_a_nt_word = read_name_table_word(
                    &self.vram,
                    self.latched_registers.scroll_a_base_nt_addr,
                    h_scroll_size.into(),
                    scroll_a_v_cell,
                    scroll_a_h_cell,
                );
                scroll_a_nt_row = scroll_a_v_cell;
                scroll_a_nt_col = scroll_a_h_cell;
            }

            if scroll_b_v_cell != scroll_b_nt_row || scroll_b_h_cell != scroll_b_nt_col {
                scroll_b_nt_word = read_name_table_word(
                    &self.vram,
                    self.latched_registers.scroll_b_base_nt_addr,
                    h_scroll_size.into(),
                    scroll_b_v_cell,
                    scroll_b_h_cell,
                );
                scroll_b_nt_row = scroll_b_v_cell;
                scroll_b_nt_col = scroll_b_h_cell;

                if h_cell_b < active_display_cells as i16 {
                    self.state.last_scroll_b_palettes[0] = self.state.last_scroll_b_palettes[1];
                    self.state.last_scroll_b_palettes[1] = scroll_b_nt_word.palette;
                }
            }

            let scroll_a_color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: scroll_a_nt_word.vertical_flip,
                    horizontal_flip: scroll_a_nt_word.horizontal_flip,
                    pattern_generator: scroll_a_nt_word.pattern_generator,
                    row: scrolled_scanline_a,
                    col: scrolled_pixel_a,
                    cell_height,
                },
            );
            let scroll_b_color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: scroll_b_nt_word.vertical_flip,
                    horizontal_flip: scroll_b_nt_word.horizontal_flip,
                    pattern_generator: scroll_b_nt_word.pattern_generator,
                    row: scrolled_scanline_b,
                    col: scrolled_pixel_b,
                    cell_height,
                },
            );

            let in_window = self.latched_registers.is_in_window(raster_line.line, pixel as u16);
            let (window_priority, window_palette, window_color_id) = if in_window {
                let window_v_cell = raster_line.line / cell_height;

                let window_width_cells =
                    self.latched_registers.horizontal_display_size.window_width_cells();
                let window_pixel = (pixel as u16) & (window_width_cells * 8 - 1);
                let window_h_cell = window_pixel / 8;

                let window_nt_word = read_name_table_word(
                    &self.vram,
                    self.latched_registers.window_base_nt_addr,
                    window_width_cells,
                    window_v_cell,
                    window_h_cell,
                );
                let window_color_id = read_pattern_generator(
                    &self.vram,
                    PatternGeneratorArgs {
                        vertical_flip: window_nt_word.vertical_flip,
                        horizontal_flip: window_nt_word.horizontal_flip,
                        pattern_generator: window_nt_word.pattern_generator,
                        row: raster_line.line,
                        col: window_pixel,
                        cell_height,
                    },
                );
                (window_nt_word.priority, window_nt_word.palette, window_color_id)
            } else {
                (false, 0, 0)
            };

            let SpritePixel {
                palette: sprite_palette,
                color_id: sprite_color_id,
                priority: sprite_priority,
            } = sprite_buffers
                .pixels
                .get(pixel as usize)
                .copied()
                .unwrap_or(SpritePixel::default());

            let (scroll_a_priority, scroll_a_palette, scroll_a_color_id) = if in_window {
                // Window replaces scroll A if this pixel is inside the window
                (window_priority, window_palette, window_color_id)
            } else {
                (scroll_a_nt_word.priority, scroll_a_nt_word.palette, scroll_a_color_id)
            };

            let (pixel_color, color_modifier) = determine_pixel_color(
                &self.cram,
                self.debug_register,
                PixelColorArgs {
                    sprite_priority,
                    sprite_palette,
                    sprite_color_id,
                    scroll_a_priority,
                    scroll_a_palette,
                    scroll_a_color_id,
                    scroll_b_priority: scroll_b_nt_word.priority,
                    scroll_b_palette: scroll_b_nt_word.palette,
                    scroll_b_color_id,
                    bg_color,
                    shadow_highlight_flag: self.latched_registers.shadow_highlight_flag,
                    in_h_border: !(0..active_display_pixels as i16).contains(&pixel),
                    in_v_border: raster_line.in_v_border && !self.state.v_border_forgotten,
                },
            );

            set_in_frame_buffer(
                &mut self.frame_buffer,
                frame_buffer_row,
                frame_buffer_col,
                pixel_color,
                color_modifier,
                screen_width,
                self.config.emulate_non_linear_dac,
            );
        }

        if self.config.render_horizontal_border {
            self.render_left_border(frame_buffer_row, bg_color, h_scroll_a, h_scroll_b);
        }
    }

    fn render_vertical_border_line(
        &mut self,
        scanline: u16,
        frame_buffer_row: u32,
        starting_pixel: u16,
    ) {
        match self.debug_register.forced_plane {
            Plane::Background => {
                // Fill with the background color
                let bg_color = colors::resolve_color(
                    &self.cram,
                    self.registers.background_palette,
                    self.registers.background_color_id,
                );
                self.fill_frame_buffer_row(frame_buffer_row, starting_pixel, bg_color);
            }
            Plane::Sprite => {
                // Fill with color 0
                self.fill_frame_buffer_row(frame_buffer_row, starting_pixel, self.cram[0]);
            }
            Plane::ScrollA | Plane::ScrollB => {
                // What happens here is quite strange. In actual hardware, the VRAM chip continues cycling through the
                // 256 bytes in the same VRAM row as the last byte accessed during rendering, which happens to be the
                // 4th sprite tile fetched for the line immediately after active display. The VDP interprets those
                // bytes as pixels and displays them using the last palettes that were used during rendering.
                //
                // A "row" in VRAM consists of 64 4-byte groups that are each separated by 1KB due to how VRAM addresses
                // map to physical addresses in the VRAM chip. See:
                // https://gendev.spritesmind.net/forum/viewtopic.php?p=17583#17583
                let h_display_size = self.registers.horizontal_display_size;
                let screen_width = self.screen_width();

                let (start_pixel, end_pixel) = if self.config.render_horizontal_border {
                    (0, screen_width as u16)
                } else {
                    let left_border = h_display_size.left_border();
                    let active_display_pixels = h_display_size.active_display_pixels();
                    (left_border, left_border + active_display_pixels)
                };

                // +2 here is needed to properly align with the horizontal borders in Overdrive 2
                // The number of 4-byte groups is equal to half the number of pixel clocks per line, 171 in H32 mode
                // and 210 in H40 mode
                let group_offset = (scanline + 2
                    - self.registers.vertical_display_size.active_scanlines())
                .wrapping_mul(h_display_size.pixels_including_hblank() / 2);

                let base_addr = self.sprite_buffers.last_tile_addresses[3];

                let mut current_addr = base_addr.wrapping_add(group_offset.wrapping_mul(1024));
                let mut odd_group = false;
                let mut current_group = [0; 4];

                for pixel in 0..end_pixel {
                    // +3 here is needed to properly align with the horizontal borders in Overdrive 2
                    let tile_col = (pixel + 3) % 8;
                    if pixel == 0 || tile_col == 0 {
                        current_group.copy_from_slice(
                            &self.vram[current_addr as usize..(current_addr + 4) as usize],
                        );
                        if odd_group {
                            current_addr = current_addr.wrapping_add(1024);
                        } else {
                            current_addr = current_addr.wrapping_add(7 * 1024);
                        }
                        odd_group = !odd_group;
                    }

                    if pixel < start_pixel {
                        continue;
                    }

                    let palette = self.state.last_scroll_b_palettes[((pixel / 8) & 1) as usize];
                    let current_byte = current_group[(tile_col >> 1) as usize];
                    let color_id = (current_byte >> (4 - ((tile_col & 1) << 2))) & 0x0F;
                    let color = colors::resolve_color(&self.cram, palette, color_id);

                    let frame_buffer_col = pixel - start_pixel;
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        frame_buffer_col.into(),
                        color,
                        ColorModifier::None,
                        screen_width,
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
        }
    }

    fn render_left_border(
        &mut self,
        frame_buffer_row: u32,
        bg_color: u16,
        h_scroll_a: u16,
        h_scroll_b: u16,
    ) {
        let screen_width = self.screen_width();
        let left_border: u32 = self.latched_registers.horizontal_display_size.left_border().into();

        match self.debug_register.forced_plane {
            Plane::Background => {
                // Fill border with background color
                for col in 0..left_border {
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        col,
                        bg_color,
                        ColorModifier::None,
                        screen_width,
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
            Plane::Sprite => {
                // Fill border with color 0
                let color_0 = self.cram[0];
                for col in 0..left_border {
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        col,
                        color_0,
                        ColorModifier::None,
                        screen_width,
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
            Plane::ScrollA => {
                // Actual hardware fills the non-rendered pixels with garbage that is somewhat unspecified by Overdrive
                // 2 docs; just fill them with color 0
                // Overdrive 2 depends on handling the Scroll A right border correctly but not the left border
                let border_pixels = h_scroll_a & 15;
                let border_offset =
                    16 - self.latched_registers.horizontal_display_size.left_border();
                let end_col = border_pixels.saturating_sub(border_offset);
                let color_0 = self.cram[0];

                for col in 0..end_col {
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        col.into(),
                        color_0,
                        ColorModifier::None,
                        screen_width,
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
            Plane::ScrollB => {
                // Render pixels from sprite tiles 36 and 37 using the palettes from the last 2 tiles of Scroll B in the
                // previous rendered line
                let border_pixels = h_scroll_b & 15;
                let border_offset =
                    16 - self.latched_registers.horizontal_display_size.left_border();
                let end_col = border_pixels.saturating_sub(border_offset);

                for col in 0..end_col {
                    let pixel = 15 - (end_col - 1 - col);
                    self.render_horizontal_border_sprite_pixel(
                        frame_buffer_row,
                        col.into(),
                        pixel,
                        36,
                    );
                }
            }
        }
    }

    fn render_right_border(&mut self, frame_buffer_row: u32, h_scroll_a: u16, h_scroll_b: u16) {
        let screen_width = self.screen_width() as u16;
        let right_border_start = screen_width - RIGHT_BORDER;

        match self.debug_register.forced_plane {
            Plane::Background => {
                // Fill border with background color
                let bg_color = colors::resolve_color(
                    &self.cram,
                    self.registers.background_palette,
                    self.registers.background_color_id,
                );
                for col in right_border_start..screen_width {
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        col.into(),
                        bg_color,
                        ColorModifier::None,
                        screen_width.into(),
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
            Plane::Sprite => {
                // Fill border with color 0
                let color_0 = self.cram[0];
                for col in right_border_start..screen_width {
                    set_in_frame_buffer(
                        &mut self.frame_buffer,
                        frame_buffer_row,
                        col.into(),
                        color_0,
                        ColorModifier::None,
                        screen_width.into(),
                        self.config.emulate_non_linear_dac,
                    );
                }
            }
            Plane::ScrollA => {
                // Render pixels from sprite tiles 0 and 1 using the palettes from the last 2 tiles of Scroll B in the
                // previous rendered line
                let last_column_end = right_border_start + cmp::min(h_scroll_a & 15, RIGHT_BORDER);

                for col in last_column_end..screen_width {
                    let pixel = col - last_column_end;
                    self.render_horizontal_border_sprite_pixel(
                        frame_buffer_row,
                        col.into(),
                        pixel,
                        0,
                    );
                }
            }
            Plane::ScrollB => {
                // Render pixels from sprite tiles 4 and 5 using the palettes from the last 2 tiles of Scroll B in the
                // previous rendered line
                let last_column_end = right_border_start + cmp::min(h_scroll_b & 15, RIGHT_BORDER);

                for col in last_column_end..screen_width {
                    let pixel = col - last_column_end;
                    self.render_horizontal_border_sprite_pixel(
                        frame_buffer_row,
                        col.into(),
                        pixel,
                        4,
                    );
                }
            }
        }
    }

    fn render_horizontal_border_sprite_pixel(
        &mut self,
        frame_buffer_row: u32,
        frame_buffer_col: u32,
        pixel: u16,
        base_sprite: u16,
    ) {
        let sprite_tile = base_sprite + pixel / 8;
        let sprite_col = pixel % 8;
        let vram_addr =
            self.sprite_buffers.last_tile_addresses[sprite_tile as usize] + (sprite_col >> 1);
        let color_id = (self.vram[vram_addr as usize] >> (4 - ((sprite_col & 1) << 2))) & 0x0F;
        let palette = self.state.last_scroll_b_palettes[(pixel / 8) as usize];
        let color = colors::resolve_color(&self.cram, palette, color_id);

        let screen_width = self.screen_width();
        set_in_frame_buffer(
            &mut self.frame_buffer,
            frame_buffer_row,
            frame_buffer_col,
            color,
            ColorModifier::None,
            screen_width,
            self.config.emulate_non_linear_dac,
        );
    }
}

fn div_floor(a: i16, b: i16) -> i16 {
    assert_ne!(b, 0);

    if a == 0 {
        0
    } else if a.signum() == b.signum() || a % b == 0 {
        a / b
    } else {
        a / b - 1
    }
}

fn set_in_frame_buffer(
    frame_buffer: &mut FrameBuffer,
    row: u32,
    col: u32,
    color: u16,
    modifier: ColorModifier,
    screen_width: u32,
    emulate_non_linear_dac: bool,
) {
    let r = ((color >> 1) & 0x07) as u8;
    let g = ((color >> 5) & 0x07) as u8;
    let b = ((color >> 9) & 0x07) as u8;
    let a = (color >> 15) as u8;
    let rgb_color = colors::gen_to_rgba(r, g, b, a, modifier, emulate_non_linear_dac);

    frame_buffer[(row * screen_width + col) as usize] = rgb_color;
}

fn read_v_scroll(
    h_cell_a: i16,
    h_cell_b: i16,
    vsram: &Vsram,
    registers: &Registers,
    latched_full_screen_v_scroll: (u16, u16),
) -> (u16, u16) {
    let (v_scroll_a, v_scroll_b) = match registers.vertical_scroll_mode {
        VerticalScrollMode::FullScreen => latched_full_screen_v_scroll,
        VerticalScrollMode::TwoCell => {
            let v_scroll_a =
                read_two_cell_v_scroll(h_cell_a, 0, vsram, registers.horizontal_display_size);
            let v_scroll_b =
                read_two_cell_v_scroll(h_cell_b, 2, vsram, registers.horizontal_display_size);
            (v_scroll_a, v_scroll_b)
        }
    };

    let v_scroll_mask = registers.interlacing_mode.v_scroll_mask();
    (v_scroll_a & v_scroll_mask, v_scroll_b & v_scroll_mask)
}

fn read_two_cell_v_scroll(
    h_cell: i16,
    offset: usize,
    vsram: &Vsram,
    h_display_size: HorizontalDisplaySize,
) -> u16 {
    let active_display_cells = (h_display_size.active_display_pixels() / 8) as i16;
    if h_cell < 0 {
        // Column -1 behaves weirdly.
        // In H40 mode, it uses a V scroll value of VSRAM[$4C] & VSRAM[$4E] for both backgrounds.
        // In H32 mode, it always uses a V scroll value of 0.
        // Source: http://gendev.spritesmind.net/forum/viewtopic.php?t=737&postdays=0&postorder=asc&start=30
        match h_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => 0,
            HorizontalDisplaySize::FortyCell => {
                u16::from_be_bytes([vsram[0x4C] & vsram[0x4E], vsram[0x4D] & vsram[0x4F]])
            }
        }
    } else if h_cell < active_display_cells {
        let addr = 4 * (h_cell as usize / 2) + offset;
        u16::from_be_bytes([vsram[addr], vsram[addr + 1]])
    } else {
        0
    }
}

fn read_h_scroll(
    vram: &Vram,
    h_scroll_table_addr: u16,
    h_scroll_mode: HorizontalScrollMode,
    scanline: u16,
) -> (u16, u16) {
    let h_scroll_addr = match h_scroll_mode {
        HorizontalScrollMode::FullScreen => h_scroll_table_addr,
        HorizontalScrollMode::Cell => h_scroll_table_addr.wrapping_add(32 * (scanline / 8)),
        HorizontalScrollMode::Line => h_scroll_table_addr.wrapping_add(4 * scanline),
        HorizontalScrollMode::Invalid => h_scroll_table_addr.wrapping_add(4 * (scanline & 0x7)),
    };

    let h_scroll_a =
        u16::from_be_bytes([vram[h_scroll_addr as usize], vram[(h_scroll_addr + 1) as usize]]);
    let h_scroll_b = u16::from_be_bytes([
        vram[(h_scroll_addr + 2) as usize],
        vram[(h_scroll_addr + 3) as usize],
    ]);

    (h_scroll_a & 0x03FF, h_scroll_b & 0x03FF)
}

#[derive(Debug, Clone, Copy, Default)]
struct NameTableWord {
    priority: bool,
    palette: u8,
    vertical_flip: bool,
    horizontal_flip: bool,
    pattern_generator: u16,
}

fn read_name_table_word(
    vram: &Vram,
    base_addr: u16,
    name_table_width: u16,
    row: u16,
    col: u16,
) -> NameTableWord {
    // Nametable size is limited to 8KB
    // If dimensions are 64x128, 128x64, or 128x128 then addresses will wrap at the 8KB boundary
    let relative_addr = (2 * (row * name_table_width + col)) & 0x1FFF;
    let addr = base_addr.wrapping_add(relative_addr);
    let word = u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);

    NameTableWord {
        priority: word.bit(15),
        palette: ((word >> 13) & 0x03) as u8,
        vertical_flip: word.bit(12),
        horizontal_flip: word.bit(11),
        pattern_generator: word & 0x07FF,
    }
}

#[derive(Debug, Clone)]
pub struct PatternGeneratorArgs {
    pub vertical_flip: bool,
    pub horizontal_flip: bool,
    pub pattern_generator: u16,
    pub row: u16,
    pub col: u16,
    pub cell_height: u16,
}

#[inline]
pub fn read_pattern_generator(
    vram: &Vram,
    PatternGeneratorArgs {
        vertical_flip,
        horizontal_flip,
        pattern_generator,
        row,
        col,
        cell_height,
    }: PatternGeneratorArgs,
) -> u8 {
    let cell_row =
        if vertical_flip { cell_height - 1 - (row % cell_height) } else { row % cell_height };
    let cell_col = if horizontal_flip { 7 - (col % 8) } else { col % 8 };

    let cell_addr = (4 * cell_height).wrapping_mul(pattern_generator);
    let addr = (cell_addr + 4 * cell_row + (cell_col >> 1)) as usize;
    (vram[addr] >> (4 - ((cell_col & 0x01) << 2))) & 0x0F
}

#[derive(Debug, Clone, Copy)]
struct UnresolvedColor {
    palette: u8,
    color_id: u8,
    is_sprite: bool,
}

struct PixelColorArgs {
    sprite_priority: bool,
    sprite_palette: u8,
    sprite_color_id: u8,
    scroll_a_priority: bool,
    scroll_a_palette: u8,
    scroll_a_color_id: u8,
    scroll_b_priority: bool,
    scroll_b_palette: u8,
    scroll_b_color_id: u8,
    bg_color: u16,
    shadow_highlight_flag: bool,
    in_h_border: bool,
    in_v_border: bool,
}

#[inline]
#[allow(clippy::unnested_or_patterns)]
fn determine_pixel_color(
    cram: &Cram,
    debug_register: DebugRegister,
    PixelColorArgs {
        sprite_priority,
        sprite_palette,
        sprite_color_id,
        scroll_a_priority,
        scroll_a_palette,
        scroll_a_color_id,
        scroll_b_priority,
        scroll_b_palette,
        scroll_b_color_id,
        bg_color,
        shadow_highlight_flag,
        in_h_border,
        in_v_border,
    }: PixelColorArgs,
) -> (u16, ColorModifier) {
    let sprite_cram_idx = (sprite_palette << 4) | sprite_color_id;
    let scroll_a_cram_idx = (scroll_a_palette << 4) | scroll_a_color_id;
    let scroll_b_cram_idx = (scroll_b_palette << 4) | scroll_b_color_id;

    if in_h_border {
        let color = match debug_register.forced_plane {
            Plane::Background => bg_color,
            Plane::Sprite => cram[0],
            Plane::ScrollA => cram[scroll_a_cram_idx as usize],
            Plane::ScrollB => cram[scroll_b_cram_idx as usize],
        };
        return (color, ColorModifier::None);
    }

    if debug_register.display_disabled || in_v_border {
        let color = match debug_register.forced_plane {
            Plane::Background => bg_color,
            Plane::Sprite => cram[sprite_cram_idx as usize],
            Plane::ScrollA => cram[scroll_a_cram_idx as usize],
            Plane::ScrollB => cram[scroll_b_cram_idx as usize],
        };
        return (color, ColorModifier::None);
    };

    let mut modifier = if shadow_highlight_flag && !scroll_a_priority && !scroll_b_priority {
        // If shadow/highlight bit is set and all priority flags are 0, default modifier to shadow
        ColorModifier::Shadow
    } else {
        ColorModifier::None
    };

    let sprite =
        UnresolvedColor { palette: sprite_palette, color_id: sprite_color_id, is_sprite: true };
    let scroll_a = UnresolvedColor {
        palette: scroll_a_palette,
        color_id: scroll_a_color_id,
        is_sprite: false,
    };
    let scroll_b = UnresolvedColor {
        palette: scroll_b_palette,
        color_id: scroll_b_color_id,
        is_sprite: false,
    };
    let colors = match (sprite_priority, scroll_a_priority, scroll_b_priority) {
        (false, false, false) | (true, false, false) | (true, true, false) | (true, true, true) => {
            [sprite, scroll_a, scroll_b]
        }
        (false, true, false) => [scroll_a, sprite, scroll_b],
        (false, false, true) => [scroll_b, sprite, scroll_a],
        (true, false, true) => [sprite, scroll_b, scroll_a],
        (false, true, true) => [scroll_a, scroll_b, sprite],
    };

    for UnresolvedColor { palette, color_id, is_sprite } in colors {
        if color_id == 0 {
            // Pixel is transparent
            continue;
        }

        if shadow_highlight_flag && is_sprite && palette == 3 {
            if color_id == 14 {
                // Palette 3 + color 14 = highlight; sprite is transparent, underlying pixel is highlighted
                modifier += ColorModifier::Highlight;
                continue;
            } else if color_id == 15 {
                // Palette 3 + color 15 = shadow; sprite is transparent, underlying pixel is shadowed
                modifier = ColorModifier::Shadow;
                continue;
            }
        }

        let cram_idx_mask = match debug_register.forced_plane {
            Plane::Background => 0x3F,
            Plane::Sprite => sprite_cram_idx,
            Plane::ScrollA => scroll_a_cram_idx,
            Plane::ScrollB => scroll_b_cram_idx,
        };
        let cram_idx = ((palette << 4) | color_id) & cram_idx_mask;

        let color = cram[cram_idx as usize];
        // Sprite color id 14 is never shadowed/highlighted, and neither is a sprite with the priority
        // bit set
        let modifier = if is_sprite && (color_id == 14 || sprite_priority) {
            ColorModifier::None
        } else {
            modifier
        };

        // Set alpha bit to indicate that the backdrop color was not used (needed by 32X)
        return (color | 0x8000, modifier);
    }

    let fallback_color = match debug_register.forced_plane {
        Plane::Background => bg_color,
        Plane::Sprite => cram[sprite_cram_idx as usize],
        Plane::ScrollA => cram[scroll_a_cram_idx as usize],
        Plane::ScrollB => cram[scroll_b_cram_idx as usize],
    };

    // Clear alpha bit to indicate that the backdrop color was used (needed by 32X)
    (fallback_color & 0x7FFF, modifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_div_floor() {
        assert_eq!(div_floor(0, 5), 0);
        assert_eq!(div_floor(8, 4), 2);
        assert_eq!(div_floor(9, 4), 2);
        assert_eq!(div_floor(-9, -4), 2);
        assert_eq!(div_floor(-9, 4), -3);
        assert_eq!(div_floor(-8, 4), -2);
    }
}
