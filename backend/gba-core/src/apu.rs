//! GBA APU (audio processing unit)
//!
//! The GBA APU contains the 4 channels from the Game Boy Color APU plus 2 new Direct Sound channels
//! that play 8-bit PCM samples. The GBC APU channels are unchanged except for the custom wave channel,
//! which now has twice as much wavetable RAM (split into two banks of 16 bytes each).
//!
//! Actual hardware outputs audio using 1-bit PWM at ~16.77 MHz. This is expensive to emulate, so
//! instead audio output is emulated as PCM at the configured sample rate (ranges from 32.768 KHz
//! to 262.144 KHz).

use bincode::{Decode, Encode};
use jgenesis_common::frontend::AudioOutput;
use std::collections::VecDeque;

pub const FIFO_A_ADDRESS: u32 = 0x040000A0;
pub const FIFO_B_ADDRESS: u32 = 0x040000A4;

const FIFO_LEN: usize = 32;
const DEFAULT_BIAS: u16 = 0x200;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum OutputSampleBits {
    // 32.768 KHz, 9-bit samples
    #[default]
    Nine = 0,
    // 65.536 KHz, 8-bit samples
    Eight = 1,
    // 131.072 KHz, 7-bit samples
    Seven = 2,
    // 262.144 KHz, 6-bit samples
    Six = 3,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Apu {
    channel_a: DirectSoundChannel,
    channel_b: DirectSoundChannel,
    sound_bias: u16,
    output_sample_bits: OutputSampleBits,
    sample_counter: u32,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            channel_a: DirectSoundChannel::new(),
            channel_b: DirectSoundChannel::new(),
            sound_bias: DEFAULT_BIAS,
            output_sample_bits: OutputSampleBits::default(),
            sample_counter: 0,
        }
    }

    pub fn tick<A: AudioOutput>(
        &mut self,
        cycles: u32,
        audio_output: &mut A,
    ) -> Result<(), A::Err> {
        self.sample_counter += cycles * 65536;
        while self.sample_counter >= 1 << 24 {
            self.sample_counter -= 1 << 24;

            let sample =
                i16::from(self.channel_a.current_sample) + i16::from(self.channel_b.current_sample);
            let sample = f64::from(sample) / 256.0;

            audio_output.push_sample(sample, sample)?;
        }

        Ok(())
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        match address & 0xFF {
            0xA0 | 0xA2 => {
                self.channel_a.fifo.push_halfword(value);
                log::trace!("FIFO A push: {value:04X}");
            }
            0xA4 | 0xA6 => {
                self.channel_b.fifo.push_halfword(value);
                log::trace!("FIFO B push: {value:04X}");
            }
            _ => {
                log::error!("APU register write {address:08X} {value:04X}");
            }
        }
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
}
