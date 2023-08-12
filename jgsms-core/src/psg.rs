use crate::num::GetBit;
use std::array;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaveOutput {
    Positive,
    Negative,
    Zero,
}

impl WaveOutput {
    fn invert(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
            Self::Zero => Self::Zero,
        }
    }
}

impl From<WaveOutput> for f64 {
    fn from(value: WaveOutput) -> Self {
        match value {
            WaveOutput::Positive => 1.0,
            WaveOutput::Negative => -1.0,
            WaveOutput::Zero => 0.0,
        }
    }
}

#[derive(Debug, Clone)]
struct SquareWaveGenerator {
    counter: u16,
    current_output: WaveOutput,
    tone: u16,
    attenuation: u8,
}

// Each step up in attenuation decreases volume by 2dB, except for the step up to 15 which silences
// A delta of -2dB is equal to a multiplier of 10^(-1/10) ~= 0.7943
const ATTENUATION_TO_VOLUME: [f64; 16] = [
    1.0,
    0.7943282347242815,
    0.6309573444801932,
    0.5011872336272722,
    0.3981071705534972,
    0.3162277660168379,
    0.25118864315095796,
    0.19952623149688792,
    0.15848931924611132,
    0.1258925411794167,
    0.09999999999999998,
    0.07943282347242814,
    0.06309573444801932,
    0.05011872336272722,
    0.03981071705534972,
    0.0,
];

// The SMS2 clips the highest 3 volumes; 0.55 was chosen arbitrarily
const SMS2_ATTENUATION_TO_VOLUME: [f64; 16] = [
    0.55,
    0.55,
    0.55,
    0.5011872336272722,
    0.3981071705534972,
    0.3162277660168379,
    0.25118864315095796,
    0.19952623149688792,
    0.15848931924611132,
    0.1258925411794167,
    0.09999999999999998,
    0.07943282347242814,
    0.06309573444801932,
    0.05011872336272722,
    0.03981071705534972,
    0.0,
];

impl SquareWaveGenerator {
    fn new() -> Self {
        Self {
            counter: 0,
            current_output: WaveOutput::Negative,
            tone: 0,
            attenuation: 0x0F,
        }
    }

    fn update_tone_low_bits(&mut self, data: u8) {
        self.tone = (self.tone & 0xFFF0) | u16::from(data & 0x0F);
    }

