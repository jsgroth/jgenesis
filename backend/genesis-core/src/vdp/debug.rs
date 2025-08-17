use crate::vdp;
use crate::vdp::{ColorModifier, SpriteData, Vdp, colors, render, sprites};
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView, DebugWordsView, Endian};

use crate::api::debug::{CopySpriteAttributesResult, SpriteAttributeEntry};
use crate::vdp::colors::ColorTables;
use crate::vdp::render::PatternGeneratorRowArgs;
use jgenesis_common::frontend::Color;

impl Vdp {
    pub fn copy_cram(&self, out: &mut [Color], modifier: ColorModifier) {
        for (out_color, &cram_color) in out.iter_mut().zip(self.cram.as_ref()) {
            *out_color = parse_gen_color(cram_color, modifier, &self.color_tables);
        }
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        for pattern in 0..vdp::VRAM_LEN / 32 {
            let base_idx = pattern / row_len * row_len * 64 + (pattern % row_len) * 8;

            for row in 0..8 {
                let colors = render::read_pattern_generator_row(
                    &self.vram,
                    PatternGeneratorRowArgs {
                        vertical_flip: false,
                        horizontal_flip: false,
                        pattern_generator: pattern as u16,
                        row: row as u16,
                        cell_height_shift: 3,
                    },
                );

                for (col, color_id) in colors.into_iter().enumerate() {
                    let out_idx = base_idx + row * row_len * 8 + col;
                    let color = colors::resolve_color(&self.cram, palette, color_id);
                    out[out_idx] = parse_gen_color(color, ColorModifier::None, &self.color_tables);
                    out[out_idx].a = 255;
                }
            }
        }
    }

    pub fn copy_h_scroll(&self, out: &mut [(u16, u16)]) {
        let h_scroll_table_addr = self.latched_registers.h_scroll_table_base_addr;

        for i in 0..256 {
            let h_scroll_addr = h_scroll_table_addr.wrapping_add(4 * i) as usize;
            let h_scroll_a =
                u16::from_be_bytes([self.vram[h_scroll_addr], self.vram[h_scroll_addr + 1]])
                    & 0x3FF;
            let h_scroll_b =
                u16::from_be_bytes([self.vram[h_scroll_addr + 2], self.vram[h_scroll_addr + 3]])
                    & 0x3FF;
            out[i as usize] = (h_scroll_a, h_scroll_b);
        }
    }

