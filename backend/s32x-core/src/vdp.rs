//! 32X VDP (Video Display Processor)

mod registers;

use crate::registers::SystemRegisters;
use crate::vdp::registers::{FrameBufferMode, Registers, SelectedFrameBuffer};
use bincode::{Decode, Encode};
use genesis_config::S32XVideoOut;
use genesis_core::vdp::BorderSize;
use jgenesis_common::frontend::{Color, FrameSize, PixelAspectRatio, Renderer, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::cmp;
use std::ops::{Deref, DerefMut, Range};

const MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::MCLK_CYCLES_PER_SCANLINE;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::ACTIVE_MCLK_CYCLES_PER_SCANLINE;

// DRAM refresh takes ~40 SH-2 cycles
const DRAM_REFRESH_MCLK_CYCLES: Range<u64> =
    ACTIVE_MCLK_CYCLES_PER_SCANLINE..ACTIVE_MCLK_CYCLES_PER_SCANLINE + 40 * 7 / 3;

const FRAME_BUFFER_LEN_WORDS: usize = 128 * 1024 / 2;
const CRAM_LEN_WORDS: usize = 512 / 2;

// 32X VDP only supports 320x224 and 320x240 frame sizes
const FRAME_WIDTH: u32 = 320;
const V28_FRAME_HEIGHT: u32 = 224;
const V30_FRAME_HEIGHT: u32 = 240;

// The H32 frame buffer should be large enough to store frames as H1280px resolution (4 * 320)
const H32_FRAME_BUFFER_LEN: usize = genesis_core::vdp::FRAME_BUFFER_LEN * 4;

// Offset between the left edge of the Genesis H32 frame and the 32X frame, in H1280px pixels
//
// Due to HSYNC/blanking/border timings being slightly different between H32 and H40 mode, and due
// to the 32X VDP always assuming H40 mode, the Genesis and 32X frames are slightly offset when the
// Genesis VDP is in H32 mode.
const H32_H_OFFSET: u32 = 13;

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

type GenesisVdp = genesis_core::vdp::Vdp;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct H32FrameBuffer(Box<[Color; H32_FRAME_BUFFER_LEN]>);

impl Default for H32FrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); H32_FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Deref for H32FrameBuffer {
    type Target = [Color; H32_FRAME_BUFFER_LEN];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for H32FrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

type FrameBufferRam = [u16; FRAME_BUFFER_LEN_WORDS];
type Cram = [u16; CRAM_LEN_WORDS];

type RenderedFrame = [[u16; FRAME_WIDTH as usize]; V30_FRAME_HEIGHT as usize];

fn new_frame_buffer() -> Box<FrameBufferRam> {
    vec![0; FRAME_BUFFER_LEN_WORDS].into_boxed_slice().try_into().unwrap()
}

fn new_rendered_frame() -> Box<RenderedFrame> {
    vec![[0; FRAME_WIDTH as usize]; V30_FRAME_HEIGHT as usize]
        .into_boxed_slice()
        .try_into()
        .unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum WhichFrameBuffer {
    #[default]
    Genesis,
    H32,
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    next_render_buffer: WhichFrameBuffer,
    scanline: u16,
    scanline_mclk: u64,
    scanlines_in_current_frame: u16,
    h_interrupt_counter: u16,
    display_frame_buffer: SelectedFrameBuffer,
    // 7 * SH-2 cycles
    auto_fill_cycles_remaining: u64,
}

impl State {
    fn new() -> Self {
        Self {
            next_render_buffer: WhichFrameBuffer::Genesis,
            scanline: 0,
            scanline_mclk: 0,
            scanlines_in_current_frame: u16::MAX,
            h_interrupt_counter: 0,
            display_frame_buffer: SelectedFrameBuffer::default(),
            auto_fill_cycles_remaining: 0,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer_0: Box<FrameBufferRam>,
    frame_buffer_1: Box<FrameBufferRam>,
    rendered_frame: Box<RenderedFrame>,
    // 1280x224 or 1280x240 (not including borders)
    // Needed for when a game enables H32 mode on the Genesis side (NFL Quarterback Club does this)
    h32_frame_buffer: H32FrameBuffer,
    cram: Box<Cram>,
    registers: Registers,
    state: State,
    timing_mode: TimingMode,
    video_out: S32XVideoOut,
}

macro_rules! front_frame_buffer {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &$self.frame_buffer_0,
            SelectedFrameBuffer::One => &$self.frame_buffer_1,
        }
    };
}

macro_rules! back_frame_buffer {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &$self.frame_buffer_1,
            SelectedFrameBuffer::One => &$self.frame_buffer_0,
        }
    };
}

