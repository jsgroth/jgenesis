//! GBA APU (audio processing unit)
//!
//! The GBA APU contains the 4 channels from the Game Boy Color APU plus 2 new Direct Sound channels
//! that play 8-bit PCM samples. The GBC APU channels are unchanged except for the custom wave channel,
//! which now has twice as much wavetable RAM (split into two banks of 16 bytes each).
//!
//! Actual hardware outputs audio using 1-bit PWM at ~16.77 MHz. This is expensive to emulate, so
//! instead audio output is emulated as PCM at the configured sample rate (ranges from 32.768 KHz
//! to 262.144 KHz).

mod psg;

use crate::apu::psg::{GbcApu, WhichPulse};
use crate::audio::GbaAudioResampler;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::{GetBit, U16Ext};
use std::collections::VecDeque;

pub const FIFO_A_ADDRESS: u32 = 0x040000A0;
pub const FIFO_B_ADDRESS: u32 = 0x040000A4;

const FIFO_LEN: usize = 32;
const DEFAULT_BIAS: i16 = 0x200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum PwmCycleShift {
    // 32768 Hz, 9-bit samples
    #[default]
    Nine = 0,
    // 65536 Hz, 8-bit samples
    Eight = 1,
    // 131072 Hz, 7-bit samples
    Seven = 2,
    // 262144 Hz, 6-bit samples
    Six = 3,
}

impl PwmCycleShift {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::Nine,
            1 => Self::Eight,
            2 => Self::Seven,
            3 => Self::Six,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    pub const fn sample_rate_hz(self) -> u32 {
        match self {
            Self::Nine => 32_768,
            Self::Eight => 65_536,
            Self::Seven => 131_072,
            Self::Six => 262_144,
        }
    }

    const fn gba_clock_downshift(self) -> u32 {
        match self {
            Self::Nine => 9,
            Self::Eight => 8,
            Self::Seven => 7,
            Self::Six => 6,
        }
    }

