//! GBA APU (audio processing unit)
//!
//! Contains the 4 Game Boy Color APU channels (slightly modified) plus two 8-bit PCM channels (Direct Sound)

mod audio;
mod psg;

use crate::api::GbaAudioConfig;
use crate::apu::audio::{BasicResampler, InterpolatingResampler};
use crate::apu::psg::Psg;
use crate::dma::DmaState;
use bincode::{Decode, Encode};
use gba_config::GbaAudioInterpolation;
use jgenesis_common::define_bit_enum;
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::GetBit;
use std::array;

pub const FIFO_A_ADDRESS: u32 = 0x40000A0;
pub const FIFO_B_ADDRESS: u32 = 0x40000A4;

const FIFO_CAPACITY: u8 = 7;

define_bit_enum!(DirectSoundTimer, [Zero, One]);

// Implementation based on https://github.com/mgba-emu/mgba/issues/1847
#[derive(Debug, Clone, Encode, Decode)]
struct DirectSoundFifo {
    buffer: [u32; FIFO_CAPACITY as usize],
    read_idx: u8,
    write_idx: u8,
    len: u8,
    playing: [i8; 4],
    playing_idx: u8,
}

impl DirectSoundFifo {
    fn new() -> Self {
        Self {
            buffer: array::from_fn(|_| 0),
            read_idx: 0,
            write_idx: 0,
            len: 0,
            playing: array::from_fn(|_| 0),
            playing_idx: 0,
        }
    }

    fn push(&mut self, word: u32) {
        self.buffer[self.write_idx as usize] = word;

        self.write_idx += 1;
        if self.write_idx == FIFO_CAPACITY {
            self.write_idx = 0;
        }

        self.len += 1;
        if self.len > FIFO_CAPACITY {
            self.reset();
        }
    }

    fn push_halfword(&mut self, address: u32, halfword: u16) {
        let existing = self.buffer[self.write_idx as usize];

        let word = if !address.bit(1) {
            u32::from(halfword) | (existing & !0xFFFF)
        } else {
            (u32::from(halfword) << 16) | (existing & 0xFFFF)
        };

        self.push(word);
    }

    fn push_byte(&mut self, address: u32, byte: u8) {
        let mut bytes = self.buffer[self.write_idx as usize].to_le_bytes();
        bytes[(address & 3) as usize] = byte;

        self.push(u32::from_le_bytes(bytes));
    }