macro_rules! back_frame_buffer_mut {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &mut $self.frame_buffer_1,
            SelectedFrameBuffer::One => &mut $self.frame_buffer_0,
        }
    };
}

impl Vdp {
    pub fn new(timing_mode: TimingMode, video_out: S32XVideoOut) -> Self {
        Self {
            frame_buffer_0: new_frame_buffer(),
            frame_buffer_1: new_frame_buffer(),
            rendered_frame: new_rendered_frame(),
            h32_frame_buffer: H32FrameBuffer::default(),
            cram: vec![0; CRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            registers: Registers::default(),
            state: State::new(),
            timing_mode,
            video_out,
        }
    }

    pub fn tick(
        &mut self,
        mclk_cycles: u64,
        registers: &mut SystemRegisters,
        genesis_vdp: &GenesisVdp,
    ) {
        self.state.auto_fill_cycles_remaining =
            self.state.auto_fill_cycles_remaining.saturating_sub(mclk_cycles * 3);

        let prev_scanline_mclk = self.state.scanline_mclk;
        self.state.scanline_mclk += mclk_cycles;

        if self.state.scanline < self.registers.v_resolution.active_scanlines_per_frame()
            || self.registers.h_interrupt_in_vblank
        {
            if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE
                && self.state.scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
            {
                if self.state.h_interrupt_counter == 0 {
                    self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
                    registers.notify_h_interrupt();
                } else {
                    self.state.h_interrupt_counter -= 1;
                }
            }
        } else {
            self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
        }

        if self.state.scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            self.state.scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;
            self.state.scanline += 1;

            let active_lines_per_frame = self.registers.v_resolution.active_scanlines_per_frame();
            if self.state.scanline == active_lines_per_frame {
                // Beginning of VBlank; frame buffer switches take effect
                if log::log_enabled!(log::Level::Debug)
                    && self.state.display_frame_buffer != self.registers.display_frame_buffer
                {
                    log::debug!(
                        "VBlank: Changing front frame buffer to {:?}",
                        self.registers.display_frame_buffer
                    );
                }
                self.state.display_frame_buffer = self.registers.display_frame_buffer;
                registers.notify_vblank();

                // Grab scanlines in frame at start of VBlank to avoid a dependency on which order
                // the VDPs execute in, since interlacing state is latched at the start of line 0
                self.state.scanlines_in_current_frame = genesis_vdp.scanlines_in_current_frame();
            } else if self.state.scanline >= self.state.scanlines_in_current_frame {
                self.state.scanline = 0;
            }

            if self.state.scanline < active_lines_per_frame {
                self.render_line();
            }
        }
    }

    pub fn mclk_cycles_until_next_event(&self, h_interrupt_enabled: bool) -> u64 {
        let cycles_till_line_end = MCLK_CYCLES_PER_SCANLINE - self.state.scanline_mclk;

        if h_interrupt_enabled
            && self.state.h_interrupt_counter == 0
            && self.state.scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE
        {
            return cmp::min(
                cycles_till_line_end,
                ACTIVE_MCLK_CYCLES_PER_SCANLINE - self.state.scanline_mclk,
            );
        }

        cycles_till_line_end
    }

    fn render_line(&mut self) {
        match self.registers.frame_buffer_mode {
            FrameBufferMode::Blank => {
                self.rendered_frame[self.state.scanline as usize].fill(0);
            }
            FrameBufferMode::PackedPixel => self.render_packed_pixel(),
            FrameBufferMode::DirectColor => self.render_direct_color(),
            FrameBufferMode::RunLength => self.render_run_length(),
        }
    }

    fn render_packed_pixel(&mut self) {
        let line = self.state.scanline as usize;
        let frame_buffer = front_frame_buffer!(self);
        let line_addr = frame_buffer[line];

        log::trace!(
            "Rendering line {line} from buffer {:?} in packed pixel mode, addr={line_addr:04X}",
            self.state.display_frame_buffer
        );

        let priority = u16::from(self.registers.priority) << 15;

        if self.registers.screen_left_shift {
            for pixel in 0..FRAME_WIDTH as u16 {
                let frame_buffer_addr = line_addr.wrapping_add((pixel + 1) >> 1);
                let color = (frame_buffer[frame_buffer_addr as usize] >> (8 * (pixel & 1))) & 0xFF;

                self.rendered_frame[line][pixel as usize] = self.cram[color as usize] ^ priority;
            }
        } else {
            for pixel in (0..FRAME_WIDTH as u16).step_by(2) {
                let frame_buffer_addr = line_addr.wrapping_add(pixel >> 1);
                let [msb, lsb] = frame_buffer[frame_buffer_addr as usize].to_be_bytes();

                self.rendered_frame[line][pixel as usize] = self.cram[msb as usize] ^ priority;
                self.rendered_frame[line][(pixel + 1) as usize] =
                    self.cram[lsb as usize] ^ priority;
            }
        }
    }

