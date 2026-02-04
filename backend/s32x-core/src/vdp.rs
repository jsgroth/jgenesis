//! 32X VDP (Video Display Processor)

mod debug;
mod registers;

use crate::api::Sega32XEmulatorConfig;
use crate::registers::SystemRegisters;
use crate::vdp::registers::{FrameBufferMode, Registers, SelectedFrameBuffer};
use bincode::{Decode, Encode};
use genesis_config::{S32XColorTint, S32XVideoOut, S32XVoidColor};
use genesis_core::vdp::BorderSize;
use jgenesis_common::frontend::{
    Color, FiniteF64, FrameSize, RenderFrameOptions, Renderer, TimingMode,
};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::cmp;
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut, Range};

const MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::MCLK_CYCLES_PER_SCANLINE;

// 13 pixels after Genesis H40 HBlank start
const HBLANK_START_MCLK_CYCLES: u64 = 343 * 8;
const HBLANK_END_MCLK_CYCLES: u64 = HBLANK_START_MCLK_CYCLES - 320 * 8;

// Very slightly after actual hardware starts rendering the line
const RENDER_LINE_MCLK_CYCLES: u64 = 26 * 8;

// DRAM refresh takes ~40 SH-2 cycles
const DRAM_REFRESH_MCLK_CYCLES: Range<u64> =
    HBLANK_START_MCLK_CYCLES..HBLANK_START_MCLK_CYCLES + 40 * 7 / 3;

const FRAME_BUFFER_LEN_WORDS: usize = 128 * 1024 / 2;
const CRAM_LEN_WORDS: usize = 512 / 2;

// 32X VDP only supports 320x224 and 320x240 frame sizes
const FRAME_WIDTH: u32 = 320;
const V28_FRAME_HEIGHT: u32 = 224;
const V30_FRAME_HEIGHT: u32 = 240;

// The H32 frame buffer should be large enough to store frames as H1280px resolution (4 * 320)
const EXPANDED_FRAME_BUFFER_LEN: usize = genesis_core::vdp::FRAME_BUFFER_LEN * 4;

// Offset between the left edge of the Genesis H32 frame and the 32X frame, in H1280px pixels
//
// Due to HSYNC/blanking/border timings being slightly different between H32 and H40 mode, and due
// to the 32X VDP always assuming H40 mode, the Genesis and 32X frames are slightly offset when the
// Genesis VDP is in H32 mode.
const H32_H_OFFSET: u32 = 13;

type GenesisVdp = genesis_core::vdp::Vdp;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct ExpandedFrameBuffer(Box<[Color; EXPANDED_FRAME_BUFFER_LEN]>);

impl Default for ExpandedFrameBuffer {
    fn default() -> Self {
        Self(
            vec![Color::default(); EXPANDED_FRAME_BUFFER_LEN]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        )
    }
}

