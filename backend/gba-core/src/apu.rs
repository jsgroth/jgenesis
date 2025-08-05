//! GBA APU (audio processing unit)
//!
//! Contains the 4 Game Boy Color APU channels (slightly modified) plus two 8-bit PCM channels (Direct Sound)

mod psg;

use crate::apu::psg::Psg;
use crate::audio::GbaAudioResampler;
use crate::dma::DmaState;
use bincode::{Decode, Encode};
use jgenesis_common::define_bit_enum;
use jgenesis_common::frontend::AudioOutput;
use jgenesis_common::num::GetBit;
use std::collections::VecDeque;

pub const FIFO_A_ADDRESS: u32 = 0x40000A0;
pub const FIFO_B_ADDRESS: u32 = 0x40000A4;

const FIFO_LEN_SAMPLES: usize = 32;

define_bit_enum!(DirectSoundTimer, [Zero, One]);

#[derive(Debug, Clone, Encode, Decode)]
struct DirectSoundChannel {
    name: String,
    fifo: VecDeque<i8>,
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
            fifo: VecDeque::with_capacity(FIFO_LEN_SAMPLES),
            current_sample: 0,
            volume_shift: 1,
            l_enabled: false,
            r_enabled: false,
            timer: DirectSoundTimer::default(),
        }
    }

    fn try_push_fifo(&mut self, sample: i8) {
        if self.fifo.len() == FIFO_LEN_SAMPLES {
            // TODO what happens when pushing into a full FIFO?
            log::warn!("Push into FIFO {} while full", self.name);
            return;
        }

        self.fifo.push_back(sample);
    }

    fn pop_fifo(&mut self) {
        self.current_sample = self.fifo.pop_front().unwrap_or(self.current_sample);
    }

    fn reset_fifo(&mut self) {
        // TODO what does this actually do?
        self.fifo.clear();
        self.current_sample = 0;
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
pub struct Apu {
    enabled: bool,
    pcm_a: DirectSoundChannel,
    pcm_b: DirectSoundChannel,
    psg: Psg,
    psg_volume: u8,
    psg_volume_shift: u8,
    pwm: PwmControl,
    resampler: GbaAudioResampler,
    cycles: u64,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            enabled: false,
            pcm_a: DirectSoundChannel::new("A".into()),
            pcm_b: DirectSoundChannel::new("B".into()),
            psg: Psg::new(),
            psg_volume: 0,
            psg_volume_shift: 2,
            pwm: PwmControl::new(),
            resampler: GbaAudioResampler::new(),
            cycles: 0,
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        if cycles <= self.cycles {
            return;
        }

        let clock_shift = self.pwm.clock_shift.gba_clock_downshift();
        let pwm_samples_elapsed = (cycles >> clock_shift) - (self.cycles >> clock_shift);

        for _ in 0..pwm_samples_elapsed {
            let psg_ticks = 1 << (20 - (24 - clock_shift));
            for _ in 0..psg_ticks {
                self.psg.tick_1mhz(self.enabled);
            }

            self.generate_sample();
        }

        self.cycles = cycles;
    }

    fn generate_sample(&mut self) {
        if !self.enabled {
            self.resampler.collect_sample(0.0, 0.0);
            return;
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

        self.resampler.collect_sample(sample_l, sample_r);
    }

    fn sample(&self) -> (u16, u16) {
        // Mixed PSG samples are unsigned 9-bit (4 unsigned 4-bit channels, 3-bit master volume)
        // Shift to unsigned 10-bit, then downshift based on volume
        let (mut psg_l, mut psg_r) = self.psg.sample();
        psg_l = (psg_l << 1) >> self.psg_volume_shift;
        psg_r = (psg_r << 1) >> self.psg_volume_shift;

        let psg_l = psg_l as i16;
        let psg_r = psg_r as i16;

        // PCM samples are signed 8-bit
        // Shift to signed 10-bit, then downshift based on volume
        let pcm_a = (i16::from(self.pcm_a.current_sample) << 2) >> self.pcm_a.volume_shift;
        let pcm_b = (i16::from(self.pcm_b.current_sample) << 2) >> self.pcm_b.volume_shift;

        let pcm_l =
            i16::from(self.pcm_a.l_enabled) * pcm_a + i16::from(self.pcm_b.l_enabled) * pcm_b;
        let pcm_r =
            i16::from(self.pcm_a.r_enabled) * pcm_a + i16::from(self.pcm_b.r_enabled) * pcm_b;

        // Final results are clamped to unsigned 10-bit after adding sound bias
        let sample_l = (psg_l + pcm_l + self.pwm.sound_bias).clamp(0x000, 0x3FF) as u16;
        let sample_r = (psg_r + pcm_r + self.pwm.sound_bias).clamp(0x000, 0x3FF) as u16;
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
            self.pcm_a.pop_fifo();

            if self.dma_request_a() {
                dma.notify_apu_fifo_a();
            }
        }

        if self.pcm_b.timer == timer {
            self.pcm_b.pop_fifo();

            if self.dma_request_b() {
                dma.notify_apu_fifo_b();
            }
        }
    }

    pub fn dma_request_a(&self) -> bool {
        self.enabled && self.pcm_a.fifo.len() <= FIFO_LEN_SAMPLES / 2
    }

    pub fn dma_request_b(&self) -> bool {
        self.enabled && self.pcm_b.fifo.len() <= FIFO_LEN_SAMPLES / 2
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
                log::warn!("Unimplemented APU register read: {address:08X}");
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
            0x40000A0..=0x40000A3 => self.pcm_a.try_push_fifo(value as i8),
            0x40000A4..=0x40000A7 => self.pcm_b.try_push_fifo(value as i8),
            0x40000A8..=0x40000AF => {} // Unused
            _ => {
                log::warn!("Unimplemented APU register write: {address:08X} {value:02X}");
            }
        }
    }

    pub fn write_register_halfword(&mut self, address: u32, value: u16) {
        let [lsb, msb] = value.to_le_bytes();
        self.write_register(address, lsb);
        self.write_register(address | 1, msb);
    }

    // $4000082: SOUNDCNT_H low byte (GBA-specific volume control)
    fn write_soundcnt_h_low(&mut self, value: u8) {
        self.psg_volume = value & 3;
        // TODO how should PSG volume 3 behave?
        self.psg_volume_shift = 2_u8.saturating_sub(self.psg_volume);
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

        self.resampler.update_source_frequency(self.pwm.clock_shift.source_frequency());

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
        self.resampler.drain_audio_output(audio_output)
    }
}