    const fn sample_downshift(self) -> u32 {
        match self {
            Self::Nine => 1,
            Self::Eight => 2,
            Self::Seven => 3,
            Self::Six => 4,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct DirectSoundFifo(VecDeque<i8>);

impl DirectSoundFifo {
    fn new() -> Self {
        Self(VecDeque::with_capacity(FIFO_LEN))
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn pop(&mut self) -> Option<i8> {
        self.0.pop_front()
    }

    fn push(&mut self, sample: i8) {
        if self.0.len() == FIFO_LEN {
            // TODO what should happen when the FIFO is full?
            self.0.pop_back();
        }
        self.0.push_back(sample);
    }

    fn push_halfword(&mut self, value: u16) {
        self.push(value as i8);
        self.push((value >> 8) as i8);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum DirectSoundTimer {
    #[default]
    Zero = 0,
    One = 1,
}

impl DirectSoundTimer {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::One } else { Self::Zero }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct DirectSoundChannel {
    fifo: DirectSoundFifo,
    half_volume: bool,
    timer: DirectSoundTimer,
    l_output: bool,
    r_output: bool,
    current_sample: i8,
}

impl DirectSoundChannel {
    fn new() -> Self {
        Self {
            fifo: DirectSoundFifo::new(),
            half_volume: true,
            timer: DirectSoundTimer::default(),
            l_output: false,
            r_output: false,
            current_sample: 0,
        }
    }

    fn pop_fifo(&mut self) {
        self.current_sample = self.fifo.pop().unwrap_or(self.current_sample);
    }

    fn sample(&self) -> (i16, i16) {
        let mut sample: i16 = self.current_sample.into();
        if self.half_volume {
            sample >>= 1;
        }

        let sample_l = i16::from(self.l_output) * sample;
        let sample_r = i16::from(self.r_output) * sample;
        (sample_l, sample_r)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Apu {
    enabled: bool,
    channel_a: DirectSoundChannel,
    channel_b: DirectSoundChannel,
    psg: GbcApu,
    psg_volume: u8,
    sound_bias: i16,
    cycle_shift: PwmCycleShift,
    resampler: GbaAudioResampler,
    sample_counter: u64,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            enabled: false,
            channel_a: DirectSoundChannel::new(),
            channel_b: DirectSoundChannel::new(),
            psg: GbcApu::new(),
            psg_volume: 0,
            sound_bias: DEFAULT_BIAS,
            cycle_shift: PwmCycleShift::default(),
            resampler: GbaAudioResampler::new(),
            sample_counter: 0,
        }
    }

    pub fn tick<A: AudioOutput>(
        &mut self,
        cycles: u32,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.sample_counter += u64::from(cycles) * u64::from(self.cycle_shift.sample_rate_hz());
        while self.sample_counter >= u64::from(crate::GBA_CLOCK_RATE) {
            self.sample_counter -= u64::from(crate::GBA_CLOCK_RATE);

            self.psg.tick(1 << self.cycle_shift.gba_clock_downshift());
            self.generate_sample();
        }

        self.resampler.drain_output(audio_output)?;

        Ok(())
    }

    fn generate_sample(&mut self) {
        // Direct Sound channels
        // Raw channel outputs are signed 8-bit: [-0x80, 0x7F]
        // Multiplied by 4 before mixing (signed 10-bit): [-0x200, 0x1FF]
        let (channel_a_l, channel_a_r) = {
            let (l, r) = self.channel_a.sample();
            (l << 2, r << 2)
        };
        let (channel_b_l, channel_b_r) = {
            let (l, r) = self.channel_b.sample();
            (l << 2, r << 2)
        };

        // PSG channels
        // Each individual channel is unsigned 4-bit: [0x0, 0xF]
        // 4 channels combined are unsigned 6-bit: [0x00, 0x3F]
        // Max volume multiplies by 8 for unsigned 9-bit: [0x000, 0x1FF]
        // Multiplied by 2 before mixing (unsigned 10-bit): [0x000, 0x3FF]
        let (psg_l, psg_r) = {
            let (mut l, mut r) = self.psg.sample();
            l <<= 1;
            r <<= 1;

            if self.psg_volume < 2 {
                // 0=25%, 1=50%, 2=100%
                let psg_downshift = 2 - self.psg_volume;
                l >>= psg_downshift;
                r >>= psg_downshift;
            }

            (l, r)
        };

        // Mixing: Add Direct Sound and PSG samples together with sound bias, and clamp the final
        // result to unsigned 10-bit
        let pwm_sample_l =
            (channel_a_l + channel_b_l + psg_l + self.sound_bias).clamp(0x000, 0x3FF);
        let pwm_sample_r =
            (channel_a_r + channel_b_r + psg_r + self.sound_bias).clamp(0x000, 0x3FF);

        // Downshift (1-4) based on current PWM sample cycle
        // Higher PWM sample rates have lower sample bit depth, which works by dropping the lowest bits
        let downshift = self.cycle_shift.sample_downshift();

        // Convert from unsigned 10-bit to signed N-bit (N=6-9 depending on sample rate)
        let pcm_sample_l = (pwm_sample_l - 0x200) >> downshift;
        let pcm_sample_r = (pwm_sample_r - 0x200) >> downshift;

        // Convert to floating-point [-1.0, +1.0]
        let final_sample_l = f64::from(pcm_sample_l) / f64::from(0x200 >> downshift);
        let final_sample_r = f64::from(pcm_sample_r) / f64::from(0x200 >> downshift);

        self.resampler.collect_sample(final_sample_l, final_sample_r);
    }

    pub fn read_register(&mut self, address: u32) -> u16 {
        log::trace!("APU register read: {address:08X}");

        match address & 0xFF {
            0x60 => self.psg.read_sound1cnt_l(),
            0x62 => self.psg.read_sound12cnt_h(WhichPulse::One),
            0x64 => self.psg.read_sound12cnt_x(WhichPulse::One),
            0x68 => self.psg.read_sound12cnt_h(WhichPulse::Two),
            0x6C => self.psg.read_sound12cnt_x(WhichPulse::Two),
            0x70 => self.psg.read_sound3cnt_l(),
            0x72 => self.psg.read_sound3cnt_h(),
            0x74 => self.psg.read_sound3cnt_x(),
            0x78 => self.psg.read_sound4cnt_l(),
            0x7C => self.psg.read_sound4cnt_h(),
            0x80 => self.psg.read_soundcnt_l(),
            0x82 => self.read_soundcnt_h(),
            0x84 => self.read_soundcnt_x(),
            0x88 => self.read_soundbias(),
            0x90..=0x9F => self.psg.read_wave_ram(address),
            _ => {
                log::error!("Invalid APU register read: {address:08X}");
                0
            }
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        // Can only write to SOUNDCNT_H, SOUNDCNT_X, and SOUNDBIAS while the APU is disabled
        if !self.enabled && !(0x04000082..0x0400008A).contains(&address) {
            log::warn!("Write to APU register while disabled: {address:08X} {value:04X}");
            return;
        }

        match address & 0xFF {
            0x60 => self.psg.write_sound1cnt_l(value),
            0x62 => self.psg.write_sound12cnt_h(WhichPulse::One, value),
            0x64 => self.psg.write_sound12cnt_x(WhichPulse::One, value),
            0x68 => self.psg.write_sound12cnt_h(WhichPulse::Two, value),
            0x6C => self.psg.write_sound12cnt_x(WhichPulse::Two, value),
            0x70 => self.psg.write_sound3cnt_l(value),
            0x72 => self.psg.write_sound3cnt_h(value),
            0x74 => self.psg.write_sound3cnt_x(value),
            0x78 => self.psg.write_sound4cnt_l(value),
            0x7C => self.psg.write_sound4cnt_h(value),
            0x80 => self.psg.write_soundcnt_l(value),
            0x82 => self.write_soundcnt_h(value),
            0x84 => self.write_soundcnt_x(value),
            0x88 => self.write_soundbias(value),
            0x90..=0x9F => self.psg.write_wave_ram_u16(address, value),
            0xA0 | 0xA2 => {
                self.channel_a.fifo.push_halfword(value);
                log::trace!("FIFO A push: {value:04X}");
            }
            0xA4 | 0xA6 => {
                self.channel_b.fifo.push_halfword(value);
                log::trace!("FIFO B push: {value:04X}");
            }
            _ => {
                log::error!("Invalid APU register write: {address:08X} {value:04X}");
            }
        }
    }

    pub fn write_register_u8(&mut self, address: u32, value: u8) {
        // Can only write to SOUNDCNT_H, SOUNDCNT_X, and SOUNDBIAS while the APU is disabled
        if !self.enabled && !(0x04000082..0x0400008A).contains(&address) {
            log::warn!("Write to APU register while disabled: {address:08X} {value:04X}");
            return;
        }

        match address & 0xFF {
            0x60 => self.psg.write_sound1cnt_l(value.into()),
            0x62 => self.psg.write_nr11nr21(WhichPulse::One, value),
            0x63 => self.psg.write_nr12nr22(WhichPulse::One, value),
            0x64 => self.psg.write_nr13nr23(WhichPulse::One, value),
            0x65 => self.psg.write_nr14nr24(WhichPulse::One, value),
            0x68 => self.psg.write_nr11nr21(WhichPulse::Two, value),
            0x69 => self.psg.write_nr12nr22(WhichPulse::Two, value),
            0x6C => self.psg.write_nr13nr23(WhichPulse::Two, value),
            0x6D => self.psg.write_nr14nr24(WhichPulse::Two, value),
            0x70 => self.psg.write_sound3cnt_l(value.into()),
            0x72 => self.psg.write_nr31(value),
            0x73 => self.psg.write_nr32(value),
            0x74 => self.psg.write_nr33(value),
            0x75 => self.psg.write_nr34(value),
            0x78 => self.psg.write_nr41(value),
            0x79 => self.psg.write_nr42(value),
            0x7C => self.psg.write_nr43(value),
            0x7D => self.psg.write_nr44(value),
            0x80 => self.psg.write_nr50(value),
            0x81 => self.psg.write_nr51(value),
            0x82 => self.write_soundcnt_h_low(value),
            0x83 => self.write_soundcnt_h_high(value),
            0x84 => self.write_soundcnt_x(value.into()),
            0x88 => self.write_soundbias_low(value),
            0x89 => self.write_soundbias_high(value),
            0x90..=0x9F => self.psg.write_wave_ram(address, value),
            _ => {
                log::error!("Invalid 8-bit APU write: {address:08X} {value:02X}");
            }
        }
    }

    // $04000088: SOUNDBIAS (PWM control and bias)
    fn read_soundbias(&self) -> u16 {
        (self.sound_bias as u16) | ((self.cycle_shift as u16) << 14)
    }

    // $04000088: SOUNDBIAS (PWM control and bias)
    fn write_soundbias(&mut self, value: u16) {
        self.sound_bias = (value & 0x3FE) as i16;

        let cycle_shift = PwmCycleShift::from_bits(value >> 14);
        if cycle_shift != self.cycle_shift {
            self.resampler.change_cycle_shift(cycle_shift);
        }
        self.cycle_shift = cycle_shift;

        log::debug!("SOUNDBIAS write: {value:04X}");
        log::debug!("  Sound bias: 0x{:03X}", self.sound_bias);
        log::debug!("  PWM sample rate: {} Hz", self.cycle_shift.sample_rate_hz());
    }

    fn write_soundbias_low(&mut self, value: u8) {
        let mut existing = self.read_soundbias();
        existing.set_lsb(value);
        self.write_soundbias(existing);
    }

    fn write_soundbias_high(&mut self, value: u8) {
        let mut existing = self.read_soundbias();
        existing.set_msb(value);
        self.write_soundbias(existing);
    }

    // $04000082: SOUNDCNT_H (new to GBA) (Volume/mixing control)
    fn read_soundcnt_h(&self) -> u16 {
        u16::from(self.psg_volume)
            | (u16::from(!self.channel_a.half_volume) << 2)
            | (u16::from(!self.channel_b.half_volume) << 3)
            | (u16::from(self.channel_a.r_output) << 8)
            | (u16::from(self.channel_a.l_output) << 9)
            | ((self.channel_a.timer as u16) << 10)
            | (u16::from(self.channel_b.r_output) << 12)
            | (u16::from(self.channel_b.l_output) << 13)
            | ((self.channel_b.timer as u16) << 14)
    }

    // $04000082: SOUNDCNT_H (new to GBA) (Volume/mixing control)
    fn write_soundcnt_h(&mut self, value: u16) {
        self.psg_volume = (value & 3) as u8;
        self.channel_a.half_volume = !value.bit(2);
        self.channel_b.half_volume = !value.bit(3);
        self.channel_a.r_output = value.bit(8);
        self.channel_a.l_output = value.bit(9);
        self.channel_a.timer = DirectSoundTimer::from_bit(value.bit(10));
        self.channel_b.r_output = value.bit(12);
        self.channel_b.l_output = value.bit(13);
        self.channel_b.timer = DirectSoundTimer::from_bit(value.bit(14));

        if value.bit(11) {
            // Channel A reset
            self.channel_a.fifo.0.clear();
            self.channel_a.current_sample = 0;
        }

        if value.bit(15) {
            // Channel B reset
            self.channel_b.fifo.0.clear();
            self.channel_b.current_sample = 0;
        }

        log::debug!("SOUNDCNT_H write: {value:04X}");
        log::debug!("  PSG volume: {}", match self.psg_volume {
            0 => "25%",
            1 => "50%",
            2 => "100%",
            3 => "Prohibited",
            _ => unreachable!(),
        });
        log::debug!(
            "  Channel A volume: {}",
            if self.channel_a.half_volume { "50%" } else { "100%" }
        );
        log::debug!(
            "  Channel B volume: {}",
            if self.channel_b.half_volume { "50%" } else { "100%" }
        );
        log::debug!("  Channel A right output: {}", self.channel_a.r_output);
        log::debug!("  Channel A left output: {}", self.channel_a.l_output);
        log::debug!("  Channel A timer: {:?}", self.channel_a.timer);
        log::debug!("  Channel A reset: {}", value.bit(11));
        log::debug!("  Channel B right output: {}", self.channel_b.r_output);
        log::debug!("  Channel B left output: {}", self.channel_b.l_output);
        log::debug!("  Channel B timer: {:?}", self.channel_b.timer);
        log::debug!("  Channel B reset: {}", value.bit(15));
    }

    fn write_soundcnt_h_low(&mut self, value: u8) {
        let mut existing = self.read_soundcnt_h();
        existing.set_lsb(value);
        self.write_soundcnt_h(existing);
    }

    fn write_soundcnt_h_high(&mut self, value: u8) {
        let mut existing = self.read_soundcnt_h();
        existing.set_msb(value);
        self.write_soundcnt_h(existing);
    }

    // $04000084: SOUNDCNT_X (NR52 from GBC) (APU enabled)
    fn read_soundcnt_x(&self) -> u16 {
        0x70 | (u16::from(self.enabled) << 7) | self.psg.read_nr52()
    }

    // $04000084: SOUNDCNT_X (NR52 from GBC) (APU enabled)
    fn write_soundcnt_x(&mut self, value: u16) {
        let enabled = value.bit(7);
        if self.enabled && !enabled {
            // Disabling the APU zeroes out all registers except for SOUNDCNT_H, SOUNDCNT_X, and SOUNDBIAS
            for address in (0x60..0x82).step_by(2) {
                self.write_register(address, 0);
            }
        }

        self.enabled = enabled;
        self.psg.set_enabled(self.enabled);

        log::debug!("SOUNDCNT_X write: {value:04X}");
        log::debug!("  APU enabled: {}", self.enabled);
    }

    pub fn fifo_a_drq(&self) -> bool {
        self.channel_a.fifo.len() <= FIFO_LEN / 2
    }

    pub fn fifo_a_len(&self) -> usize {
        self.channel_a.fifo.len()
    }

    pub fn fifo_b_drq(&self) -> bool {
        self.channel_b.fifo.len() <= FIFO_LEN / 2
    }

    pub fn fifo_b_len(&self) -> usize {
        self.channel_b.fifo.len()
    }

    pub fn timer_0_overflow(&mut self) {
        self.handle_timer_overflow(DirectSoundTimer::Zero);
    }

    pub fn timer_1_overflow(&mut self) {
        self.handle_timer_overflow(DirectSoundTimer::One);
    }

    fn handle_timer_overflow(&mut self, timer: DirectSoundTimer) {
        for channel in [&mut self.channel_a, &mut self.channel_b] {
            if channel.timer == timer {
                channel.pop_fifo();
            }
        }
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }
}
