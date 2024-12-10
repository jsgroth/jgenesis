//! PSG channels, aka the Game Boy Color APU

mod wavetable;

use crate::apu::psg::wavetable::WavetableChannel;
use bincode::{Decode, Encode};
use gb_core::apu::StereoControl;
use gb_core::apu::noise::NoiseChannel;
use gb_core::apu::pulse::PulseChannel;
use jgenesis_common::num::GetBit;
use std::array;

const PSG_DIVIDER: u32 = 16;
const FRAME_SEQUENCER_DIVIDER: u32 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichPulse {
    One,
    Two,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GbcApu {
    enabled: bool,
    pulse_1: PulseChannel,
    pulse_2: PulseChannel,
    wavetable: WavetableChannel,
    noise: NoiseChannel,
    stereo_control: StereoControl,
    gba_counter: u32,
    frame_sequencer_step: u8,
    frame_sequencer_divider: u32,
}

impl GbcApu {
    pub fn new() -> Self {
        Self {
            enabled: false,
            pulse_1: PulseChannel::new(),
            pulse_2: PulseChannel::new(),
            wavetable: WavetableChannel::new(),
            noise: NoiseChannel::new(),
            stereo_control: StereoControl::new(),
            gba_counter: 0,
            frame_sequencer_step: 0,
            frame_sequencer_divider: FRAME_SEQUENCER_DIVIDER,
        }
    }

    pub fn tick(&mut self, gba_cycles: u32) {
        self.gba_counter += gba_cycles;
        while self.gba_counter >= PSG_DIVIDER {
            self.gba_counter -= PSG_DIVIDER;

            self.frame_sequencer_divider -= 1;
            if self.frame_sequencer_divider == 0 {
                self.frame_sequencer_divider = FRAME_SEQUENCER_DIVIDER;
                self.clock_frame_sequencer();
            }

            if self.enabled {
                self.pulse_1.tick_m_cycle();
                self.pulse_2.tick_m_cycle();
                self.wavetable.tick_m_cycle();
                self.noise.tick_m_cycle();
            }
        }
    }

    pub fn sample(&self) -> (i16, i16) {
        if !self.enabled {
            return (0, 0);
        }

        let channel_samples = [
            self.pulse_1.sample().unwrap_or(0),
            self.pulse_2.sample().unwrap_or(0),
            self.wavetable.sample().unwrap_or(0),
            self.noise.sample().unwrap_or(0),
        ]
        .map(i16::from);

        let l_samples: [i16; 4] = array::from_fn(|i| {
            if self.stereo_control.left_channels[i] { channel_samples[i] } else { 0 }
        });
        let r_samples: [i16; 4] = array::from_fn(|i| {
            if self.stereo_control.right_channels[i] { channel_samples[i] } else { 0 }
        });

        let mut sample_l = l_samples.into_iter().sum();
        let mut sample_r = r_samples.into_iter().sum();

        sample_l *= i16::from(self.stereo_control.left_volume + 1);
        sample_r *= i16::from(self.stereo_control.right_volume + 1);

        (sample_l, sample_r)
    }

    fn clock_frame_sequencer(&mut self) {
        self.frame_sequencer_step = (self.frame_sequencer_step + 1) & 7;

        if self.enabled {
            if !self.frame_sequencer_step.bit(0) {
                self.clock_length_counters();
            }

            if self.frame_sequencer_step == 7 {
                self.clock_envelopes();
            }

            if self.frame_sequencer_step == 2 || self.frame_sequencer_step == 6 {
                self.pulse_1.clock_sweep();
            }
        }
    }

    fn clock_length_counters(&mut self) {
        self.pulse_1.clock_length_counter();
        self.pulse_2.clock_length_counter();
        self.wavetable.clock_length_counter();
        self.noise.clock_length_counter();
    }

    fn clock_envelopes(&mut self) {
        self.pulse_1.clock_envelope();
        self.pulse_2.clock_envelope();
        self.noise.clock_envelope();
    }

    // $04000060: SOUND1CNT_L (NR10 from GBC) (Channel 1 sweep control)
    pub fn read_sound1cnt_l(&self) -> u16 {
        self.pulse_1.read_register_0().into()
    }

    // $04000060: SOUND1CNT_L (NR10 from GBC) (Channel 1 sweep control)
    pub fn write_sound1cnt_l(&mut self, value: u16) {
        log::trace!("SOUND1CNT_L write: {value:04X}");
        self.pulse_1.write_register_0(value as u8);
    }

    // $04000062: SOUND1CNT_H (NR11 + NR12 from GBC) (Duty cycle, length counter, envelope)
    // $04000068: SOUND2CNT_H (NR21 + NR22 from GBC)
    pub fn read_sound12cnt_h(&self, channel: WhichPulse) -> u16 {
        let channel = match channel {
            WhichPulse::One => &self.pulse_1,
            WhichPulse::Two => &self.pulse_2,
        };

        let lsb = channel.read_register_1();
        let msb = channel.read_register_2();
        u16::from_le_bytes([lsb, msb])
    }

    // $04000062: SOUND1CNT_H (NR11 + NR12 from GBC) (Duty cycle, length counter, envelope)
    // $04000068: SOUND2CNT_L (NR21 + NR22 from GBC)
    pub fn write_sound12cnt_h(&mut self, channel: WhichPulse, value: u16) {
        log::trace!("{} write: {value:04X}", match channel {
            WhichPulse::One => "SOUND1CNT_H",
            WhichPulse::Two => "SOUND2CNT_L",
        });

        let [lsb, msb] = value.to_le_bytes();
        self.write_nr11nr21(channel, lsb);
        self.write_nr12nr22(channel, msb);
    }

    pub fn write_nr11nr21(&mut self, channel: WhichPulse, value: u8) {
        let channel = match channel {
            WhichPulse::One => &mut self.pulse_1,
            WhichPulse::Two => &mut self.pulse_2,
        };

        channel.write_register_1(value, true);
    }

    pub fn write_nr12nr22(&mut self, channel: WhichPulse, value: u8) {
        let channel = match channel {
            WhichPulse::One => &mut self.pulse_1,
            WhichPulse::Two => &mut self.pulse_2,
        };

        channel.write_register_2(value);
    }

    // $04000064: SOUND1CNT_X (NR13 + NR14 from GBC) (Frequency, trigger)
    // $0400006C: SOUND2CNT_H (NR23 + NR24 from GBC)
    pub fn read_sound12cnt_x(&self, channel: WhichPulse) -> u16 {
        let channel = match channel {
            WhichPulse::One => &self.pulse_1,
            WhichPulse::Two => &self.pulse_2,
        };

        let msb = channel.read_register_4();
        u16::from_le_bytes([0xFF, msb])
    }

    // $04000064: SOUND1CNT_X (NR13 + NR14 from GBC) (Frequency, trigger)
    // $0400006C: SOUND2CNT_H (NR23 + NR24 from GBC)
    pub fn write_sound12cnt_x(&mut self, channel: WhichPulse, value: u16) {
        log::trace!("{} write: {value:04X}", match channel {
            WhichPulse::One => "SOUND1CNT_X",
            WhichPulse::Two => "SOUND2CNT_H",
        });

        let [lsb, msb] = value.to_le_bytes();

        let channel = match channel {
            WhichPulse::One => &mut self.pulse_1,
            WhichPulse::Two => &mut self.pulse_2,
        };

        channel.write_register_3(lsb);
        channel.write_register_4(msb, self.frame_sequencer_step);
    }

    pub fn write_nr13nr23(&mut self, channel: WhichPulse, value: u8) {
        let channel = match channel {
            WhichPulse::One => &mut self.pulse_1,
            WhichPulse::Two => &mut self.pulse_2,
        };
        channel.write_register_3(value);
    }

    pub fn write_nr14nr24(&mut self, channel: WhichPulse, value: u8) {
        let channel = match channel {
            WhichPulse::One => &mut self.pulse_1,
            WhichPulse::Two => &mut self.pulse_2,
        };
        channel.write_register_4(value, self.frame_sequencer_step);
    }

    // $04000070: SOUND3CNT_L (NR30 from GBC) (Channel 3 enabled and wave RAM mode)
    pub fn read_sound3cnt_l(&self) -> u16 {
        let lsb = self.wavetable.read_register_0();
        u16::from_le_bytes([lsb, 0xFF])
    }

    // $04000070: SOUND3CNT_L (NR30 from GBC) (Channel 3 enabled and wave RAM mode)
    pub fn write_sound3cnt_l(&mut self, value: u16) {
        log::trace!("SOUND3CNT_L write: {value:04X}");
        self.wavetable.write_register_0(value as u8);
    }

    // $04000072: SOUND3CNT_H (NR31 + NR32 from GBC) (Length counter, volume)
    pub fn read_sound3cnt_h(&self) -> u16 {
        let msb = self.wavetable.read_register_2();
        u16::from_le_bytes([0xFF, msb])
    }

    // $04000072: SOUND3CNT_H (NR31 + NR32 from GBC) (Length counter, volume)
    pub fn write_sound3cnt_h(&mut self, value: u16) {
        log::trace!("SOUND3CNT_H write: {value:04X}");

        let [lsb, msb] = value.to_le_bytes();
        self.write_nr31(lsb);
        self.write_nr32(msb);
    }

    pub fn write_nr31(&mut self, value: u8) {
        self.wavetable.write_register_1(value);
    }

    pub fn write_nr32(&mut self, value: u8) {
        self.wavetable.write_register_2(value);
    }

    // $04000074: SOUND3CNT_X (NR33 + NR34 from GBC) (Frequency, trigger)
    pub fn read_sound3cnt_x(&self) -> u16 {
        let msb = self.wavetable.read_register_4();
        u16::from_le_bytes([0xFF, msb])
    }

    // $04000074: SOUND3CNT_X (NR33 + NR34 from GBC) (Frequency, trigger)
    pub fn write_sound3cnt_x(&mut self, value: u16) {
        log::trace!("SOUND3CNT_X write: {value:04X}");
        let [lsb, msb] = value.to_le_bytes();
        self.write_nr33(lsb);
        self.write_nr34(msb);
    }

    pub fn write_nr33(&mut self, value: u8) {
        self.wavetable.write_register_3(value);
    }

    pub fn write_nr34(&mut self, value: u8) {
        self.wavetable.write_register_4(value, self.frame_sequencer_step);
    }

    // $04000078: SOUND4CNT_L (NR41 + NR42 from GBC) (Length counter, envelope)
    pub fn read_sound4cnt_l(&self) -> u16 {
        let msb = self.noise.read_register_2();
        u16::from_le_bytes([0xFF, msb])
    }

    // $04000078: SOUND4CNT_L (NR41 + NR42 from GBC) (Length counter, envelope)
    pub fn write_sound4cnt_l(&mut self, value: u16) {
        log::trace!("SOUND4CNT_L write: {value:04X}");
        let [lsb, msb] = value.to_le_bytes();
        self.write_nr41(lsb);
        self.write_nr42(msb);
    }

    pub fn write_nr41(&mut self, value: u8) {
        self.noise.write_register_1(value);
    }

    pub fn write_nr42(&mut self, value: u8) {
        self.noise.write_register_2(value);
    }

    // $0400007C: SOUND4CNT_H (NR43 + NR44 from GBC) (Frequency, trigger)
    pub fn read_sound4cnt_h(&self) -> u16 {
        let lsb = self.noise.read_register_3();
        let msb = self.noise.read_register_4();
        u16::from_le_bytes([lsb, msb])
    }

    // $0400007C: SOUND4CNT_H (NR43 + NR44 from GBC) (Frequency, trigger)
    pub fn write_sound4cnt_h(&mut self, value: u16) {
        log::trace!("SOUND4CNT_H write: {value:04X}");
        let [lsb, msb] = value.to_le_bytes();
        self.write_nr43(lsb);
        self.write_nr44(msb);
    }

    pub fn write_nr43(&mut self, value: u8) {
        self.noise.write_register_3(value);
    }

    pub fn write_nr44(&mut self, value: u8) {
        self.noise.write_register_4(value, self.frame_sequencer_step);
    }

    // $04000080: SOUNDCNT_L (NR50 + NR51 from GBC) (Master volume, panning flags)
    pub fn read_soundcnt_l(&self) -> u16 {
        let lsb = self.stereo_control.read_volume();
        let msb = self.stereo_control.read_enabled();
        u16::from_le_bytes([lsb, msb])
    }

    // $04000080: SOUNDCNT_L (NR50 + NR51 from GBC) (Master volume, panning flags)
    pub fn write_soundcnt_l(&mut self, value: u16) {
        log::trace!("SOUNDCNT_L write: {value:04X}");

        let [lsb, msb] = value.to_le_bytes();
        self.write_nr50(lsb);
        self.write_nr51(msb);
    }

    pub fn write_nr50(&mut self, value: u8) {
        self.stereo_control.write_volume(value);
    }

    pub fn write_nr51(&mut self, value: u8) {
        self.stereo_control.write_enabled(value);
    }

    // $04000084: SOUNDCNT_X (NR52 from GBC) (Channel enabled bits)
    pub fn read_nr52(&self) -> u16 {
        (u16::from(self.noise.enabled()) << 3)
            | (u16::from(self.wavetable.enabled()) << 2)
            | (u16::from(self.pulse_2.enabled()) << 1)
            | u16::from(self.pulse_1.enabled())
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn read_wave_ram(&self, address: u32) -> u16 {
        let lsb = self.wavetable.read_ram(address);
        let msb = self.wavetable.read_ram(address + 1);
        u16::from_le_bytes([lsb, msb])
    }

    pub fn write_wave_ram(&mut self, address: u32, value: u8) {
        self.wavetable.write_ram(address, value);
    }

    pub fn write_wave_ram_u16(&mut self, address: u32, value: u16) {
        let [lsb, msb] = value.to_le_bytes();
        self.wavetable.write_ram(address, lsb);
        self.wavetable.write_ram(address + 1, msb);
    }
}