    fn pop(&mut self) -> Option<i8> {
        if self.playing_idx == 0 {
            if self.len == 0 {
                return None;
            }

            let word = self.buffer[self.read_idx as usize];

            self.read_idx += 1;
            if self.read_idx == FIFO_CAPACITY {
                self.read_idx = 0;
            }

            self.len -= 1;

            self.playing = word.to_le_bytes().map(|b| b as i8);
        }

        let sample = self.playing[self.playing_idx as usize];
        self.playing_idx = (self.playing_idx + 1) % 4;

        Some(sample)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct DirectSoundChannel {
    name: String,
    fifo: DirectSoundFifo,
    current_sample: i8,
    volume_shift: u8,
    l_enabled: bool,
    r_enabled: bool,
    timer: DirectSoundTimer,
}

impl DirectSoundChannel {
    fn new(name: String) -> Self {
        Self {
            name,
            fifo: DirectSoundFifo::new(),
            current_sample: 0,
            volume_shift: 0,
            l_enabled: false,
            r_enabled: false,
            timer: DirectSoundTimer::default(),
        }
    }

    fn pop_fifo(&mut self) {
        self.current_sample = self.fifo.pop().unwrap_or(self.current_sample);
    }

    fn reset_fifo(&mut self) {
        self.fifo.reset();
    }

    fn dma_request(&self) -> bool {
        self.fifo.len <= FIFO_CAPACITY - 4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum PwmClockShift {
    #[default]
    Nine = 0, // 32768 Hz, 9-bit samples
    Eight = 1, // 65536 Hz, 8-bit samples
    Seven = 2, // 131072 Hz, 7-bit samples
    Six = 3,   // 262144 Hz, 6-bit samples
}

impl PwmClockShift {
    fn from_bits(bits: u8) -> Self {
        [Self::Nine, Self::Eight, Self::Seven, Self::Six][(bits & 3) as usize]
    }

    fn gba_clock_downshift(self) -> u8 {
        // Downshifting from ~16.77 MHz
        match self {
            Self::Nine => 9,
            Self::Eight => 8,
            Self::Seven => 7,
            Self::Six => 6,
        }
    }

    fn mixed_sample_downshift(self) -> u8 {
        // Downshifting from unsigned 10-bit
        match self {
            Self::Nine => 1,
            Self::Eight => 2,
            Self::Seven => 3,
            Self::Six => 4,
        }
    }

    fn source_frequency(self) -> u64 {
        crate::GBA_CLOCK_SPEED >> self.gba_clock_downshift()
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct PwmControl {
    clock_shift: PwmClockShift,
    sound_bias: i16,
}

impl PwmControl {
    fn new() -> Self {
        Self { clock_shift: PwmClockShift::default(), sound_bias: 0x200 }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum AudioResampler {
    Basic(Box<BasicResampler>),
    Interpolating(Box<InterpolatingResampler>),
}

impl AudioResampler {
    fn new(
        config: GbaAudioConfig,
        clock_shift: PwmClockShift,
        output_frequency: u64,
        pcm_frequencies: [Option<f64>; 2],
    ) -> Self {
        match config.audio_interpolation {
            GbaAudioInterpolation::NearestNeighbor => {
                Self::Basic(Box::new(BasicResampler::new(clock_shift, output_frequency)))
            }
            GbaAudioInterpolation::CubicHermite | GbaAudioInterpolation::WindowedSinc => {
                Self::Interpolating(Box::new(InterpolatingResampler::new(
                    config.audio_interpolation,
                    config.psg_low_pass,
                    output_frequency,
                    pcm_frequencies,
                )))
            }
        }
    }

    fn update_source_frequency(&mut self, clock_shift: PwmClockShift) {
        if let Self::Basic(resampler) = self {
            resampler.update_source_frequency(clock_shift);
        }
    }

    fn update_output_frequency(&mut self, output_frequency: u64) {
        match self {
            Self::Basic(resampler) => resampler.update_output_frequency(output_frequency),
            Self::Interpolating(resampler) => resampler.update_output_frequency(output_frequency),
        }
    }

    fn push_mixed_sample(&mut self, sample: [f64; 2]) {
        if let Self::Basic(resampler) = self {
            resampler.push_mixed_sample(sample);
        }
    }

    fn push_psg(&mut self, sample: (i16, i16)) {
        if let Self::Interpolating(resampler) = self {
            resampler.push_psg(sample);
        }
    }

    fn push_pcm_a(&mut self, sample: i8) {
        if let Self::Interpolating(resampler) = self {
            resampler.push_pcm_a(sample);
        }
    }

    fn push_pcm_b(&mut self, sample: i8) {
        if let Self::Interpolating(resampler) = self {
            resampler.push_pcm_b(sample);
        }
    }

    fn update_pcm_a_frequency(&mut self, frequency: Option<f64>) {
        if let Self::Interpolating(resampler) = self {
            resampler.update_pcm_a_frequency(frequency);
        }
    }

    fn update_pcm_b_frequency(&mut self, frequency: Option<f64>) {
        if let Self::Interpolating(resampler) = self {
            resampler.update_pcm_b_frequency(frequency);
        }
    }

    fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
        pcm_volume_shifts: [bool; 2],
        psg_volume_shift: u8,
        pcm_a_enabled: [bool; 2],
        pcm_b_enabled: [bool; 2],
    ) -> Result<(), A::Err> {
        match self {
            Self::Basic(resampler) => resampler.drain_audio_output(audio_output),
            Self::Interpolating(resampler) => resampler.drain_audio_output(
                audio_output,
                pcm_volume_shifts,
                psg_volume_shift,
                pcm_a_enabled,
                pcm_b_enabled,
            ),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Apu {
    enabled: bool,
    pcm_a: DirectSoundChannel,
    pcm_b: DirectSoundChannel,
    psg: Psg,
    psg_volume: u8,
    psg_volume_shift: u8,
    pwm: PwmControl,
    resampler: AudioResampler,
    output_frequency: u64,
    timer_frequencies: [Option<f64>; 2],
    cycles: u64,
    config: GbaAudioConfig,
}

impl Apu {
    pub fn new(config: GbaAudioConfig) -> Self {
        const DEFAULT_OUTPUT_FREQUENCY: u64 = 48000;

        Self {
            enabled: false,
            pcm_a: DirectSoundChannel::new("A".into()),
            pcm_b: DirectSoundChannel::new("B".into()),
            psg: Psg::new(),
            psg_volume: 2,
            psg_volume_shift: 0,
            pwm: PwmControl::new(),
            resampler: AudioResampler::new(
                config,
                PwmClockShift::default(),
                DEFAULT_OUTPUT_FREQUENCY,
                [None; 2],
            ),
            output_frequency: DEFAULT_OUTPUT_FREQUENCY,
            timer_frequencies: [None; 2],
            cycles: 0,
            config,
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        if cycles <= self.cycles {
            return;
        }

        let clock_shift = self.pwm.clock_shift.gba_clock_downshift();
        let pwm_samples_elapsed = (cycles >> clock_shift) - (self.cycles >> clock_shift);
        let psg_ticks = 1 << (21 - (24 - clock_shift));

        match self.config.audio_interpolation {
            GbaAudioInterpolation::NearestNeighbor => {
                for _ in 0..pwm_samples_elapsed {
                    for _ in 0..psg_ticks {
                        self.psg.tick_2mhz(self.enabled);
                    }

                    let (sample_l, sample_r) = self.generate_mixed_pwm_sample();
                    self.resampler.push_mixed_sample([sample_l, sample_r]);
                }
            }
            GbaAudioInterpolation::CubicHermite | GbaAudioInterpolation::WindowedSinc => {
                for _ in 0..pwm_samples_elapsed * psg_ticks {
                    self.psg.tick_2mhz(self.enabled);
                    self.resampler.push_psg(self.psg.sample(self.config.psg_channels_enabled()));
                }
            }
        }

        self.cycles = cycles;
    }

    fn generate_mixed_pwm_sample(&self) -> (f64, f64) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        let (mut pwm_l, mut pwm_r) = self.sample();

        let sample_downshift = self.pwm.clock_shift.mixed_sample_downshift();
        pwm_l >>= sample_downshift;
        pwm_r >>= sample_downshift;

        // Remap from [0x000, 0x3FF >> shift] to [0, 1]
        let max_sample = f64::from(0x3FF >> sample_downshift);
        let mut sample_l = f64::from(pwm_l) / max_sample;
        let mut sample_r = f64::from(pwm_r) / max_sample;

        // Remap from [0, 1] to [-1, +1], if only for volume purposes
        sample_l = 2.0 * (sample_l - 0.5);
        sample_r = 2.0 * (sample_r - 0.5);

        (sample_l, sample_r)
    }

    fn sample_pcm(&self, channels_enabled: [bool; 2]) -> (i16, i16) {
        let channels_enabled = channels_enabled.map(i16::from);

        // Left shift from signed 8-bit to signed 10-bit, then apply optional 50% volume reduction
        let raw_samples = [self.pcm_a.current_sample, self.pcm_b.current_sample].map(i16::from);
        let samples = [
            ((channels_enabled[0] * raw_samples[0]) << 2) >> self.pcm_a.volume_shift,
            ((channels_enabled[1] * raw_samples[1]) << 2) >> self.pcm_b.volume_shift,
        ];

        let l_enabled = [self.pcm_a.l_enabled, self.pcm_b.l_enabled].map(i16::from);
        let r_enabled = [self.pcm_a.r_enabled, self.pcm_b.r_enabled].map(i16::from);

        // PCM sum is clamped before mixing with PSG (tested on hardware)
        let pcm_l = (l_enabled[0] * samples[0] + l_enabled[1] * samples[1]).clamp(-0x200, 0x1FF);
        let pcm_r = (r_enabled[0] * samples[0] + r_enabled[1] * samples[1]).clamp(-0x200, 0x1FF);

        (pcm_l, pcm_r)
    }

    fn sample(&self) -> (u16, u16) {
        let (pcm_l, pcm_r) = self.sample_pcm(self.config.pcm_channels_enabled());

        // Mixed PSG sample is already signed 10-bit
        let (mut psg_l, mut psg_r) = self.psg.sample(self.config.psg_channels_enabled());
        psg_l >>= self.psg_volume_shift;
        psg_r >>= self.psg_volume_shift;

        // PCM + PSG sum is clamped to signed 10-bit (verified on hardware)
        // Then added with sound bias and clamped to unsigned 10-bit
        let final_mix = |pcm: i16, psg: i16| {
            let signed_sum = (pcm + psg).clamp(-0x200, 0x1FF);
            (signed_sum + self.pwm.sound_bias).clamp(0x000, 0x3FF) as u16
        };

        let sample_l = final_mix(pcm_l, psg_l);
        let sample_r = final_mix(pcm_r, psg_r);
        (sample_l, sample_r)
    }

    pub fn handle_timer_overflow(&mut self, timer_idx: usize, cycles: u64, dma: &mut DmaState) {
        self.step_to(cycles);

        let timer = match timer_idx {
            0 => DirectSoundTimer::Zero,
            1 => DirectSoundTimer::One,
            _ => return,
        };

        if self.pcm_a.timer == timer {
            if self.pcm_a.dma_request() {
                dma.notify_apu_fifo_a(cycles);
            }

            self.pcm_a.pop_fifo();
            self.resampler
                .push_pcm_a(self.pcm_a.current_sample * i8::from(self.config.pcm_a_enabled));
        }

        if self.pcm_b.timer == timer {
            if self.pcm_b.dma_request() {
                dma.notify_apu_fifo_b(cycles);
            }

            self.pcm_b.pop_fifo();
            self.resampler
                .push_pcm_b(self.pcm_b.current_sample * i8::from(self.config.pcm_b_enabled));
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn read_register(&self, address: u32) -> Option<u8> {
        let value = match address {
            0x4000060 => self.psg.read_sound1cnt_l(),
            0x4000061 => 0, // SOUND1CNT_L high
            0x4000062 => self.psg.read_sound1cnt_h_low(),
            0x4000063 => self.psg.read_sound1cnt_h_high(),
            0x4000064 => 0, // SOUND1CNT_X low
            0x4000065 => self.psg.read_sound1cnt_x_high(),
            0x4000066 => 0,
            0x4000067 => 0,
            0x4000068 => self.psg.read_sound2cnt_l_low(),
            0x4000069 => self.psg.read_sound2cnt_l_high(),
            0x400006A => 0,
            0x400006B => 0,
            0x400006C => 0, // SOUND2CNT_H low
            0x400006D => self.psg.read_sound2cnt_h_high(),
            0x400006E => 0,
            0x400006F => 0,
            0x4000070 => self.psg.read_sound3cnt_l(),
            0x4000071 => 0, // SOUND3CNT_L high
            0x4000072 => 0, // SOUND3CNT_H low
            0x4000073 => self.psg.read_sound3cnt_h_high(),
            0x4000074 => 0, // SOUND3CNT_X low
            0x4000075 => self.psg.read_sound3cnt_x_high(),
            0x4000076 => 0,
            0x4000077 => 0,
            0x4000078 => 0, // SOUND4CNT_L low
            0x4000079 => self.psg.read_sound4cnt_l_high(),
            0x400007A => 0,
            0x400007B => 0,
            0x400007C => self.psg.read_sound4cnt_h_low(),
            0x400007D => self.psg.read_sound4cnt_h_high(),
            0x400007E => 0,
            0x400007F => 0,
            0x4000080 => self.psg.read_soundcnt_l_low(),
            0x4000081 => self.psg.read_soundcnt_l_high(),
            0x4000082 => self.read_soundcnt_h_low(),
            0x4000083 => self.read_soundcnt_h_high(),
            0x4000084 => self.read_soundcnt_x(),
            0x4000085 => 0, // SOUNDCNT_X high
            0x4000086 => 0,
            0x4000087 => 0,
            0x4000088 => self.read_soundbias_low(),
            0x4000089 => self.read_soundbias_high(),
            0x400008A => 0,
            0x400008B => 0,
            0x4000090..=0x400009F => self.psg.read_wave_ram(address),
            _ => {
                log::debug!("Unimplemented APU register read: {address:08X}");
                return None;
            }
        };

        Some(value)
    }

    pub fn read_register_halfword(&mut self, address: u32) -> Option<u16> {
        let lsb = self.read_register(address)?;
        let msb = self.read_register(address | 1)?;
        Some(u16::from_le_bytes([lsb, msb]))
    }

    #[allow(clippy::match_same_arms)]
    pub fn write_register(&mut self, address: u32, value: u8) {
        // Registers outside of $4000082-$4000089 are not writable when APU is disabled
        if !self.enabled && !(0x4000082..0x400008A).contains(&address) {
            return;
        }

        match address {
            0x4000060 => self.psg.write_sound1cnt_l(value),
            0x4000061 => {} // SOUND1CNT_L high
            0x4000062 => self.psg.write_sound1cnt_h_low(value),
            0x4000063 => self.psg.write_sound1cnt_h_high(value),
            0x4000064 => self.psg.write_sound1cnt_x_low(value),
            0x4000065 => self.psg.write_sound1cnt_x_high(value),
            0x4000068 => self.psg.write_sound2cnt_l_low(value),
            0x4000069 => self.psg.write_sound2cnt_l_high(value),
            0x400006C => self.psg.write_sound2cnt_h_low(value),
            0x400006D => self.psg.write_sound2cnt_h_high(value),
            0x4000070 => self.psg.write_sound3cnt_l(value),
            0x4000071 => {} // SOUND3CNT_L high
            0x4000072 => self.psg.write_sound3cnt_h_low(value),
            0x4000073 => self.psg.write_sound3cnt_h_high(value),
            0x4000074 => self.psg.write_sound3cnt_x_low(value),
            0x4000075 => self.psg.write_sound3cnt_x_high(value),
            0x4000078 => self.psg.write_sound4cnt_l_low(value),
            0x4000079 => self.psg.write_sound4cnt_l_high(value),
            0x400007C => self.psg.write_sound4cnt_h_low(value),
            0x400007D => self.psg.write_sound4cnt_h_high(value),
            0x4000080 => self.psg.write_soundcnt_l_low(value),
            0x4000081 => self.psg.write_soundcnt_l_high(value),
            0x4000082 => self.write_soundcnt_h_low(value),
            0x4000083 => self.write_soundcnt_h_high(value),
            0x4000084 => self.write_soundcnt_x(value),
            0x4000085 => {} // SOUNDCNT_X high
            0x4000088 => self.write_soundbias_low(value),
            0x4000089 => self.write_soundbias_high(value),
            0x4000090..=0x400009F => self.psg.write_wave_ram(address, value),
            0x40000A0..=0x40000A3 => self.pcm_a.fifo.push_byte(address, value),
            0x40000A4..=0x40000A7 => self.pcm_b.fifo.push_byte(address, value),
            0x40000A8..=0x40000AF => {} // Unused
            _ => {
                log::debug!("Unimplemented APU register write: {address:08X} {value:02X}");
            }
        }
    }

    pub fn write_register_halfword(&mut self, address: u32, value: u16) {
        match address & !1 {
            0x40000A0 | 0x40000A2 => self.pcm_a.fifo.push_halfword(address, value),
            0x40000A4 | 0x40000A6 => self.pcm_b.fifo.push_halfword(address, value),
            address => {
                let [lsb, msb] = value.to_le_bytes();
                self.write_register(address, lsb);
                self.write_register(address | 1, msb);
            }
        }
    }

    pub fn write_register_word(&mut self, address: u32, value: u32) {
        match address & !3 {
            0x40000A0 => self.pcm_a.fifo.push(value),
            0x40000A4 => self.pcm_b.fifo.push(value),
            address => {
                let bytes = value.to_le_bytes();
                for (i, byte) in bytes.into_iter().enumerate() {
                    let i = i as u32;
                    self.write_register(address | i, byte);
                }
            }
        }
    }

    // $4000082: SOUNDCNT_H low byte (GBA-specific volume control)
    fn write_soundcnt_h_low(&mut self, value: u8) {
        self.psg_volume = value & 3;

        // PSG volume 3 "prohibited" functions same as 0 (tested on hardware)
        self.psg_volume_shift = 2 - (self.psg_volume % 3);

        self.pcm_a.volume_shift = 1 - ((value >> 2) & 1);
        self.pcm_b.volume_shift = 1 - ((value >> 3) & 1);

        log::trace!("SOUNDCNT_H low write: {value:02X}");
        log::trace!("  PSG volume: {}%", 100 >> self.psg_volume_shift);
        log::trace!("  PCM A volume: {}%", 100 >> self.pcm_a.volume_shift);
        log::trace!("  PCM B volume: {}%", 100 >> self.pcm_b.volume_shift);
    }

    // $4000082: SOUNDCNT_H low byte (GBA-specific volume control)
    fn read_soundcnt_h_low(&self) -> u8 {
        (self.psg_volume)
            | ((1 - self.pcm_a.volume_shift) << 2)
            | ((1 - self.pcm_b.volume_shift) << 3)
    }

    // $4000083: SOUNDCNT_H high byte (Direct Sound mixing and timer control)
    fn write_soundcnt_h_high(&mut self, value: u8) {
        self.pcm_a.r_enabled = value.bit(0);
        self.pcm_a.l_enabled = value.bit(1);
        self.pcm_a.timer = DirectSoundTimer::from_bit(value.bit(2));

        if value.bit(3) {
            self.pcm_a.reset_fifo();
        }

        self.pcm_b.r_enabled = value.bit(4);
        self.pcm_b.l_enabled = value.bit(5);
        self.pcm_b.timer = DirectSoundTimer::from_bit(value.bit(6));

        if value.bit(7) {
            self.pcm_b.reset_fifo();
        }

        self.resampler.update_pcm_a_frequency(self.timer_frequencies[self.pcm_a.timer as usize]);
        self.resampler.update_pcm_b_frequency(self.timer_frequencies[self.pcm_b.timer as usize]);

        log::trace!("SOUNDCNT_H high write: {value:02X}");
        log::trace!("  PCM A stereo enabled: [{}, {}]", self.pcm_a.l_enabled, self.pcm_a.r_enabled);
        log::trace!("  PCM A timer: {:?}", self.pcm_a.timer);
        log::trace!("  PCM A reset: {}", value.bit(3));
        log::trace!("  PCM B stereo enabled: [{}, {}]", self.pcm_b.l_enabled, self.pcm_b.r_enabled);
        log::trace!("  PCM B timer: {:?}", self.pcm_b.timer);
        log::trace!("  PCM B reset: {}", value.bit(7));
    }

    // $4000083: SOUNDCNT_H high byte (Direct Sound mixing and timer control)
    fn read_soundcnt_h_high(&self) -> u8 {
        u8::from(self.pcm_a.r_enabled)
            | (u8::from(self.pcm_a.l_enabled) << 1)
            | ((self.pcm_a.timer as u8) << 2)
            | (u8::from(self.pcm_b.r_enabled) << 4)
            | (u8::from(self.pcm_b.l_enabled) << 5)
            | ((self.pcm_b.timer as u8) << 6)
    }

    // $4000084: SOUNDCNT_X / NR52 (channel enabled status, enable/disable APU)
    fn write_soundcnt_x(&mut self, value: u8) {
        self.enabled = value.bit(7);

        if !self.enabled {
            // Disabling the APU resets all PSG registers to 0
            self.psg.disable();
            self.pcm_a.reset_fifo();
            self.pcm_b.reset_fifo();
            self.pcm_a.current_sample = 0;
            self.pcm_b.current_sample = 0;
        }

        log::trace!("SOUNDCNT_X write: {value:02X}");
        log::trace!("  APU enabled: {}", self.enabled);
    }

    // $4000084: SOUNDCNT_X / NR52 (channel enabled status, enable/disable APU)
    fn read_soundcnt_x(&self) -> u8 {
        self.psg.read_soundcnt_x(self.enabled)
    }

    // $4000088: SOUNDBIAS low byte (sound bias lowest 7 bits)
    fn write_soundbias_low(&mut self, value: u8) {
        self.pwm.sound_bias = (self.pwm.sound_bias & !0xFF) | i16::from(value & !1);

        log::trace!("SOUNDBIAS low write: {value:02X}");
        log::trace!("  PWM sound bias: {:03X}", self.pwm.sound_bias);
    }

    // $4000088: SOUNDBIAS low byte (sound bias lowest 7 bits)
    fn read_soundbias_low(&self) -> u8 {
        self.pwm.sound_bias as u8
    }

    // $4000089: SOUNDBIAS high byte (PWM sampling cycle, sound bias highest 2 bits)
    fn write_soundbias_high(&mut self, value: u8) {
        self.pwm.sound_bias = (self.pwm.sound_bias & 0xFF) | (i16::from(value & 3) << 8);
        self.pwm.clock_shift = PwmClockShift::from_bits(value >> 6);

        self.resampler.update_source_frequency(self.pwm.clock_shift);

        log::trace!("SOUNDBIAS high write: {value:02X}");
        log::trace!("  PWM sound bias: {:03X}", self.pwm.sound_bias);
        log::trace!("  PWM sample rate: {} Hz", self.pwm.clock_shift.source_frequency());
    }

    // $4000089: SOUNDBIAS high byte (PWM sampling cycle, sound bias highest 2 bits)
    fn read_soundbias_high(&self) -> u8 {
        ((self.pwm.sound_bias >> 8) as u8) | ((self.pwm.clock_shift as u8) << 6)
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.resampler.update_output_frequency(output_frequency);
    }

    pub fn drain_audio_output<A: AudioOutput>(
        &mut self,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.resampler.drain_audio_output(
            audio_output,
            [self.pcm_a.volume_shift != 0, self.pcm_b.volume_shift != 0],
            self.psg_volume_shift,
            [self.pcm_a.l_enabled, self.pcm_a.r_enabled],
            [self.pcm_b.l_enabled, self.pcm_b.r_enabled],
        )
    }

    pub fn reload_config(&mut self, config: GbaAudioConfig) {
        let prev_interpolation = self.config.audio_interpolation;
        self.config = config;

        if prev_interpolation != self.config.audio_interpolation {
            self.resampler = AudioResampler::new(
                self.config,
                self.pwm.clock_shift,
                self.output_frequency,
                [
                    self.timer_frequencies[self.pcm_a.timer as usize],
                    self.timer_frequencies[self.pcm_b.timer as usize],
                ],
            );
        } else if let AudioResampler::Interpolating(resampler) = &mut self.resampler {
            resampler.update_psg_low_pass(config.psg_low_pass);
        }
    }

    pub fn notify_timer_frequency_update(&mut self, timer_idx: u8, frequency: Option<f64>) {
        self.timer_frequencies[timer_idx as usize] = frequency;

        if self.pcm_a.timer as u8 == timer_idx {
            self.resampler.update_pcm_a_frequency(frequency);
        }

        if self.pcm_b.timer as u8 == timer_idx {
            self.resampler.update_pcm_b_frequency(frequency);
        }
    }
}
