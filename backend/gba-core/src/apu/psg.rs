//! PSG (programmable sound generator) channels
//!
//! Mostly the same as the Game Boy Color APU channels, though the wavetable channel works a bit differently

mod wavetable;

use crate::apu::psg::wavetable::WavetableChannel;
use bincode::{Decode, Encode};
use gb_core::apu::StereoControl;
use gb_core::apu::noise::NoiseChannel;
use gb_core::apu::pulse::PulseChannel;
use jgenesis_common::num::GetBit;

// Number of 1 MHz cycles for a tick rate of 512 Hz
const FRAME_SEQUENCER_DIVIDER: u64 = (1 << 20) / 512;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Psg {
    pulse_1: PulseChannel,
    pulse_2: PulseChannel,
    wavetable: WavetableChannel,
    noise: NoiseChannel,
    stereo_control: StereoControl,
    frame_sequencer_step: u8,
    frame_sequencer_divider: u64,
}

impl Psg {
    pub fn new() -> Self {
        Self {
            pulse_1: PulseChannel::new(),
            pulse_2: PulseChannel::new(),
            wavetable: WavetableChannel::new(),
            noise: NoiseChannel::new(),
            stereo_control: StereoControl::new(),
            frame_sequencer_step: 0,
            frame_sequencer_divider: FRAME_SEQUENCER_DIVIDER,
        }
    }

    pub fn tick_1mhz(&mut self, apu_enabled: bool) {
        self.tick_frame_sequencer(apu_enabled);

        self.pulse_1.tick_m_cycle();
        self.pulse_2.tick_m_cycle();
        self.wavetable.tick_1mhz();
        self.noise.tick_m_cycle();
    }

    fn tick_frame_sequencer(&mut self, apu_enabled: bool) {
        self.frame_sequencer_divider -= 1;
        if self.frame_sequencer_divider != 0 {
            return;
        }
        self.frame_sequencer_divider = FRAME_SEQUENCER_DIVIDER;

        self.frame_sequencer_step = (self.frame_sequencer_step + 1) % 8;

        if !apu_enabled {
            return;
        }

        // Length counters clock on steps 0, 2, 4, 6
        if !self.frame_sequencer_step.bit(0) {
            self.pulse_1.clock_length_counter();
            self.pulse_2.clock_length_counter();
            self.wavetable.clock_length_counter();
            self.noise.clock_length_counter();
        }

        // Envelopes clock on step 7
        if self.frame_sequencer_step == 7 {
            self.pulse_1.clock_envelope();
            self.pulse_2.clock_envelope();
            self.noise.clock_envelope();
        }

        // Channel 1 sweep clocks on steps 2 and 6
        if self.frame_sequencer_step == 2 || self.frame_sequencer_step == 6 {
            self.pulse_1.clock_sweep();
        }
    }

    pub fn sample(&self) -> (u16, u16) {
        let samples = [
            self.pulse_1.sample().unwrap_or(0),
            self.pulse_2.sample().unwrap_or(0),
            self.wavetable.sample(),
            self.noise.sample().unwrap_or(0),
        ]
        .map(u16::from);

        let mut sample_l: u16 = samples
            .into_iter()
            .enumerate()
            .map(|(i, sample)| u16::from(self.stereo_control.left_channels[i]) * sample)
            .sum();
        let mut sample_r: u16 = samples
            .into_iter()
            .enumerate()
            .map(|(i, sample)| u16::from(self.stereo_control.right_channels[i]) * sample)
            .sum();

        sample_l *= u16::from(self.stereo_control.left_volume + 1);
        sample_r *= u16::from(self.stereo_control.right_volume + 1);

        (sample_l, sample_r)
    }

    pub fn disable(&mut self) {
        *self = Self {
            frame_sequencer_step: self.frame_sequencer_step,
            frame_sequencer_divider: self.frame_sequencer_divider,
            ..Self::new()
        };
    }

    // $4000060: SOUND1CNT_L / NR10 (Channel 1 sweep)
    pub fn write_sound1cnt_l(&mut self, value: u8) {
        self.pulse_1.write_register_0(value);
    }