impl Deref for ExpandedFrameBuffer {
    type Target = [Color; EXPANDED_FRAME_BUFFER_LEN];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ExpandedFrameBuffer {
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
    ExpandedH32,
    ExpandedH40,
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    next_render_buffer: WhichFrameBuffer,
    scanline: u16,
    scanline_mclk: u64,
    scanlines_in_current_frame: u16,
    h_interrupt_this_line: bool,
    h_interrupt_counter: u16,
    display_frame_buffer: SelectedFrameBuffer,
    auto_fill_mclk_remaining: u64,
    fb_write_timing_fifo: VecDeque<u64>,
    last_fb_write_cycles: u64,
    cycles_till_next_render: u64,
}

impl State {
    fn new() -> Self {
        Self {
            next_render_buffer: WhichFrameBuffer::Genesis,
            scanline: 0,
            scanline_mclk: 0,
            scanlines_in_current_frame: u16::MAX,
            h_interrupt_this_line: true,
            h_interrupt_counter: 0,
            display_frame_buffer: SelectedFrameBuffer::default(),
            auto_fill_mclk_remaining: 0,
            fb_write_timing_fifo: VecDeque::with_capacity(4),
            last_fb_write_cycles: 0,
            cycles_till_next_render: u64::MAX,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VdpConfig {
    video_out: S32XVideoOut,
    color_tint: S32XColorTint,
    show_high_priority: bool,
    show_low_priority: bool,
    void_color: S32XVoidColor,
    emulate_pixel_switch_delay: bool,
}

impl VdpConfig {
    fn from_emu_config(config: &Sega32XEmulatorConfig) -> Self {
        Self {
            video_out: config.video_out,
            color_tint: config.color_tint,
            show_high_priority: config.show_high_priority,
            show_low_priority: config.show_low_priority,
            void_color: config.void_color,
            emulate_pixel_switch_delay: config.emulate_pixel_switch_delay,
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
    expanded_frame_buffer: ExpandedFrameBuffer,
    cram: Box<Cram>,
    registers: Registers,
    // Per documentation, the VDP latches registers for rendering once per line beginning shortly
    // after the start of HBlank
    latched: Registers,
    state: State,
    timing_mode: TimingMode,
    config: VdpConfig,
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
    pub fn new(timing_mode: TimingMode, config: &Sega32XEmulatorConfig) -> Self {
        Self {
            frame_buffer_0: new_frame_buffer(),
            frame_buffer_1: new_frame_buffer(),
            rendered_frame: new_rendered_frame(),
            expanded_frame_buffer: ExpandedFrameBuffer::default(),
            cram: vec![0; CRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            registers: Registers::default(),
            latched: Registers::default(),
            state: State::new(),
            timing_mode,
            config: VdpConfig::from_emu_config(config),
        }
    }

    pub fn tick(
        &mut self,
        mclk_cycles: u64,
        registers: &mut SystemRegisters,
        genesis_vdp: &GenesisVdp,
    ) {
        self.state.auto_fill_mclk_remaining =
            self.state.auto_fill_mclk_remaining.saturating_sub(mclk_cycles);

        let prev_scanline_mclk = self.state.scanline_mclk;
        self.state.scanline_mclk += mclk_cycles;

        if self.state.h_interrupt_this_line || self.registers.h_interrupt_in_vblank {
            if prev_scanline_mclk < HBLANK_START_MCLK_CYCLES
                && self.state.scanline_mclk >= HBLANK_START_MCLK_CYCLES
            {
                self.handle_hblank_start(registers);
            }
        } else {
            self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
        }

        let active_lines_per_frame = self.latched.v_resolution.active_scanlines_per_frame();
        if self.state.scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            self.state.scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;
            self.state.scanline += 1;

            if self.state.scanline == active_lines_per_frame {
                // Beginning of VBlank; frame buffer switches take effect
                if self.state.display_frame_buffer != self.registers.display_frame_buffer {
                    log::debug!(
                        "VBlank: Changing front frame buffer to {:?}",
                        self.registers.display_frame_buffer
                    );
                }
                self.state.display_frame_buffer = self.registers.display_frame_buffer;
                registers.notify_vblank_start();

                // Grab scanlines in frame at start of VBlank to avoid a dependency on which order
                // the VDPs execute in, since interlacing state is latched at the start of line 0
                self.state.scanlines_in_current_frame = genesis_vdp.scanlines_in_current_frame();

                // No HINTs during VBlank (unless the HEN bit is set)
                self.state.h_interrupt_this_line = false;
            } else if self.state.scanline >= self.state.scanlines_in_current_frame {
                self.state.scanline = 0;
                registers.notify_vblank_end();
            } else if self.state.scanline == self.state.scanlines_in_current_frame - 1 {
                // First HINT before rendering line 0
                self.state.h_interrupt_this_line = true;
            }

            if self.state.scanline < active_lines_per_frame
                && self.state.scanline_mclk >= RENDER_LINE_MCLK_CYCLES
            {
                self.render_line();
                self.state.cycles_till_next_render =
                    MCLK_CYCLES_PER_SCANLINE + RENDER_LINE_MCLK_CYCLES - self.state.scanline_mclk;
            }
        } else if prev_scanline_mclk < RENDER_LINE_MCLK_CYCLES
            && self.state.scanline_mclk >= RENDER_LINE_MCLK_CYCLES
        {
            if self.state.scanline < active_lines_per_frame {
                self.render_line();
            }
            self.state.cycles_till_next_render =
                MCLK_CYCLES_PER_SCANLINE + RENDER_LINE_MCLK_CYCLES - self.state.scanline_mclk;
        }
    }

    fn handle_hblank_start(&mut self, registers: &mut SystemRegisters) {
        // Latch registers for rendering next line
        self.latched = self.registers.clone();

        // In case VDP was switched to blank mode with a pending frame buffer swap
        if self.latched.frame_buffer_mode == FrameBufferMode::Blank {
            if self.state.display_frame_buffer != self.registers.display_frame_buffer {
                log::debug!("Front frame buffer set to {:?}", self.state.display_frame_buffer);
            }
            self.state.display_frame_buffer = self.registers.display_frame_buffer;
        }

        if self.state.h_interrupt_counter == 0 {
            self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
            registers.notify_h_interrupt();
        } else {
            self.state.h_interrupt_counter -= 1;
        }
    }

    pub fn mclk_cycles_until_next_event(&self, h_interrupt_enabled: bool) -> u64 {
        // Sync at every line render
        let mut cycles_till_next = self.state.cycles_till_next_render;

        // Sync at HINT if HINT will trigger on this line
        if h_interrupt_enabled
            && self.state.h_interrupt_counter == 0
            && self.state.scanline_mclk < HBLANK_START_MCLK_CYCLES
        {
            cycles_till_next =
                cmp::min(cycles_till_next, HBLANK_START_MCLK_CYCLES - self.state.scanline_mclk);
        }

        // Sync at auto fill end if an auto fill is in progress
        if self.state.auto_fill_mclk_remaining != 0 {
            cycles_till_next = cmp::min(cycles_till_next, self.state.auto_fill_mclk_remaining);
        }

        cycles_till_next
    }

    fn render_line(&mut self) {
        match self.latched.frame_buffer_mode {
            FrameBufferMode::Blank => {
                self.rendered_frame[self.state.scanline as usize].fill(0);
            }
            FrameBufferMode::PackedPixel => self.render_packed_pixel(),
            FrameBufferMode::DirectColor => self.render_direct_color(),
            FrameBufferMode::RunLength => self.render_run_length(),
        }
    }

    fn priority_mask_fn(&self) -> impl Fn(u16) -> u16 + 'static {
        let vdp_priority = u16::from(self.latched.priority) << 15;
        let show_high_priority = self.config.show_high_priority;
        let show_low_priority = self.config.show_low_priority;
        let void_color = match self.config.void_color {
            S32XVoidColor::PaletteRam { idx } => self.cram[idx as usize],
            S32XVoidColor::Direct { r, g, b, a } => {
                u16::from(r & 0x1F)
                    | (u16::from(g & 0x1F) << 5)
                    | (u16::from(b & 0x1F) << 10)
                    | (u16::from(a) << 15)
            }
        };

        move |mut pixel| {
            let pixel_priority = pixel.bit(15);
            if (pixel_priority && !show_high_priority) || (!pixel_priority && !show_low_priority) {
                pixel = void_color;
            }
            pixel ^ vdp_priority
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

        let mask_fn = self.priority_mask_fn();

        if self.latched.screen_left_shift {
            for pixel in 0..FRAME_WIDTH as u16 {
                let frame_buffer_addr = line_addr.wrapping_add((pixel + 1) >> 1);
                let color = (frame_buffer[frame_buffer_addr as usize] >> (8 * (pixel & 1))) & 0xFF;

                self.rendered_frame[line][pixel as usize] = mask_fn(self.cram[color as usize]);
            }
        } else {
            for pixel in (0..FRAME_WIDTH as u16).step_by(2) {
                let frame_buffer_addr = line_addr.wrapping_add(pixel >> 1);
                let [msb, lsb] = frame_buffer[frame_buffer_addr as usize].to_be_bytes();

                self.rendered_frame[line][pixel as usize] = mask_fn(self.cram[msb as usize]);
                self.rendered_frame[line][(pixel + 1) as usize] = mask_fn(self.cram[lsb as usize]);
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

        let mask_fn = self.priority_mask_fn();

        for pixel in 0..FRAME_WIDTH as u16 {
            let color = frame_buffer[line_addr.wrapping_add(pixel) as usize];
            self.rendered_frame[line][pixel as usize] = mask_fn(color);
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

        let mask_fn = self.priority_mask_fn();

        let mut pixel = 0;
        while pixel < FRAME_WIDTH {
            let [run_length_byte, color_idx] = frame_buffer[line_addr as usize].to_be_bytes();
            line_addr = line_addr.wrapping_add(1);

            let color = self.cram[color_idx as usize];
            let mut run_length = u16::from(run_length_byte) + 1;
            while pixel < FRAME_WIDTH && run_length != 0 {
                self.rendered_frame[line][pixel as usize] = mask_fn(color);
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
                if self.in_vblank() || self.latched.frame_buffer_mode == FrameBufferMode::Blank {
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

    #[must_use]
    pub fn frame_buffer_write_latency(&mut self, cycles: u64) -> u64 {
        if cycles <= self.state.last_fb_write_cycles {
            // Can happen if both SH-2s are writing to the frame buffer simultaneously
            // Just ignore and return minimum latency
            return 1;
        }

        // Progress times in FIFO
        let cycle_diff = cycles - self.state.last_fb_write_cycles;
        for _ in 0..cycle_diff {
            let Some(front) = self.state.fb_write_timing_fifo.front_mut() else { break };

            *front -= 1;
            if *front == 0 {
                self.state.fb_write_timing_fifo.pop_front();
            }
        }

        // VDP can only accept 1 write every 4 cycles
        let initial_write_time = if cycle_diff < 3 { 3 - (cycle_diff - 1) } else { 1 };

        // If the 4-entry FIFO is full, must wait for an empty slot
        let fifo_wait_time = if self.state.fb_write_timing_fifo.len() == 4 {
            1 + self.state.fb_write_timing_fifo.pop_front().unwrap()
        } else {
            0
        };

        let wait_cycles = cmp::max(initial_write_time, fifo_wait_time);
        debug_assert!((1..=5).contains(&wait_cycles));

        self.state.fb_write_timing_fifo.push_back(5);
        self.state.last_fb_write_cycles = cycles + wait_cycles - 1;

        wait_cycles
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
        let frame_buffer_busy = self.state.auto_fill_mclk_remaining != 0
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
        //
        // Official documentation appears to say that auto fill takes (7 + 3 * length) Sclk cycles
        let auto_fill_sclk = 7 + 3 * u64::from(self.registers.auto_fill_length);
        self.state.auto_fill_mclk_remaining = auto_fill_sclk * 7 / 3;
    }

    fn in_vblank(&self) -> bool {
        self.state.scanline >= self.latched.v_resolution.active_scanlines_per_frame()
    }

    fn in_hblank(&self) -> bool {
        !(HBLANK_END_MCLK_CYCLES..HBLANK_START_MCLK_CYCLES).contains(&self.state.scanline_mclk)
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

        if (self.config.video_out != S32XVideoOut::S32XOnly
            && self.latched.frame_buffer_mode == FrameBufferMode::Blank)
            || self.config.video_out == S32XVideoOut::GenesisOnly
        {
            // Leave Genesis frame as-is
            // TODO what if 32X VDP was switched to blank mode mid-frame?
            return;
        }

        let genesis_frame_size = genesis_vdp.frame_size();
        let border_size = genesis_vdp.border_size();

        let h32 = genesis_frame_size.width - border_size.left - border_size.right == 256;
        if h32 {
            // If the Genesis VDP is in H32 mode, render the frame in 1280x224 so that it's possible
            // to composite the 320x224 32X frame with the 256x224 Genesis frame without needing to
            // blend or filter any pixels.
            // NFL Quarterback Club depends on this - it uses H32 mode in menus
            self.composite_frame_expanded::<true>(genesis_frame_size, border_size, genesis_vdp);
            return;
        }

        if self.config.emulate_pixel_switch_delay {
            // In H40 mode, but need to render at sub-pixel resolution to emulate pixel switch delay
            self.composite_frame_expanded::<false>(genesis_frame_size, border_size, genesis_vdp);
            return;
        }

        // Otherwise, composite in H40 into the Genesis VDP frame buffer
        self.composite_frame_inner::<false, false>(genesis_frame_size, border_size, genesis_vdp);
    }

    fn composite_frame_expanded<const H32: bool>(
        &mut self,
        genesis_frame_size: FrameSize,
        border_size: BorderSize,
        genesis_vdp: &mut GenesisVdp,
    ) {
        let genesis_frame_width =
            genesis_frame_size.width.wrapping_sub(border_size.left).wrapping_sub(border_size.right);
        if H32 {
            debug_assert_eq!(genesis_frame_width, 256);
        } else {
            debug_assert_eq!(genesis_frame_width, 320);
        }

        self.state.next_render_buffer =
            if H32 { WhichFrameBuffer::ExpandedH32 } else { WhichFrameBuffer::ExpandedH40 };

        if self.config.video_out != S32XVideoOut::S32XOnly {
            self.copy_genesis_frame_buffer_to_expanded::<H32>(
                genesis_frame_size,
                border_size,
                genesis_vdp.frame_buffer_mut(),
            );
        } else {
            self.expanded_frame_buffer.fill(Color::rgba(0, 0, 0, 0));
        }

        self.composite_frame_inner::<H32, true>(genesis_frame_size, border_size, genesis_vdp);
    }

    fn composite_frame_inner<const H32: bool, const EXPAND_H: bool>(
        &mut self,
        frame_size: FrameSize,
        border_size: BorderSize,
        genesis_vdp: &mut GenesisVdp,
    ) {
        fn should_use_32x_pixel(s32x_only: bool, s32x_pixel: u16, gen_color: Color) -> bool {
            s32x_only || s32x_pixel.bit(15) || gen_color.a == 0
        }

        assert!(
            !H32 || EXPAND_H,
            "Does not make sense to not expand if Genesis VDP is in H32 mode"
        );
        assert!(
            !self.config.emulate_pixel_switch_delay || EXPAND_H,
            "Does not make sense to not expand if emulating pixel switch delay"
        );

        let interlaced_frame: u32 = genesis_vdp.is_interlaced_frame().into();
        let interlaced_odd: u32 = genesis_vdp.is_interlaced_odd().into();

        let frame_buffer = if EXPAND_H {
            self.expanded_frame_buffer.as_mut()
        } else {
            genesis_vdp.frame_buffer_mut()
        };

        let active_lines_per_frame: u32 =
            self.latched.v_resolution.active_scanlines_per_frame().into();
        let s32x_only = self.config.video_out == S32XVideoOut::S32XOnly;

        let frame_width = if EXPAND_H {
            determine_expanded_buffer_width::<H32>(frame_size, border_size)
        } else {
            frame_size.width
        };
        let left_offset = match (EXPAND_H, H32) {
            (false, _) => border_size.left,
            (true, false) => genesis_expanded_pixel_width::<false>() * border_size.left,
            (true, true) => {
                genesis_expanded_pixel_width::<true>() * border_size.left + H32_H_OFFSET
            }
        };

        let top_offset = border_size.top << interlaced_frame;

        let color_tables = ColorTables::from_tint(self.config.color_tint);

        for line in 0..active_lines_per_frame {
            let effective_line = (line << interlaced_frame) + interlaced_odd;
            let fb_row_addr = ((effective_line + top_offset) * frame_width + left_offset) as usize;

            if EXPAND_H && self.config.emulate_pixel_switch_delay {
                let mut last_pixel_was_32x = false;

                for pixel in 0..FRAME_WIDTH {
                    let s32x_pixel = self.rendered_frame[line as usize][pixel as usize];
                    let fb_addr = fb_row_addr + 4 * pixel as usize;

                    for i in 0..4 {
                        // Quarter-pixel delay when switching from 32X output to Genesis output
                        let s32x_has_priority =
                            should_use_32x_pixel(s32x_only, s32x_pixel, frame_buffer[fb_addr + i]);
                        if last_pixel_was_32x || s32x_has_priority {
                            frame_buffer[fb_addr + i] = u16_to_rgb(s32x_pixel, color_tables);
                        }
                        last_pixel_was_32x = s32x_has_priority;
                    }
                }
            } else if EXPAND_H {
                // Horizontal resolution expansion w/o pixel switch delay emulation
                for pixel in 0..FRAME_WIDTH {
                    let s32x_pixel = self.rendered_frame[line as usize][pixel as usize];
                    let fb_addr = fb_row_addr + 4 * pixel as usize;

                    for i in 0..4 {
                        if should_use_32x_pixel(s32x_only, s32x_pixel, frame_buffer[fb_addr + i]) {
                            frame_buffer[fb_addr + i] = u16_to_rgb(s32x_pixel, color_tables);
                        }
                    }
                }
            } else {
                // No horizontal resolution expansion
                for pixel in 0..FRAME_WIDTH {
                    let s32x_pixel = self.rendered_frame[line as usize][pixel as usize];
                    let fb_addr = fb_row_addr + pixel as usize;

                    if should_use_32x_pixel(s32x_only, s32x_pixel, frame_buffer[fb_addr]) {
                        frame_buffer[fb_addr] = u16_to_rgb(s32x_pixel, color_tables);
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

    fn copy_genesis_frame_buffer_to_expanded<const H32: bool>(
        &mut self,
        frame_size: FrameSize,
        border_size: BorderSize,
        frame_buffer: &mut [Color; genesis_core::vdp::FRAME_BUFFER_LEN],
    ) {
        let expanded_frame_width = determine_expanded_buffer_width::<H32>(frame_size, border_size);
        let gen_pixel_width = genesis_expanded_pixel_width::<H32>();

        // Expand the frame buffer from 256x224 to 1280x224 (plus borders)
        for line in 0..frame_size.height {
            let h32_fb_line_addr = line * expanded_frame_width;
            for pixel in 0..frame_size.width {
                let genesis_fb_addr = (line * frame_size.width + pixel) as usize;
                let h32_fb_addr = (h32_fb_line_addr + gen_pixel_width * pixel) as usize;
                self.expanded_frame_buffer[h32_fb_addr..h32_fb_addr + gen_pixel_width as usize]
                    .fill(frame_buffer[genesis_fb_addr]);
            }

            if gen_pixel_width * frame_size.width != expanded_frame_width {
                // Clear right border pixels so 32X pixels will always display over them
                self.expanded_frame_buffer[(h32_fb_line_addr + gen_pixel_width * frame_size.width)
                    as usize
                    ..(h32_fb_line_addr + expanded_frame_width) as usize]
                    .fill(Color::rgba(0, 0, 0, 0));
            }
        }
    }

    pub fn render_frame<R: Renderer>(
        &self,
        genesis_vdp: &GenesisVdp,
        aspect_ratio: Option<FiniteF64>,
        renderer: &mut R,
    ) -> Result<(), R::Err> {
        if self.state.next_render_buffer == WhichFrameBuffer::Genesis {
            let target_fps = genesis_core::target_framerate(genesis_vdp, genesis_vdp.timing_mode());
            return renderer.render_frame(
                genesis_vdp.frame_buffer(),
                genesis_vdp.frame_size(),
                target_fps,
                RenderFrameOptions::pixel_aspect_ratio(aspect_ratio),
            );
        }

        let h32 = self.state.next_render_buffer == WhichFrameBuffer::ExpandedH32;
        if h32 {
            self.render_expanded_frame::<true, _>(genesis_vdp, aspect_ratio, renderer)
        } else {
            self.render_expanded_frame::<false, _>(genesis_vdp, aspect_ratio, renderer)
        }
    }

    fn render_expanded_frame<const H32: bool, R: Renderer>(
        &self,
        genesis_vdp: &GenesisVdp,
        mut aspect_ratio: Option<FiniteF64>,
        renderer: &mut R,
    ) -> Result<(), R::Err> {
        let mut frame_size = genesis_vdp.frame_size();

        frame_size.width =
            determine_expanded_buffer_width::<H32>(frame_size, genesis_vdp.border_size());

        let gen_pixel_width = genesis_expanded_pixel_width::<H32>();
        aspect_ratio = aspect_ratio
            .map(|par| par * FiniteF64::try_from(1.0 / f64::from(gen_pixel_width)).unwrap());

        let target_fps = genesis_core::target_framerate(genesis_vdp, genesis_vdp.timing_mode());
        renderer.render_frame(
            self.expanded_frame_buffer.as_ref(),
            frame_size,
            target_fps,
            RenderFrameOptions::pixel_aspect_ratio(aspect_ratio),
        )
    }

    pub fn reload_config(&mut self, config: &Sega32XEmulatorConfig) {
        self.config = VdpConfig::from_emu_config(config);
    }
}

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

// Scaled to 0-251 instead of 0-255
const RGB_5_TO_8_SLIGHT_DARK: &[u8; 32] = &[
    0, 8, 16, 24, 32, 40, 49, 57, 65, 73, 81, 89, 97, 105, 113, 121, 130, 138, 146, 154, 162, 170,
    178, 186, 194, 202, 211, 219, 227, 235, 243, 251,
];

// Scaled to 0-247 instead of 0-255
const RGB_5_TO_8_DARK: &[u8; 32] = &[
    0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 127, 135, 143, 151, 159, 167,
    175, 183, 191, 199, 207, 215, 223, 231, 239, 247,
];

#[derive(Debug, Clone, Copy)]
struct ColorTables {
    red: &'static [u8; 32],
    green: &'static [u8; 32],
    blue: &'static [u8; 32],
}

impl ColorTables {
    fn from_tint(color_tint: S32XColorTint) -> Self {
        match color_tint {
            S32XColorTint::None => Self { red: RGB_5_TO_8, green: RGB_5_TO_8, blue: RGB_5_TO_8 },
            // yellow tint = blue deficiency
            S32XColorTint::SlightYellow => {
                Self { red: RGB_5_TO_8, green: RGB_5_TO_8, blue: RGB_5_TO_8_SLIGHT_DARK }
            }
            S32XColorTint::Yellow => {
                Self { red: RGB_5_TO_8, green: RGB_5_TO_8, blue: RGB_5_TO_8_DARK }
            }
            // purple tint = green deficiency
            S32XColorTint::SlightPurple => {
                Self { red: RGB_5_TO_8, green: RGB_5_TO_8_SLIGHT_DARK, blue: RGB_5_TO_8 }
            }
            S32XColorTint::Purple => {
                Self { red: RGB_5_TO_8, green: RGB_5_TO_8_DARK, blue: RGB_5_TO_8 }
            }
        }
    }
}

fn u16_to_rgb(s32x_pixel: u16, color_tables: ColorTables) -> Color {
    let r = s32x_pixel & 0x1F;
    let g = (s32x_pixel >> 5) & 0x1F;
    let b = (s32x_pixel >> 10) & 0x1F;

    Color::rgb(
        color_tables.red[r as usize],
        color_tables.green[g as usize],
        color_tables.blue[b as usize],
    )
}

const fn genesis_expanded_pixel_width<const H32: bool>() -> u32 {
    if H32 { 5 } else { 4 }
}

fn determine_expanded_buffer_width<const H32: bool>(
    frame_size: FrameSize,
    border_size: BorderSize,
) -> u32 {
    let gen_pixel_width = genesis_expanded_pixel_width::<H32>();

    if H32 && border_size.right < H32_H_OFFSET.div_ceil(gen_pixel_width) {
        gen_pixel_width * frame_size.width + H32_H_OFFSET
    } else {
        gen_pixel_width * frame_size.width
    }
}