    fn render_direct_color(&mut self) {
        let line = self.state.scanline as usize;
        let frame_buffer = front_frame_buffer!(self);
        let line_addr = frame_buffer[line];

        log::trace!(
            "Rendering line {line} from frame buffer {:?} in direct color mode, addr={line_addr:04X}",
            self.state.display_frame_buffer
        );

        let priority = u16::from(self.registers.priority) << 15;

        for pixel in 0..FRAME_WIDTH as u16 {
            let color = frame_buffer[line_addr.wrapping_add(pixel) as usize];
            self.rendered_frame[line][pixel as usize] = color ^ priority;
        }
    }

    fn render_run_length(&mut self) {
        let line = self.state.scanline as usize;
        let frame_buffer = front_frame_buffer!(self);
        let mut line_addr = frame_buffer[line];

        log::trace!(
            "Rendering line {line} from buffer {:?} in run length mode, addr={line_addr:04X}",
            self.state.display_frame_buffer
        );

        let priority = u16::from(self.registers.priority) << 15;

        let mut pixel = 0;
        while pixel < FRAME_WIDTH {
            let [run_length_byte, color_idx] = frame_buffer[line_addr as usize].to_be_bytes();
            line_addr = line_addr.wrapping_add(1);

            let color = self.cram[color_idx as usize];
            let mut run_length = u16::from(run_length_byte) + 1;
            while pixel < FRAME_WIDTH && run_length != 0 {
                self.rendered_frame[line][pixel as usize] = color ^ priority;
                pixel += 1;
                run_length -= 1;
            }
        }
    }

    pub fn read_register(&self, address: u32) -> u16 {
        match address & 0xF {
            0x0 => self.registers.read_display_mode(self.timing_mode),
            0x2 => self.registers.read_screen_shift(),
            0x4 => self.registers.read_auto_fill_length(),
            0x6 => self.registers.read_auto_fill_start_address(),
            0xA => self.read_frame_buffer_control(),
            _ => {
                log::warn!("Invalid VDP register read {address:08X}");
                0
            }
        }
    }

    pub fn write_register_byte(&mut self, address: u32, value: u8) {
        let mut word = self.read_register(address & !1);
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
        self.write_register(address & !1, word);
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        log::trace!(
            "VDP register write on line {}: {address:08X} {value:04X}",
            self.state.scanline
        );

        match address & 0xF {
            0x0 => self.registers.write_display_mode(value),
            0x2 => self.registers.write_screen_shift(value),
            0x4 => self.registers.write_auto_fill_length(value),
            0x6 => self.registers.write_auto_fill_start_address(value),
            0x8 => {
                self.registers.write_auto_fill_data(value);

                // Writing auto fill data initiates auto fill
                self.do_auto_fill();
            }
            0xA => {
                self.registers.write_frame_buffer_control(value);
                if self.in_vblank() || self.registers.frame_buffer_mode == FrameBufferMode::Blank {
                    self.state.display_frame_buffer = self.registers.display_frame_buffer;
                    log::debug!("Front frame buffer set to {:?}", self.state.display_frame_buffer);
                }
            }
            0xC | 0xE => {
                log::warn!("Invalid VDP register write {address:08X} {value:04X}");
            }
            _ => panic!("VDP register write to an unaligned address: {address:08X} {value:04X}"),
        }
    }

    pub fn read_frame_buffer(&self, address: u32) -> u16 {
        let frame_buffer = back_frame_buffer!(self);
        frame_buffer[((address & 0x1FFFF) >> 1) as usize]
    }

    pub fn write_frame_buffer_byte(&mut self, address: u32, value: u8) {
        log::trace!(
            "Frame buffer byte write {:05X} {value:02X} (word addr {:04X})",
            address & 0x1FFFF,
            (address >> 1) & 0xFFFF
        );

        if value == 0 {
            return;
        }

        let frame_buffer = back_frame_buffer_mut!(self);
        let frame_buffer_addr = ((address & 0x1FFFF) >> 1) as usize;

        if !address.bit(0) {
            frame_buffer[frame_buffer_addr].set_msb(value);
        } else {
            frame_buffer[frame_buffer_addr].set_lsb(value);
        }
    }