    pub fn dump_registers(&self, mut callback: impl FnMut(&str, &[(&str, &str)])) {
        callback(
            "Register #0",
            &[
                ("Horizontal interrupt enabled", bool_str(self.registers.h_interrupt_enabled)),
                ("HV counter latched", bool_str(self.registers.hv_counter_stopped)),
            ],
        );

        callback(
            "Register #1",
            &[
                ("Display enabled", bool_str(self.registers.display_enabled)),
                ("Vertical interrupt enabled", bool_str(self.registers.v_interrupt_enabled)),
                ("DMA enabled", bool_str(self.registers.dma_enabled)),
                ("Vertical resolution", &self.registers.vertical_display_size.to_string()),
                ("Mode", if self.registers.mode_4 { "4" } else { "5" }),
                ("VRAM size", &self.registers.vram_size.to_string()),
            ],
        );

        callback(
            "Register #2",
            &[(
                "Plane A nametable address",
                &format!("${:04X}", self.registers.scroll_a_base_nt_addr),
            )],
        );

        callback(
            "Register #3",
            &[(
                "Window nametable address",
                &format!("${:04X}", self.registers.window_base_nt_addr),
            )],
        );

        callback(
            "Register #4",
            &[(
                "Plane B nametable address",
                &format!("${:04X}", self.registers.scroll_b_base_nt_addr),
            )],
        );

        callback(
            "Register #5",
            &[(
                "Sprite attribute table address",
                &format!("${:04X}", self.registers.sprite_attribute_table_base_addr),
            )],
        );

        callback(
            "Register #7",
            &[
                ("Backdrop palette", &self.registers.background_palette.to_string()),
                ("Backdrop color index", &self.registers.background_color_id.to_string()),
            ],
        );

        callback(
            "Register #10",
            &[("Horizontal interrupt interval", &self.registers.h_interrupt_interval.to_string())],
        );

        callback(
            "Register #11",
            &[
                ("Vertical scroll mode", &self.registers.vertical_scroll_mode.to_string()),
                ("Horizontal scroll mode", &self.registers.horizontal_scroll_mode.to_string()),
            ],
        );

        callback(
            "Register #12",
            &[
                ("Horizontal resolution", &self.registers.horizontal_display_size.to_string()),
                ("Shadow/highlight enabled", bool_str(self.registers.shadow_highlight_flag)),
                ("Screen mode", &self.registers.interlacing_mode.to_string()),
            ],
        );

        callback(
            "Register #13",
            &[(
                "H scroll table address",
                &format!("${:04X}", self.registers.h_scroll_table_base_addr),
            )],
        );

        callback(
            "Register #15",
            &[(
                "Data port auto-increment",
                &format!("0x{:X}", self.registers.data_port_auto_increment),
            )],
        );

        callback(
            "Register #16",
            &[
                ("Vertical plane size", &self.registers.vertical_scroll_size.to_string()),
                ("Horizontal plane size", &self.registers.horizontal_scroll_size.to_string()),
            ],
        );

        callback(
            "Register #17",
            &[
                ("Window horizontal mode", &self.registers.window_horizontal_mode.to_string()),
                ("Window X", &self.registers.window_x_position.to_string()),
            ],
        );

        callback(
            "Register #18",
            &[
                ("Window vertical mode", &self.registers.window_vertical_mode.to_string()),
                ("Window Y", &self.registers.window_y_position.to_string()),
            ],
        );

        callback("Registers #19-20", &[("DMA length", &self.registers.dma_length.to_string())]);

        callback(
            "Registers #21-23",
            &[
                ("DMA source address", &format!("${:06X}", self.registers.dma_source_address)),
                ("DMA mode", &self.registers.dma_mode.to_string()),
            ],
        );

        callback(
            "Debug Register",
            &[
                ("Display disabled", bool_str(self.debug_register.display_disabled)),
                ("Forced layer", &self.debug_register.forced_plane.to_string()),
            ],
        );
    }

    pub fn copy_sprite_attributes(
        &self,
        out: &mut [SpriteAttributeEntry],
    ) -> CopySpriteAttributesResult {
        let h_display_size = self.registers.horizontal_display_size;
        let base_sprite_table_addr = self.registers.sprite_attribute_table_base_addr
            & h_display_size.sprite_attribute_table_mask();
        let len = h_display_size.sprite_table_len();

        let interlacing_mode = self.registers.interlacing_mode;
        let y_mask = sprites::y_position_mask(interlacing_mode);

        for i in 0..len {
            let sprite_table_addr = base_sprite_table_addr.wrapping_add(8 * i) as usize;
            let data = SpriteData::create(
                self.cached_sprite_attributes[i as usize],
                &self.vram[sprite_table_addr + 4..sprite_table_addr + 8],
            );

            out[i as usize] = SpriteAttributeEntry {
                tile_number: data.pattern_generator,
                x: data.h_position,
                y: data.v_position & y_mask,
                h_cells: data.h_size_cells,
                v_cells: data.v_size_cells,
                palette: data.palette,
                priority: data.priority,
                h_flip: data.horizontal_flip,
                v_flip: data.vertical_flip,
                link: data.link_data,
            };
        }

        CopySpriteAttributesResult {
            sprite_table_len: len,
            top_left_x: sprites::SPRITE_H_DISPLAY_START,
            top_left_y: interlacing_mode.sprite_display_top(),
        }
    }

    pub fn debug_vram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.vram.as_mut_slice())
    }

    pub fn debug_cram_view(&mut self) -> impl DebugMemoryView {
        DebugWordsView(self.cram.as_mut_slice(), Endian::Big)
    }

    pub fn debug_vsram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.vsram.as_mut_slice())
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn parse_gen_color(gen_color: u16, modifier: ColorModifier, tables: &ColorTables) -> Color {
    let r = ((gen_color >> 1) & 0x07) as u8;
    let g = ((gen_color >> 5) & 0x07) as u8;
    let b = ((gen_color >> 9) & 0x07) as u8;
    colors::gen_to_rgba(r, g, b, 255, modifier, tables)
}
