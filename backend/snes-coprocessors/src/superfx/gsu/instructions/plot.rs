use crate::superfx::gsu::instructions::{
    MemoryType, clear_prefix_flags, read_register, write_register,
};
use crate::superfx::gsu::{ClockSpeed, GraphicsSupportUnit, ScreenHeight};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, SignBit};
use std::cmp;

#[derive(Debug, Clone, Encode, Decode)]
struct PixelBuffer {
    pixels: [u8; 8],
    valid_bits: u8,
}

impl PixelBuffer {
    fn new() -> Self {
        Self { pixels: [0; 8], valid_bits: 0 }
    }

    fn write_pixel(&mut self, i: u8, color: u8) {
        self.pixels[i as usize] = color;
        self.valid_bits |= 1 << i;
    }

    fn is_valid(&self, i: u8) -> bool {
        self.valid_bits.bit(i)
    }

    fn any_valid(&self) -> bool {
        self.valid_bits != 0
    }

    fn all_valid(&self) -> bool {
        self.valid_bits == 0xFF
    }

    fn clear_valid(&mut self) {
        self.valid_bits = 0;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PlotState {
    pixel_buffer: PixelBuffer,
    // The secondary pixel buffer is not explicitly stored; implementation writes values to RAM
    // immediately when primary buffer is flushed
    last_coarse_x: u8,
    last_y: u8,
    flush_cycles_remaining: u8,
    just_flushed: bool,
}

impl PlotState {
    pub fn new() -> Self {
        Self {
            pixel_buffer: PixelBuffer::new(),
            last_coarse_x: 0,
            last_y: 0,
            flush_cycles_remaining: 0,
            just_flushed: false,
        }
    }

    pub fn tick(&mut self, gsu_cycles: u8) {
        if self.just_flushed {
            self.just_flushed = false;
        } else {
            self.flush_cycles_remaining = self.flush_cycles_remaining.saturating_sub(gsu_cycles);
        }
    }
}

pub(super) fn cmode(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // CMODE: Set POR (plot option register)
    let source = read_register(gsu, gsu.sreg);

    gsu.plot_transparent_pixels = source.bit(0);
    gsu.dither_on = source.bit(1);
    gsu.por_high_nibble_flag = source.bit(2);
    gsu.por_freeze_high_nibble = source.bit(3);
    gsu.force_obj_mode = source.bit(4);

    log::trace!("Plot transparent pixels: {}", gsu.plot_transparent_pixels);
    log::trace!("Dithering on: {}", gsu.dither_on);
    log::trace!("High nibble only in color writes: {}", gsu.por_high_nibble_flag);
    log::trace!("Freeze color high nibble: {}", gsu.por_freeze_high_nibble);
    log::trace!("Force OBJ mode: {}", gsu.force_obj_mode);

    clear_prefix_flags(gsu);
    memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn color(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // COLOR: Set color register
    let source = read_register(gsu, gsu.sreg);
    gsu.color = mask_color(source as u8, gsu);

    clear_prefix_flags(gsu);
    memory_type.access_cycles(gsu.clock_speed)
}

pub(super) fn getc(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit) -> u8 {
    // GETC: Get byte from ROM into color register
    let byte = gsu.state.rom_buffer;
    gsu.color = mask_color(byte, gsu);

    let cycles = gsu.state.rom_buffer_wait_cycles;
    gsu.state.rom_buffer_wait_cycles = 0;

    clear_prefix_flags(gsu);
    cycles + memory_type.access_cycles(gsu.clock_speed)
}

fn mask_color(mut new_color: u8, gsu: &GraphicsSupportUnit) -> u8 {
    if gsu.por_high_nibble_flag {
        // Replace low nibble with a copy of high nibble
        new_color = (new_color & 0xF0) | (new_color >> 4);
    }

    if gsu.por_freeze_high_nibble {
        // Copy high nibble from existing color register
        new_color = (new_color & 0x0F) | (gsu.color & 0xF0);
    }

    new_color
}

pub(super) fn plot(memory_type: MemoryType, gsu: &mut GraphicsSupportUnit, ram: &mut [u8]) -> u8 {
    // PLOT: Plot a pixel to the primary pixel buffer
    let x = gsu.r[1] as u8;
    let y = gsu.r[2] as u8;

    let mut cycles = 0;

    let coarse_x = x & !0x07;
    if (coarse_x != gsu.plot_state.last_coarse_x || y != gsu.plot_state.last_y)
        && gsu.plot_state.pixel_buffer.any_valid()
    {
        cycles += flush_pixel_buffer(gsu, ram);
    }

    gsu.plot_state.last_coarse_x = coarse_x;
    gsu.plot_state.last_y = y;

    let color = if gsu.dither_on && (x.bit(0) ^ y.bit(0)) { gsu.color >> 4 } else { gsu.color };
    let is_transparent = if gsu.por_freeze_high_nibble {
        // If high nibble is frozen, transparency check only looks at the lowest 2/4 bits even in
        // 256-color mode
        color & 0x0F & gsu.color_gradient.color_mask() == 0
    } else {
        color & gsu.color_gradient.color_mask() == 0
    };

    if gsu.plot_transparent_pixels || !is_transparent {
        let i = x & 0x07;
        gsu.plot_state.pixel_buffer.write_pixel(i, color);

        if gsu.plot_state.pixel_buffer.all_valid() {
            cycles += flush_pixel_buffer(gsu, ram);
        }
    }

    gsu.r[1] = gsu.r[1].wrapping_add(1);

    log::trace!("PLOT: x={x}, y={y}, color={color:02X}");

    clear_prefix_flags(gsu);
    cmp::max(memory_type.access_cycles(gsu.clock_speed), cycles)
}

pub(super) fn rpix(
    memory_type: MemoryType,
    gsu: &mut GraphicsSupportUnit,
    rom: &[u8],
    ram: &mut [u8],
) -> u8 {
    // RPIX: Read a pixel from RAM and flush both pixel buffers
    let bitplanes = gsu.color_gradient.bitplanes();
    let mut cycles = bitplanes as u8 * gsu.clock_speed.memory_access_cycles();
    if memory_type != MemoryType::CodeCache {
        cycles += 4;
    }

    if !gsu.plot_state.pixel_buffer.any_valid() || gsu.plot_state.pixel_buffer.all_valid() {
        cycles += match gsu.clock_speed {
            ClockSpeed::Slow => 7 * bitplanes as u8 - bitplanes as u8 / 2,
            ClockSpeed::Fast => 10 * bitplanes as u8,
        };
    }

    if gsu.plot_state.pixel_buffer.any_valid() {
        cycles += flush_pixel_buffer(gsu, ram);
    }

    cycles += gsu.plot_state.flush_cycles_remaining;
    gsu.plot_state.flush_cycles_remaining = 0;

    let x = gsu.r[1] as u8;
    let y = gsu.r[2] as u8;

    let tile_addr = compute_tile_addr(gsu, x, y, ram.len());
    let tile_size = gsu.color_gradient.tile_size();
    let tile_data = &ram[tile_addr..tile_addr + tile_size as usize];

    let row = y & 0x07;
    let line_base_addr: u32 = (row * 0x02).into();

    let pixel_idx = x & 0x07;
    let bitplane_idx = 7 - pixel_idx;

    let mut color = 0;
    for plane in (0..bitplanes).step_by(2) {
        let plane_addr = (line_base_addr + 8 * plane) as usize;

        color |= u8::from(tile_data[plane_addr].bit(bitplane_idx)) << plane;
        color |= u8::from(tile_data[plane_addr + 1].bit(bitplane_idx)) << (plane + 1);
    }

    cycles += write_register(gsu, gsu.dreg, color.into(), rom, ram);

    gsu.zero_flag = color == 0;
    gsu.sign_flag = color.sign_bit();

    clear_prefix_flags(gsu);
    cycles
}

#[must_use]
fn flush_pixel_buffer(gsu: &mut GraphicsSupportUnit, ram: &mut [u8]) -> u8 {
    let x = gsu.plot_state.last_coarse_x;
    let y = gsu.plot_state.last_y;

    let tile_addr = compute_tile_addr(gsu, x, y, ram.len());
    let tile_size = gsu.color_gradient.tile_size();

    let tile_data = &mut ram[tile_addr..tile_addr + tile_size as usize];

    let row = y & 0x07;
    let line_base_addr: u32 = (row * 0x02).into();

    log::trace!(
        "  Flushing pixel buffer; base={:05X}, x={x}, y={y}, tile_addr={tile_addr:04X}, line_addr={line_base_addr:02X}",
        gsu.screen_base
    );

    // Convert row of pixels from bitmap format to SNES bitplane format, only overwriting pixels that
    // have the valid flag set in the pixel buffer
    let bitplanes = gsu.color_gradient.bitplanes();
    for pixel_idx in 0..8 {
        if !gsu.plot_state.pixel_buffer.is_valid(pixel_idx) {
            continue;
        }

        let shift = 7 - pixel_idx;
        let color = gsu.plot_state.pixel_buffer.pixels[pixel_idx as usize];

        for plane in (0..bitplanes).step_by(2) {
            let plane_addr = (line_base_addr + 8 * plane) as usize;

            tile_data[plane_addr] = (tile_data[plane_addr] & !(1 << shift))
                | (u8::from(color.bit(plane as u8)) << shift);
            tile_data[plane_addr + 1] = (tile_data[plane_addr + 1] & !(1 << shift))
                | (u8::from(color.bit(plane as u8 + 1)) << shift);
        }
    }

    let cycles = gsu.plot_state.flush_cycles_remaining;

    let mut flush_cycles_required = gsu.clock_speed.memory_access_cycles() * bitplanes as u8;
    if !gsu.plot_state.pixel_buffer.all_valid() {
        // If not all 8 bit-pend flags are set, the chip needs to perform a read before each write
        flush_cycles_required *= 2;
    }

    gsu.plot_state.pixel_buffer.clear_valid();
    gsu.plot_state.flush_cycles_remaining = flush_cycles_required;
    gsu.plot_state.just_flushed = true;

    cycles
}

fn compute_tile_addr(gsu: &GraphicsSupportUnit, x: u8, y: u8, ram_len: usize) -> usize {
    let tile_x: u16 = (x / 8).into();
    let tile_y: u16 = (y / 8).into();

    let screen_height = if gsu.force_obj_mode { ScreenHeight::ObjMode } else { gsu.screen_height };

    let tile_number = match screen_height {
        ScreenHeight::Bg128Pixel => tile_x * 0x10 + tile_y,
        ScreenHeight::Bg160Pixel => tile_x * 0x14 + tile_y,
        ScreenHeight::Bg192Pixel => tile_x * 0x18 + tile_y,
        ScreenHeight::ObjMode => {
            let grid_offset = (u16::from(y.bit(7)) << 9) | (u16::from(x.bit(7)) << 8);
            let grid_x = tile_x & 0x0F;
            let grid_y = tile_y & 0x0F;
            grid_offset + grid_y * 0x10 + grid_x
        }
    };
    let tile_number: u32 = tile_number.into();

    let tile_size = gsu.color_gradient.tile_size();
    let tile_addr = gsu.screen_base + tile_number * tile_size;
    (tile_addr as usize) & (ram_len - 1)
}