    pub fn write_frame_buffer_word(&mut self, address: u32, value: u16) {
        log::trace!(
            "Frame buffer write {:05X} {value:04X} (word addr {:04X})",
            address & 0x1FFFF,
            (address >> 1) & 0xFFFF
        );

        let frame_buffer = back_frame_buffer_mut!(self);
        frame_buffer[((address & 0x1FFFF) >> 1) as usize] = value;
    }

    pub fn frame_buffer_overwrite_word(&mut self, address: u32, value: u16) {
        log::trace!(
            "Overwrite image write {:05X} {value:04X} (word addr {:04X})",
            address & 0x1FFFF,
            (address >> 1) & 0xFFFF
        );

        if value == 0 {
            return;
        }

        let frame_buffer = back_frame_buffer_mut!(self);
        let frame_buffer_addr = ((address & 0x1FFFF) >> 1) as usize;

        let [msb, lsb] = value.to_be_bytes();

        if msb != 0 {
            frame_buffer[frame_buffer_addr].set_msb(msb);
        }

        if lsb != 0 {
            frame_buffer[frame_buffer_addr].set_lsb(lsb);
        }
    }

    pub fn read_cram(&self, address: u32) -> u16 {
        // TODO block access to CRAM when in use?
        self.cram[((address & 0x1FF) >> 1) as usize]
    }

    pub fn write_cram_byte(&mut self, address: u32, value: u8) {
        let mut word = self.read_cram(address & !1);
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
        self.write_cram(address, word);
    }

    pub fn write_cram(&mut self, address: u32, value: u16) {
        // TODO block access to CRAM while in use?
        self.cram[((address & 0x1FF) >> 1) as usize] = value;
    }

    // Interrupt mask bit 7: HEN (H interrupts enabled during VBlank)
    pub fn hen_bit(&self) -> bool {
        self.registers.h_interrupt_in_vblank
    }

    // Interrupt mask bit 7: HEN (H interrupts enabled during VBlank)
    pub fn write_hen_bit(&mut self, hen: bool) {
        self.registers.h_interrupt_in_vblank = hen;
    }

    // SH-2: $4004
    pub fn h_interrupt_interval(&self) -> u16 {
        self.registers.h_interrupt_interval
    }

    // SH-2: $4004
    pub fn write_h_interrupt_interval(&mut self, value: u16) {
        self.registers.h_interrupt_interval = value & 0xFF;
        log::trace!("H interrupt interval write: {value:04X}");
    }

    // 68000: $A1518A
    // SH-2: $410A
    fn read_frame_buffer_control(&self) -> u16 {
        let in_vblank = self.in_vblank();
        let in_hblank = self.in_hblank();

        let cram_accessible = in_vblank || in_hblank;

        // Metal Head depends on the FEN bit reading 1 during DRAM refresh (beginning of HBlank every line)
        // or else it will freeze after the Sega splash screen
        let frame_buffer_busy = self.state.auto_fill_cycles_remaining != 0
            || DRAM_REFRESH_MCLK_CYCLES.contains(&self.state.scanline_mclk);

        (u16::from(in_vblank) << 15)
            | (u16::from(in_hblank) << 14)
            | (u16::from(cram_accessible) << 13)
            | (u16::from(frame_buffer_busy) << 1)
            | (self.state.display_frame_buffer as u16)
    }

    fn do_auto_fill(&mut self) {
        let frame_buffer = back_frame_buffer_mut!(self);

        let data = self.registers.auto_fill_data;
        for _ in 0..self.registers.auto_fill_length {
            frame_buffer[self.registers.auto_fill_start_address as usize] = data;
            self.registers.increment_auto_fill_address();
        }

        // Note: Auto fill finishing too quickly will cause major glitches in Mortal Kombat II.
        //
        // At VINT, the master SH-2 immediately starts drawing the next frame without syncing with
        // the 68000. It first zeroes out the frame buffer using auto fills and then starts drawing
        // sprites and the HUD.
        //
        // If the auto fills finish before the 68000 interrupts the master SH-2 to send updated I/O
        // data and then the SH-2 sees that user inputs have changed, it will get very confused and
        // draw a single glitched frame that was partly drawn using the previous inputs and partly
        // drawn using the new inputs. Depending on emulated SH-2 speed, this can also cause
        // gameplay glitches as the game may briefly stop responding to user inputs.
        self.state.auto_fill_cycles_remaining =
            7 * (7 * u64::from(self.registers.auto_fill_length) / 3);
    }