    fn update_tone_high_bits(&mut self, data: u8) {
        self.tone = (self.tone & 0x000F) | (u16::from(data & 0x3F) << 4);
    }

    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.tone;
        } else {
            self.counter -= 1;
            if self.counter == 0 {
                self.counter = self.tone;
                // TODO hack - don't oscillate the wave when the channel is at ultrasonic frequencies
                if self.tone >= 5 {
                    self.current_output = self.current_output.invert();
                }
            }
        }
    }

    fn sample(&self, volume_table: &[f64; 16]) -> f64 {
        f64::from(self.current_output) * volume_table[self.attenuation as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoiseType {
    Periodic,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoiseReload {
    Value(u16),
    Tone2,
}

impl NoiseReload {
    fn from_noise_register(value: u8) -> Self {
        match value & 0x03 {
            0x00 => Self::Value(0x10),
            0x01 => Self::Value(0x20),
            0x02 => Self::Value(0x40),
            0x03 => Self::Tone2,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }

    fn value(self, tone2: u16) -> u16 {
        match self {
            Self::Value(value) => value,
            Self::Tone2 => tone2,
        }
    }
}

#[derive(Debug, Clone)]
struct NoiseGenerator {
    counter: u16,
    current_counter_output: WaveOutput,
    counter_reload: NoiseReload,
    lfsr: u16,
    current_lfsr_output: WaveOutput,
    noise_type: NoiseType,
    attenuation: u8,
}

const INITIAL_LFSR: u16 = 0x8000;

impl NoiseGenerator {
    fn new() -> Self {
        Self {
            counter: 0,
            current_counter_output: WaveOutput::Negative,
            counter_reload: NoiseReload::from_noise_register(0x00),
            lfsr: INITIAL_LFSR,
            current_lfsr_output: WaveOutput::Zero,
            noise_type: NoiseType::Periodic,
            attenuation: 0x0F,
        }
    }

    fn shift_lfsr(&mut self) {
        self.current_lfsr_output = if self.lfsr.bit(0) {
            WaveOutput::Positive
        } else {
            WaveOutput::Zero
        };

        let input_bit = match self.noise_type {
            NoiseType::Periodic => self.lfsr.bit(0),
            NoiseType::White => self.lfsr.bit(0) ^ self.lfsr.bit(3),
        };

        self.lfsr = (self.lfsr >> 1) | (u16::from(input_bit) << 15);
    }

    fn write_data(&mut self, data: u8) {
        self.counter_reload = NoiseReload::from_noise_register(data);
        self.noise_type = if data.bit(2) {
            NoiseType::White
        } else {
            NoiseType::Periodic
        };

        self.lfsr = INITIAL_LFSR;
    }

    fn clock(&mut self, tone2: u16) {
        if self.counter == 0 {
            self.counter = self.counter_reload.value(tone2);
        } else {
            self.counter -= 1;
            if self.counter == 0 {
                self.counter = self.counter_reload.value(tone2);
                self.current_counter_output = self.current_counter_output.invert();
                if self.current_counter_output == WaveOutput::Positive {
                    self.shift_lfsr();
                }
            }
        }
    }

    fn sample(&self, volume_table: &[f64; 16]) -> f64 {
        f64::from(self.current_lfsr_output) * volume_table[self.attenuation as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Register {
    Tone0,
    Tone1,
    Tone2,
    Noise,
    Volume0,
    Volume1,
    Volume2,
    Volume3,
}

impl Register {
    fn from_latch_byte(value: u8) -> Self {
        match value & 0x70 {
            0x00 => Self::Tone0,
            0x10 => Self::Volume0,
            0x20 => Self::Tone1,
            0x30 => Self::Volume1,
            0x40 => Self::Tone2,
            0x50 => Self::Volume2,
            0x60 => Self::Noise,
            0x70 => Self::Volume3,
            _ => unreachable!("value & 0x70 is always one of the above values"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsgTickEffect {
    None,
    Clocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PsgVersion {
    #[default]
    MasterSystem2,
    Standard,
}

impl Display for PsgVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MasterSystem2 => write!(f, "MasterSystem2"),
            Self::Standard => write!(f, "Standard"),
        }
    }
}

impl FromStr for PsgVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MasterSystem2" => Ok(Self::MasterSystem2),
            "Standard" => Ok(Self::Standard),
            _ => Err(format!("invalid PSG version string: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Psg {
    version: PsgVersion,
    square_wave_channels: [SquareWaveGenerator; 3],
    noise_channel: NoiseGenerator,
    latched_register: Register,
    divider: u8,
}

const PSG_DIVIDER: u8 = 16;

impl Psg {
    pub fn new(version: PsgVersion) -> Self {
        Self {
            version,
            square_wave_channels: array::from_fn(|_| SquareWaveGenerator::new()),
            noise_channel: NoiseGenerator::new(),
            latched_register: Register::Tone0,
            divider: PSG_DIVIDER,
        }
    }

    fn write_register_low_bits(&mut self, data: u8) {
        match self.latched_register {
            Register::Tone0 => {
                self.square_wave_channels[0].update_tone_low_bits(data);
            }
            Register::Tone1 => {
                self.square_wave_channels[1].update_tone_low_bits(data);
            }
            Register::Tone2 => {
                self.square_wave_channels[2].update_tone_low_bits(data);
            }
            Register::Noise => {
                self.noise_channel.write_data(data);
            }
            Register::Volume0 => {
                self.square_wave_channels[0].attenuation = data & 0x0F;
            }
            Register::Volume1 => {
                self.square_wave_channels[1].attenuation = data & 0x0F;
            }
            Register::Volume2 => {
                self.square_wave_channels[2].attenuation = data & 0x0F;
            }
            Register::Volume3 => {
                self.noise_channel.attenuation = data & 0x0F;
            }
        }
    }

    fn write_register_high_bits(&mut self, data: u8) {
        match self.latched_register {
            Register::Tone0 => {
                self.square_wave_channels[0].update_tone_high_bits(data);
            }
            Register::Tone1 => {
                self.square_wave_channels[1].update_tone_high_bits(data);
            }
            Register::Tone2 => {
                self.square_wave_channels[2].update_tone_high_bits(data);
            }
            _ => {
                self.write_register_low_bits(data);
            }
        }
    }

    pub fn write(&mut self, value: u8) {
        if value.bit(7) {
            // LATCH/DATA byte
            self.latched_register = Register::from_latch_byte(value);
            self.write_register_low_bits(value);
        } else {
            // DATA byte
            self.write_register_high_bits(value);
        }
    }

    pub fn tick(&mut self) -> PsgTickEffect {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = PSG_DIVIDER;

            for channel in &mut self.square_wave_channels {
                channel.clock();
            }
            self.noise_channel.clock(self.square_wave_channels[2].tone);

            PsgTickEffect::Clocked
        } else {
            PsgTickEffect::None
        }
    }

    pub fn sample(&self) -> f64 {
        // TODO rewrite to use integer arithmetic as much as possible
        let volume_table = match self.version {
            PsgVersion::MasterSystem2 => &SMS2_ATTENUATION_TO_VOLUME,
            PsgVersion::Standard => &ATTENUATION_TO_VOLUME,
        };
        let mixed_square = self
            .square_wave_channels
            .iter()
            .map(|channel| channel.sample(volume_table) * 0.5)
            .sum::<f64>();
        (mixed_square + self.noise_channel.sample(volume_table)) / 4.0
    }
}
