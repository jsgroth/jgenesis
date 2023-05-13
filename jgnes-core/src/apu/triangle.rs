//! The APU's triangle wave generator channel, which constantly cycles through a 32-step waveform
//! going from 15 down to 0 then back up to 15. Period is configurable.
//!
//! In addition to a length counter, this channel also has a "linear counter" which works similarly
//! but is clocked every quarter-frame rather than every half-frame.
//!
//! Channel output values are between 0 and 15 (inclusive).

use crate::apu::units::{LengthCounter, LengthCounterChannel, PhaseTimer};
use crate::num::GetBit;
use bincode::{Decode, Encode};

type TrianglePhaseTimer = PhaseTimer<32, 1, 11, false>;

#[derive(Debug, Clone, Encode, Decode)]
struct LinearCounter {
    counter: u8,
    reload_value: u8,
    control_flag: bool,
    reload_flag: bool,
}

impl LinearCounter {
    fn new() -> Self {
        Self {
            counter: 0,
            reload_value: 0,
            control_flag: false,
            reload_flag: false,
        }
    }

    fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        self.control_flag = tri_linear_value.bit(7);
        self.reload_value = tri_linear_value & 0x7F;
    }

    fn process_hi_update(&mut self) {
        self.reload_flag = true;
    }

    fn clock(&mut self) {
        if self.reload_flag {
            self.counter = self.reload_value;
        } else if self.counter > 0 {
            self.counter -= 1;
        }

        if !self.control_flag {
            self.reload_flag = false;
        }
    }
}

const TRIANGLE_WAVEFORM: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct TriangleChannel {
    timer: TrianglePhaseTimer,
    linear_counter: LinearCounter,
    length_counter: LengthCounter,
}

impl TriangleChannel {
    pub fn new() -> Self {
        Self {
            timer: TrianglePhaseTimer::new(),
            linear_counter: LinearCounter::new(),
            length_counter: LengthCounter::new(LengthCounterChannel::Triangle),
        }
    }

    pub fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        self.linear_counter
            .process_tri_linear_update(tri_linear_value);
        self.length_counter
            .process_tri_linear_update(tri_linear_value);
    }

    pub fn process_lo_update(&mut self, lo_value: u8) {
        self.timer.process_lo_update(lo_value);
    }

    pub fn process_hi_update(&mut self, hi_value: u8) {
        self.timer.process_hi_update(hi_value);
        self.linear_counter.process_hi_update();
        self.length_counter.process_hi_update(hi_value);
    }

    pub fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        self.length_counter.process_snd_chn_update(snd_chn_value);
    }

    pub fn clock_quarter_frame(&mut self) {
        self.linear_counter.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    fn silenced(&self) -> bool {
        self.linear_counter.counter == 0 || self.length_counter.counter == 0
    }

    pub fn tick_cpu(&mut self) {
        self.timer.tick(!self.silenced());
    }

    pub fn sample(&self) -> u8 {
        TRIANGLE_WAVEFORM[self.timer.phase as usize]
    }

    pub fn length_counter(&self) -> u8 {
        self.length_counter.counter
    }

    pub fn reset(&mut self) {
        self.timer.phase = 0;
    }
}