    fn in_vblank(&self) -> bool {
        self.state.scanline >= self.registers.v_resolution.active_scanlines_per_frame()
    }

    fn in_hblank(&self) -> bool {
        self.state.scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
    }

    pub fn scanline(&self) -> u16 {
        self.state.scanline
    }

    pub fn scanline_mclk(&self) -> u64 {
        self.state.scanline_mclk
    }

    pub fn composite_frame(&mut self, genesis_vdp: &mut GenesisVdp) {
        // Default to rendering from the Genesis VDP frame buffer, switch later if necessary
        self.state.next_render_buffer = WhichFrameBuffer::Genesis;

        if (self.video_out != S32XVideoOut::S32XOnly
            && self.registers.frame_buffer_mode == FrameBufferMode::Blank)
            || self.video_out == S32XVideoOut::GenesisOnly
        {
            // Leave Genesis frame as-is
            // TODO what if 32X VDP was switched to blank mode mid-frame?
            return;
        }

        let genesis_frame_size = genesis_vdp.frame_size();
        let border_size = genesis_vdp.border_size();

        if genesis_frame_size.width - border_size.left - border_size.right == 256 {
            // If the Genesis VDP is in H32 mode, render the frame in 1280x224 so that it's possible
            // to composite the 320x224 32X frame with the 256x224 Genesis frame without needing to
            // blend or filter any pixels.
            // NFL Quarterback Club depends on this - it uses H32 mode in menus
            self.composite_frame_h32(genesis_frame_size, border_size, genesis_vdp);
            return;
        }

        // Otherwise, composite in H40 into the Genesis VDP frame buffer
        self.composite_frame_inner::<false>(genesis_frame_size, border_size, genesis_vdp);
    }

    fn composite_frame_h32(
        &mut self,
        genesis_frame_size: FrameSize,
        border_size: BorderSize,
        genesis_vdp: &mut GenesisVdp,
    ) {
        debug_assert!(genesis_frame_size.width == 256 + border_size.left + border_size.right);

        self.state.next_render_buffer = WhichFrameBuffer::H32;

        if self.video_out != S32XVideoOut::S32XOnly {
            self.copy_genesis_frame_buffer_to_h32(
                genesis_frame_size,
                border_size,
                genesis_vdp.frame_buffer_mut(),
            );
        } else {
            self.h32_frame_buffer.fill(Color::rgba(0, 0, 0, 0));
        }

        self.composite_frame_inner::<true>(genesis_frame_size, border_size, genesis_vdp);
    }

    fn composite_frame_inner<const H32: bool>(
        &mut self,
        frame_size: FrameSize,
        border_size: BorderSize,
        genesis_vdp: &mut GenesisVdp,
    ) {
        fn should_use_32x_pixel(s32x_only: bool, s32x_pixel: u16, gen_color: Color) -> bool {
            s32x_only || s32x_pixel.bit(15) || gen_color.a == 0
        }

        let interlaced_frame: u32 = genesis_vdp.is_interlaced_frame().into();
        let interlaced_odd: u32 = genesis_vdp.is_interlaced_odd().into();

        let frame_buffer =
            if H32 { self.h32_frame_buffer.as_mut() } else { genesis_vdp.frame_buffer_mut() };

        let active_lines_per_frame: u32 =
            self.registers.v_resolution.active_scanlines_per_frame().into();
        let s32x_only = self.video_out == S32XVideoOut::S32XOnly;

        let frame_width = if H32 {
            determine_h32_buffer_width(frame_size, border_size)
        } else {
            frame_size.width
        };
        let left_offset = if H32 { 5 * border_size.left + H32_H_OFFSET } else { border_size.left };

        let top_offset = border_size.top << interlaced_frame;

        for line in 0..active_lines_per_frame {
            let effective_line = (line << interlaced_frame) + interlaced_odd;
            let fb_row_addr = ((effective_line + top_offset) * frame_width + left_offset) as usize;

            for pixel in 0..FRAME_WIDTH {
                let s32x_pixel = self.rendered_frame[line as usize][pixel as usize];

                if H32 {
                    let fb_addr = fb_row_addr + 4 * pixel as usize;
                    for i in 0..4 {
                        if should_use_32x_pixel(s32x_only, s32x_pixel, frame_buffer[fb_addr + i]) {
                            frame_buffer[fb_addr + i] = u16_to_rgb(s32x_pixel);
                        }
                    }
                } else {
                    let fb_addr = fb_row_addr + pixel as usize;
                    if should_use_32x_pixel(s32x_only, s32x_pixel, frame_buffer[fb_addr]) {
                        frame_buffer[fb_addr] = u16_to_rgb(s32x_pixel);
                    }
                }
            }
        }

        // TODO how do interlaced modes actually work with 32X? Needs testing
        if interlaced_frame != 0 {
            for line in 0..active_lines_per_frame {
                let from_line = 2 * line + interlaced_odd;
                let to_line = from_line ^ 1;

                let from_line_fb_addr = ((from_line + top_offset) * frame_width) as usize;
                let to_line_fb_addr = ((to_line + top_offset) * frame_width) as usize;

                if to_line_fb_addr > from_line_fb_addr {
                    let (a, b) = frame_buffer.split_at_mut(to_line_fb_addr);
                    b[..frame_width as usize].copy_from_slice(
                        &a[from_line_fb_addr..from_line_fb_addr + frame_width as usize],
                    );
                } else {
                    let (a, b) = frame_buffer.split_at_mut(from_line_fb_addr);
                    a[to_line_fb_addr..to_line_fb_addr + frame_width as usize]
                        .copy_from_slice(&b[..frame_width as usize]);
                }
            }
        }
    }

