//! Ricoh RF5C164 PCM sound chip

use crate::api::{PcmInterpolation, SegaCdEmulatorConfig};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use std::array;

// Divider of sub CPU cycles
const RF5C164_DIVIDER: u64 = 384;

const ADDRESS_FRACT_BITS: u32 = 11;
const ADDRESS_FRACT_MASK: u32 = (1 << ADDRESS_FRACT_BITS) - 1;

const WAVEFORM_RAM_LEN: usize = 64 * 1024;
const WAVEFORM_ADDRESS_MASK: u32 = WAVEFORM_RAM_LEN as u32 - 1;

const ADDRESS_FIXED_POINT_MASK: u32 = (1 << (16 + ADDRESS_FRACT_BITS)) - 1;

type WaveformRam = [u8; WAVEFORM_RAM_LEN];

#[derive(Debug, Clone, Default, Encode, Decode)]
struct InterpolationBuffer {
    buffer: [i8; 6],
}

impl InterpolationBuffer {
    fn clear(&mut self) {
        self.buffer.fill(0);
    }

    fn push(&mut self, sample: i8) {
        for i in 0..5 {
            self.buffer[i] = self.buffer[i + 1];
        }
        self.buffer[5] = sample;
    }

    fn sample(&self, interpolation: PcmInterpolation, current_address: u32) -> f64 {
        match interpolation {
            PcmInterpolation::None => self.buffer[5].into(),
            PcmInterpolation::Linear => {
                interpolate_linear(self.buffer[4], self.buffer[5], interpolation_x(current_address))
            }
            PcmInterpolation::CubicHermite => interpolate_cubic_4p(
                self.buffer[2..6].try_into().unwrap(),
                interpolation_x(current_address),
            ),
            PcmInterpolation::CubicHermite6Point => {
                interpolate_cubic_6p(self.buffer, interpolation_x(current_address))
            }
        }
    }
}

fn interpolation_x(address: u32) -> f64 {
    f64::from(address & ADDRESS_FRACT_MASK) / f64::from(1 << ADDRESS_FRACT_BITS)
}

fn interpolate_linear(y0: i8, y1: i8, x: f64) -> f64 {
    let y0: f64 = y0.into();
    let y1: f64 = y1.into();

    y0 * (1.0 - x) + y1 * x
}

// Clamp to [-127, 126] because samples are sign+magnitude, not signed 8-bit
// +127 is not a valid sample value because 0xFF is the loop end marker
const MIN_SAMPLE: f64 = -127.0;
const MAX_SAMPLE: f64 = 126.0;

fn interpolate_cubic_4p(samples: [i8; 4], x: f64) -> f64 {
    let result = jgenesis_common::audio::interpolate_cubic_hermite_4p(samples.map(f64::from), x);
    result.clamp(MIN_SAMPLE, MAX_SAMPLE)
}