    // $4000060: SOUND1CNT_L / NR10 (Channel 1 sweep)
    pub fn read_sound1cnt_l(&self) -> u8 {
        self.pulse_1.read_register_0() & 0x7F
    }

    // $4000062: SOUND1CNT_H low / NR11 (Channel 1 duty cycle and length counter)
    pub fn write_sound1cnt_h_low(&mut self, value: u8) {
        self.pulse_1.write_register_1(value, true);
    }

    // $4000062: SOUND1CNT_H low / NR11 (Channel 1 duty cycle and length counter)
    pub fn read_sound1cnt_h_low(&self) -> u8 {
        self.pulse_1.read_register_1() & 0xC0
    }

    // $4000063: SOUND1CNT_H high / NR12 (Channel 1 envelope)
    pub fn write_sound1cnt_h_high(&mut self, value: u8) {
        self.pulse_1.write_register_2(value);
    }

    // $4000063: SOUND1CNT_H high / NR12 (Channel 1 envelope)
    pub fn read_sound1cnt_h_high(&self) -> u8 {
        self.pulse_1.read_register_2()
    }

    // $4000064: SOUND1CNT_X low / NR13 (Channel 1 frequency low bits)
    pub fn write_sound1cnt_x_low(&mut self, value: u8) {
        self.pulse_1.write_register_3(value);
    }

    // $4000065: SOUND1CNT_X high / NR14 (Channel 1 control)
    pub fn write_sound1cnt_x_high(&mut self, value: u8) {
        self.pulse_1.write_register_4(value, self.frame_sequencer_step);
    }

    // $4000065: SOUND1CNT_X high / NR14 (Channel 1 control)
    pub fn read_sound1cnt_x_high(&self) -> u8 {
        self.pulse_1.read_register_4() & 0x40
    }

    // $4000068: SOUND2CNT_L low / NR21 (Channel 2 duty cycle and length counter)
    pub fn write_sound2cnt_l_low(&mut self, value: u8) {
        self.pulse_2.write_register_1(value, true);
    }

    // $4000068: SOUND2CNT_L low / NR21 (Channel 2 duty cycle and length counter)
    pub fn read_sound2cnt_l_low(&self) -> u8 {
        self.pulse_2.read_register_1() & 0xC0
    }

    // $4000069: SOUND2CNT_L high / NR22 (Channel 2 envelope)
    pub fn write_sound2cnt_l_high(&mut self, value: u8) {
        self.pulse_2.write_register_2(value);
    }

    // $4000069: SOUND2CNT_L high / NR22 (Channel 2 envelope)
    pub fn read_sound2cnt_l_high(&self) -> u8 {
        self.pulse_2.read_register_2()
    }

    // $400006C: SOUND2CNT_H low / NR23 (Channel 2 frequency low bits)
    pub fn write_sound2cnt_h_low(&mut self, value: u8) {
        self.pulse_2.write_register_3(value);
    }

    // $400006D: SOUND2CNT_H high / NR24 (Channel 2 control)
    pub fn write_sound2cnt_h_high(&mut self, value: u8) {
        self.pulse_2.write_register_4(value, self.frame_sequencer_step);
    }

    // $400006D: SOUND2CNT_H high / NR24 (Channel 2 control)
    pub fn read_sound2cnt_h_high(&self) -> u8 {
        self.pulse_2.read_register_4() & 0x40
    }

    // $4000070: SOUND3CNT_L / NR30 (Channel 3 enabled and wave RAM control)
    pub fn write_sound3cnt_l(&mut self, value: u8) {
        self.wavetable.write_nr30(value);
    }

    // $4000070: SOUND3CNT_L / NR30 (Channel 3 enabled and wave RAM control)
    pub fn read_sound3cnt_l(&self) -> u8 {
        self.wavetable.read_nr30() & 0xE0
    }

    // $4000072: SOUND3CNT_H low / NR31 (Channel 3 length counter)
    pub fn write_sound3cnt_h_low(&mut self, value: u8) {
        self.wavetable.write_nr31(value);
    }