    fn copy_genesis_frame_buffer_to_h32(
        &mut self,
        frame_size: FrameSize,
        border_size: BorderSize,
        frame_buffer: &mut [Color; genesis_core::vdp::FRAME_BUFFER_LEN],
    ) {
        let h32_frame_width = determine_h32_buffer_width(frame_size, border_size);

        // Expand the frame buffer from 256x224 to 1280x224 (plus borders)
        for line in 0..frame_size.height {
            let h32_fb_line_addr = line * h32_frame_width;
            for pixel in 0..frame_size.width {
                let genesis_fb_addr = (line * frame_size.width + pixel) as usize;
                let h32_fb_addr = (h32_fb_line_addr + 5 * pixel) as usize;
                self.h32_frame_buffer[h32_fb_addr..h32_fb_addr + 5]
                    .fill(frame_buffer[genesis_fb_addr]);
            }

            if 5 * frame_size.width != h32_frame_width {
                // Clear right border pixels so 32X pixels will always display over them
                self.h32_frame_buffer[(h32_fb_line_addr + 5 * frame_size.width) as usize
                    ..(h32_fb_line_addr + h32_frame_width) as usize]
                    .fill(Color::rgba(0, 0, 0, 0));
            }
        }
    }

    pub fn render_frame<R: Renderer>(
        &self,
        genesis_vdp: &GenesisVdp,
        mut aspect_ratio: Option<PixelAspectRatio>,
        renderer: &mut R,
    ) -> Result<(), R::Err> {
        if self.state.next_render_buffer == WhichFrameBuffer::Genesis {
            return renderer.render_frame(
                genesis_vdp.frame_buffer(),
                genesis_vdp.frame_size(),
                aspect_ratio,
            );
        }

        let mut frame_size = genesis_vdp.frame_size();
        frame_size.width = determine_h32_buffer_width(frame_size, genesis_vdp.border_size());

        aspect_ratio =
            aspect_ratio.map(|par| PixelAspectRatio::try_from(0.2 * f64::from(par)).unwrap());

        renderer.render_frame(self.h32_frame_buffer.as_ref(), frame_size, aspect_ratio)
    }

    pub fn update_video_out(&mut self, video_out: S32XVideoOut) {
        self.video_out = video_out;
    }
}

fn u16_to_rgb(s32x_pixel: u16) -> Color {
    let r = s32x_pixel & 0x1F;
    let g = (s32x_pixel >> 5) & 0x1F;
    let b = (s32x_pixel >> 10) & 0x1F;

    Color::rgb(RGB_5_TO_8[r as usize], RGB_5_TO_8[g as usize], RGB_5_TO_8[b as usize])
}

fn determine_h32_buffer_width(frame_size: FrameSize, border_size: BorderSize) -> u32 {
    if border_size.right < H32_H_OFFSET.div_ceil(5) {
        5 * frame_size.width + H32_H_OFFSET
    } else {
        5 * frame_size.width
    }
}