fn interpolate_cubic_6p(samples: [i8; 6], x: f64) -> f64 {
    let result = jgenesis_common::audio::interpolate_cubic_hermite_6p(samples.map(f64::from), x);
    result.clamp(MIN_SAMPLE, MAX_SAMPLE)
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Channel {
    enabled: bool,
    start_address: u16,
    loop_address: u16,
    master_volume: u8,
    l_volume: u8,
    r_volume: u8,
    // Fixed point 16.11
    current_address: u32,
    // Fixed point 5.11
    address_increment: u16,
    interpolation_buffer: InterpolationBuffer,
}

impl Channel {
    fn enable(&mut self, waveform_ram: &WaveformRam) {
        if !self.enabled {
            self.current_address = u32::from(self.start_address) << ADDRESS_FRACT_BITS;
            self.interpolation_buffer.clear();
            self.enabled = true;

            // Immediately read the first sample when a channel is enabled; otherwise it will get skipped
            let first_sample = waveform_ram[self.start_address as usize];
            if first_sample != 0xFF {
                self.interpolation_buffer.push(sign_magnitude_to_pcm(first_sample));
            }
        }
    }

    fn disable(&mut self) {
        self.enabled = false;
    }

    fn clock(&mut self, waveform_ram: &WaveformRam) {
        if !self.enabled {
            return;
        }

        let address_increment: u32 = self.address_increment.into();
        let incremented_address = self.current_address + address_increment;

        let mut address = self.current_address >> ADDRESS_FRACT_BITS;
        let steps = (incremented_address >> ADDRESS_FRACT_BITS) - address;
        if steps == 0 {
            // Only the fractional bits changed; no new samples read
            self.current_address = incremented_address & ADDRESS_FIXED_POINT_MASK;
            return;
        }

        // All steps but last
        for _ in 0..steps - 1 {
            address = (address + 1) & WAVEFORM_ADDRESS_MASK;
            let sample = waveform_ram[address as usize];
            if sample == 0xFF {
                // Loop signal
                // Actual hardware would skip over this, so just ignore it.
                // This shouldn't really happen in practice unless a game puts multiple loop markers
                // at the end of a sample while playing at >32552 Hz to guarantee that the chip
                // doesn't miss the loop.
                continue;
            }

            self.interpolation_buffer.push(sign_magnitude_to_pcm(sample));
        }

        // Last step
        address = (address + 1) & WAVEFORM_ADDRESS_MASK;
        let sample = waveform_ram[address as usize];
        if sample == 0xFF {
            // Loop signal; jump to start of loop and immediately read the next sample
            address = self.loop_address.into();
            let loop_start_sample = waveform_ram[self.loop_address as usize];
            if loop_start_sample == 0xFF {
                // Infinite loop
                // TODO what does actual hardware do when there's an infinite loop?
                self.interpolation_buffer.push(0);
            } else {
                self.interpolation_buffer.push(sign_magnitude_to_pcm(loop_start_sample));
            }
        } else {
            self.interpolation_buffer.push(sign_magnitude_to_pcm(sample));
        }

        let new_address_int = address & WAVEFORM_ADDRESS_MASK;
        let new_address_fract = incremented_address & ADDRESS_FRACT_MASK;
        self.current_address = (new_address_int << ADDRESS_FRACT_BITS) | new_address_fract;
    }

    fn sample(&self, interpolation: PcmInterpolation) -> (i32, i32) {
        if !self.enabled {
            return (0, 0);
        }

        let sample = self.interpolation_buffer.sample(interpolation, self.current_address);
        let sign = sample.signum() as i32;
        let magnitude = sample.abs();

        // Apply volume
        let amplified = magnitude * f64::from(self.master_volume);
        let panned_l = amplified * f64::from(self.l_volume);
        let panned_r = amplified * f64::from(self.r_volume);

        // Drop the lowest 5 bits and apply sign
        // Per the RF5C164 datasheet, the truncation is done purely on the magnitude, before taking
        // sign into account
        let output_l = sign * ((panned_l.round() as i32) >> 5);
        let output_r = sign * ((panned_r.round() as i32) >> 5);

        (output_l, output_r)
    }
}

fn sign_magnitude_to_pcm(sample: u8) -> i8 {
    // RF5C164 samples have a sign bit and a 7-bit magnitude
    // Sign bit 1 = Positive, 0 = Negative
    let magnitude = (sample & 0x7F) as i8;
    if sample.bit(7) { magnitude } else { -magnitude }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Rf5c164 {
    enabled: bool,
    channels: [Channel; 8],
    waveform_ram: Box<WaveformRam>,
    waveform_ram_bank: u8,
    selected_channel: u8,
    divider: u64,
    interpolation: PcmInterpolation,
}

impl Rf5c164 {
    pub fn new(config: &SegaCdEmulatorConfig) -> Self {
        Self {
            enabled: false,
            channels: array::from_fn(|_| Channel::default()),
            waveform_ram: vec![0; WAVEFORM_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            waveform_ram_bank: 0,
            selected_channel: 0,
            divider: RF5C164_DIVIDER,
            interpolation: config.pcm_interpolation,
        }
    }

    pub fn read(&self, address: u32) -> u8 {
        match address {
            0x0000..=0x0007 | 0x0009..=0x000F | 0x0020..=0x0FFF => {
                // Unused for reads
                0x00
            }
            0x0008 => self.read_channel_on_register(),
            0x0010..=0x001F => self.read_channel_address(address),
            0x1000..=0x1FFF => {
                // Reading waveform RAM is only allowed while the chip is not running
                if !self.enabled {
                    let waveform_ram_addr =
                        (u32::from(self.waveform_ram_bank) << 12) | (address & 0x0FFF);
                    self.waveform_ram[waveform_ram_addr as usize]
                } else {
                    0x00
                }
            }
            _ => panic!("invalid RF5C164 address: {address:06X}"),
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        match address {
            0x0000..=0x0008 => {
                self.write_register(address, value);
            }
            0x0009..=0x0FFF => {
                // Unused
            }
            0x1000..=0x1FFF => {
                let waveform_ram_addr =
                    (u32::from(self.waveform_ram_bank) << 12) | (address & 0x0FFF);
                self.waveform_ram[waveform_ram_addr as usize] = value;
            }
            _ => panic!("invalid RF5C164 address: {address:06X}"),
        }
    }

    pub fn dma_write(&mut self, address: u32, value: u8) {
        let waveform_ram_addr = (u32::from(self.waveform_ram_bank) << 12) | address;
        self.waveform_ram[waveform_ram_addr as usize] = value;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    fn read_channel_on_register(&self) -> u8 {
        log::trace!("Channel on/off register read");

        self.channels
            .iter()
            .enumerate()
            .map(|(i, channel)| u8::from(channel.enabled) << i)
            .reduce(|a, b| a | b)
            .unwrap()
    }

    fn read_channel_address(&self, address: u32) -> u8 {
        let channel_idx = (address & 0xF) >> 1;
        let channel = &self.channels[channel_idx as usize];
        let channel_address = if channel.enabled {
            (channel.current_address >> ADDRESS_FRACT_BITS) as u16
        } else {
            channel.start_address
        };

        log::trace!("Channel {channel_idx} address read; current address = {channel_address:04X}");

        if address.bit(0) {
            // High byte
            channel_address.msb()
        } else {
            // Low byte
            channel_address.lsb()
        }
    }

    fn write_register(&mut self, address: u32, value: u8) {
        log::trace!(
            "PCM register: Wrote {value:02X} to {address:04X}, current channel is {}",
            self.selected_channel
        );

        match address {
            0x0000 => {
                // Envelope
                self.channels[self.selected_channel as usize].master_volume = value;

                log::trace!("  Master volume = {value:02X}");
            }
            0x0001 => {
                // Pan
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.l_volume = value & 0x0F;
                channel.r_volume = value >> 4;

                log::trace!(
                    "  L volume = {:02X}, R volume = {:02X}",
                    channel.l_volume,
                    channel.r_volume
                );
            }
            0x0002 => {
                // Address increment, low byte
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.address_increment.set_lsb(value);

                log::trace!("  Address increment = {:04X}", channel.address_increment);
            }
            0x0003 => {
                // Address increment, high byte
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.address_increment.set_msb(value);

                log::trace!("  Address increment = {:04X}", channel.address_increment);
            }
            0x0004 => {
                // Loop address, low byte
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.loop_address.set_lsb(value);

                log::trace!("  Loop address = {:04X}", channel.loop_address);
            }
            0x0005 => {
                // Loop address, high byte
                let channel = &mut self.channels[self.selected_channel as usize];
                channel.loop_address.set_msb(value);

                log::trace!("  Loop address = {:04X}", channel.loop_address);
            }
            0x0006 => {
                // Start address (low byte is always $00)
                self.channels[self.selected_channel as usize].start_address =
                    u16::from_be_bytes([value, 0x00]);

                log::trace!("  Start address = {value:02X}00");
            }
            0x0007 => {
                // Control register
                self.enabled = value.bit(7);

                log::trace!("Chip enabled = {}", self.enabled);

                // Bits 3-0 have different effects depending on the value of bit 6
                if value.bit(6) {
                    // Change selected channel (3 bits)
                    self.selected_channel = value & 0x07;

                    log::trace!("  Selected channel = {}", self.selected_channel);
                } else {
                    // Change waveform RAM bank (4 bits)
                    self.waveform_ram_bank = value & 0x0F;

                    log::trace!("  PCM waveform RAM bank = {:X}", self.waveform_ram_bank);
                }
            }
            0x0008 => {
                // Channel on/off register
                // 1 = Disabled, 0 = Enabled
                for (i, channel) in self.channels.iter_mut().enumerate() {
                    if value.bit(i as u8) {
                        channel.disable();
                    } else {
                        channel.enable(&self.waveform_ram);
                    }
                }
            }
            _ => panic!("invalid RF5C164 register address: {address:06X}"),
        }
    }

    pub fn tick(&mut self, mut sub_cpu_cycles: u64, mut audio_callback: impl FnMut((f64, f64))) {
        while sub_cpu_cycles >= self.divider {
            sub_cpu_cycles -= self.divider;
            self.divider = RF5C164_DIVIDER;

            if self.enabled {
                self.clock();
            }

            audio_callback(self.sample());
        }
        self.divider -= sub_cpu_cycles;
    }

    fn clock(&mut self) {
        for channel in &mut self.channels {
            channel.clock(&self.waveform_ram);
        }
    }

    pub fn sample(&self) -> (f64, f64) {
        if !self.enabled {
            return (0.0, 0.0);
        }

        let (sample_l, sample_r) = self
            .channels
            .iter()
            .map(|channel| channel.sample(self.interpolation))
            .fold((0, 0), |(sum_l, sum_r), (sample_l, sample_r)| {
                (sum_l + sample_l, sum_r + sample_r)
            });

        // Individual channel samples are effectively signed 15-bit after applying volume (and
        // dropping the lowest 5 bits)
        // Mixed output is clamped to signed 16-bit
        let sample_l = sample_l.clamp(i16::MIN.into(), i16::MAX.into());
        let sample_r = sample_r.clamp(i16::MIN.into(), i16::MAX.into());

        let sample_l = f64::from(sample_l) / -f64::from(i16::MIN);
        let sample_r = f64::from(sample_r) / -f64::from(i16::MIN);
        (sample_l, sample_r)
    }

    pub fn reload_config(&mut self, config: &SegaCdEmulatorConfig) {
        self.interpolation = config.pcm_interpolation;
    }
}
