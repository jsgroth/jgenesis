use crate::vdp::{ColorTables, Vdp, u16_to_rgb};
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::Color;

impl Vdp {
    pub fn debug_frame_buffer_view(&mut self, frame_buffer: usize) -> impl DebugMemoryView {
        let frame_buffer = match frame_buffer {
            0 => &mut self.frame_buffer_0,
            _ => &mut self.frame_buffer_1,
        };

        DebugWordsView(frame_buffer.as_mut_slice(), Endian::Big)
    }

    pub fn debug_palette_ram_view(&mut self) -> impl DebugMemoryView {
        DebugWordsView(self.cram.as_mut_slice(), Endian::Big)
    }

    pub fn copy_palette(&self, out: &mut [Color]) {
        let color_tables = ColorTables::from_tint(self.config.color_tint);

        for (i, color) in out[..256].iter_mut().enumerate() {
            let s32x_color = self.cram[i];
            *color = u16_to_rgb(s32x_color, color_tables);
        }
    }

    pub fn dump_registers(&self, mut callback: impl FnMut(&str, &[(&str, &str)])) {
        callback(
            "$4100 / $A15180",
            &[
                ("Mode", &self.registers.frame_buffer_mode.to_string()),
                ("Vertical resolution", &self.registers.v_resolution.to_string()),
                ("Invert priority", bool_str(self.registers.priority)),
            ],
        );

        callback(
            "$4102 / $A15182",
            &[("Shift screen left", bool_str(self.registers.screen_left_shift))],
        );

        callback(
            "$4104 / $A15184",
            &[("Auto fill length", &self.registers.auto_fill_length.to_string())],
        );

        callback(
            "$4106 / $A15186",
            &[(
                "Auto fill start address",
                &format!("${:05X}", u32::from(self.registers.auto_fill_start_address) << 1),
            )],
        );

        callback(
            "$4108 / $A15188",
            &[("Auto fill data", &format!("0x{:04X}", self.registers.auto_fill_data))],
        );

        callback(
            "$410A / $A1518A",
            &[("Display frame buffer", ["0", "1"][self.registers.display_frame_buffer as usize])],
        );
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}