    // $4000073: SOUND3CNT_H high / NR32 (Channel 3 volume)
    pub fn write_sound3cnt_h_high(&mut self, value: u8) {
        self.wavetable.write_nr32(value);
    }

    // $4000073: SOUND3CNT_H high / NR32 (Channel 3 volume)
    pub fn read_sound3cnt_h_high(&self) -> u8 {
        self.wavetable.read_nr32() & 0xE0
    }

    // $4000074: SOUND3CNT_X low / NR33 (Channel 3 frequency low bits)
    pub fn write_sound3cnt_x_low(&mut self, value: u8) {
        self.wavetable.write_nr33(value);
    }

    // $4000075: SOUND3CNT_X high / NR34 (Channel 3 control)
    pub fn write_sound3cnt_x_high(&mut self, value: u8) {
        self.wavetable.write_nr34(value, self.frame_sequencer_step);
    }

    // $4000075: SOUND3CNT_X high / NR34 (Channel 3 control)
    pub fn read_sound3cnt_x_high(&self) -> u8 {
        self.wavetable.read_nr34() & 0x40
    }

    // $4000078: SOUND4CNT_L low / NR41 (Channel 4 length counter)
    pub fn write_sound4cnt_l_low(&mut self, value: u8) {
        self.noise.write_register_1(value);
    }

    // $4000079: SOUND4CNT_L high / NR42 (Channel 4 envelope)
    pub fn write_sound4cnt_l_high(&mut self, value: u8) {
        self.noise.write_register_2(value);
    }

    // $4000079: SOUND4CNT_L high / NR42 (Channel 4 envelope)
    pub fn read_sound4cnt_l_high(&self) -> u8 {
        self.noise.read_register_2()
    }

    // $400007C: SOUND4CNT_H low / NR43 (Channel 4 frequency)
    pub fn write_sound4cnt_h_low(&mut self, value: u8) {
        self.noise.write_register_3(value);
    }

    // $400007C: SOUND4CNT_H low / NR43 (Channel 4 frequency)
    pub fn read_sound4cnt_h_low(&self) -> u8 {
        self.noise.read_register_3()
    }

    // $400007D: SOUND4CNT_H high / NR44 (Channel 4 control)
    pub fn write_sound4cnt_h_high(&mut self, value: u8) {
        self.noise.write_register_4(value, self.frame_sequencer_step);
    }

    // $400007D: SOUND4CNT_H high / NR44 (Channel 4 control)
    pub fn read_sound4cnt_h_high(&self) -> u8 {
        self.noise.read_register_4() & 0x40
    }

    // $4000080: SOUNDCNT_L low / NR50 (PSG master volume)
    pub fn write_soundcnt_l_low(&mut self, value: u8) {
        self.stereo_control.write_volume(value);
    }

    // $4000080: SOUNDCNT_L low / NR50 (PSG master volume)
    pub fn read_soundcnt_l_low(&self) -> u8 {
        self.stereo_control.read_volume() & 0x77
    }

    // $4000081: SOUNDCNT_L high / NR51 (PSG stereo control)
    pub fn write_soundcnt_l_high(&mut self, value: u8) {
        self.stereo_control.write_enabled(value);
    }

    // $4000081: SOUNDCNT_L high / NR51 (PSG stereo control)
    pub fn read_soundcnt_l_high(&self) -> u8 {
        self.stereo_control.read_enabled()
    }

    // $4000084: SOUNDCNT_X / NR52 (Channels and APU enabled)
    pub fn read_soundcnt_x(&self, apu_enabled: bool) -> u8 {
        (u8::from(self.pulse_1.enabled()))
            | (u8::from(self.pulse_2.enabled()) << 1)
            | (u8::from(self.wavetable.active()) << 2)
            | (u8::from(self.noise.enabled()) << 3)
            | (u8::from(apu_enabled) << 7)
    }

    // $4000090-$400009F: Wave RAM
    pub fn read_wave_ram(&self, address: u32) -> u8 {
        self.wavetable.read_wave_ram(address)
    }

    // $4000090-$400009F: Wave RAM
    pub fn write_wave_ram(&mut self, address: u32, value: u8) {
        self.wavetable.write_wave_ram(address, value);
    }
}
