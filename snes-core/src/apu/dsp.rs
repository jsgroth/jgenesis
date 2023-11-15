use crate::apu::AudioRam;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::ops::Index;

const BRR_BLOCK_LEN: u16 = 9;

// From https://problemkaputt.github.io/fullsnes.htm#snesapudspbrrsamples
#[rustfmt::skip]
const GAUSSIAN_TABLE: &[i32; 512] = &[
    0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000,
    0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x001, 0x002, 0x002, 0x002, 0x002, 0x002,
    0x002, 0x002, 0x003, 0x003, 0x003, 0x003, 0x003, 0x004, 0x004, 0x004, 0x004, 0x004, 0x005, 0x005, 0x005, 0x005,
    0x006, 0x006, 0x006, 0x006, 0x007, 0x007, 0x007, 0x008, 0x008, 0x008, 0x009, 0x009, 0x009, 0x00A, 0x00A, 0x00A,
    0x00B, 0x00B, 0x00B, 0x00C, 0x00C, 0x00D, 0x00D, 0x00E, 0x00E, 0x00F, 0x00F, 0x00F, 0x010, 0x010, 0x011, 0x011,
    0x012, 0x013, 0x013, 0x014, 0x014, 0x015, 0x015, 0x016, 0x017, 0x017, 0x018, 0x018, 0x019, 0x01A, 0x01B, 0x01B,
    0x01C, 0x01D, 0x01D, 0x01E, 0x01F, 0x020, 0x020, 0x021, 0x022, 0x023, 0x024, 0x024, 0x025, 0x026, 0x027, 0x028,
    0x029, 0x02A, 0x02B, 0x02C, 0x02D, 0x02E, 0x02F, 0x030, 0x031, 0x032, 0x033, 0x034, 0x035, 0x036, 0x037, 0x038,
    0x03A, 0x03B, 0x03C, 0x03D, 0x03E, 0x040, 0x041, 0x042, 0x043, 0x045, 0x046, 0x047, 0x049, 0x04A, 0x04C, 0x04D,
    0x04E, 0x050, 0x051, 0x053, 0x054, 0x056, 0x057, 0x059, 0x05A, 0x05C, 0x05E, 0x05F, 0x061, 0x063, 0x064, 0x066,
    0x068, 0x06A, 0x06B, 0x06D, 0x06F, 0x071, 0x073, 0x075, 0x076, 0x078, 0x07A, 0x07C, 0x07E, 0x080, 0x082, 0x084,
    0x086, 0x089, 0x08B, 0x08D, 0x08F, 0x091, 0x093, 0x096, 0x098, 0x09A, 0x09C, 0x09F, 0x0A1, 0x0A3, 0x0A6, 0x0A8,
    0x0AB, 0x0AD, 0x0AF, 0x0B2, 0x0B4, 0x0B7, 0x0BA, 0x0BC, 0x0BF, 0x0C1, 0x0C4, 0x0C7, 0x0C9, 0x0CC, 0x0CF, 0x0D2,
    0x0D4, 0x0D7, 0x0DA, 0x0DD, 0x0E0, 0x0E3, 0x0E6, 0x0E9, 0x0EC, 0x0EF, 0x0F2, 0x0F5, 0x0F8, 0x0FB, 0x0FE, 0x101,
    0x104, 0x107, 0x10B, 0x10E, 0x111, 0x114, 0x118, 0x11B, 0x11E, 0x122, 0x125, 0x129, 0x12C, 0x130, 0x133, 0x137,
    0x13A, 0x13E, 0x141, 0x145, 0x148, 0x14C, 0x150, 0x153, 0x157, 0x15B, 0x15F, 0x162, 0x166, 0x16A, 0x16E, 0x172,
    0x176, 0x17A, 0x17D, 0x181, 0x185, 0x189, 0x18D, 0x191, 0x195, 0x19A, 0x19E, 0x1A2, 0x1A6, 0x1AA, 0x1AE, 0x1B2,
    0x1B7, 0x1BB, 0x1BF, 0x1C3, 0x1C8, 0x1CC, 0x1D0, 0x1D5, 0x1D9, 0x1DD, 0x1E2, 0x1E6, 0x1EB, 0x1EF, 0x1F3, 0x1F8,
    0x1FC, 0x201, 0x205, 0x20A, 0x20F, 0x213, 0x218, 0x21C, 0x221, 0x226, 0x22A, 0x22F, 0x233, 0x238, 0x23D, 0x241,
    0x246, 0x24B, 0x250, 0x254, 0x259, 0x25E, 0x263, 0x267, 0x26C, 0x271, 0x276, 0x27B, 0x280, 0x284, 0x289, 0x28E,
    0x293, 0x298, 0x29D, 0x2A2, 0x2A6, 0x2AB, 0x2B0, 0x2B5, 0x2BA, 0x2BF, 0x2C4, 0x2C9, 0x2CE, 0x2D3, 0x2D8, 0x2DC,
    0x2E1, 0x2E6, 0x2EB, 0x2F0, 0x2F5, 0x2FA, 0x2FF, 0x304, 0x309, 0x30E, 0x313, 0x318, 0x31D, 0x322, 0x326, 0x32B,
    0x330, 0x335, 0x33A, 0x33F, 0x344, 0x349, 0x34E, 0x353, 0x357, 0x35C, 0x361, 0x366, 0x36B, 0x370, 0x374, 0x379,
    0x37E, 0x383, 0x388, 0x38C, 0x391, 0x396, 0x39B, 0x39F, 0x3A4, 0x3A9, 0x3AD, 0x3B2, 0x3B7, 0x3BB, 0x3C0, 0x3C5,
    0x3C9, 0x3CE, 0x3D2, 0x3D7, 0x3DC, 0x3E0, 0x3E5, 0x3E9, 0x3ED, 0x3F2, 0x3F6, 0x3FB, 0x3FF, 0x403, 0x408, 0x40C,
    0x410, 0x415, 0x419, 0x41D, 0x421, 0x425, 0x42A, 0x42E, 0x432, 0x436, 0x43A, 0x43E, 0x442, 0x446, 0x44A, 0x44E,
    0x452, 0x455, 0x459, 0x45D, 0x461, 0x465, 0x468, 0x46C, 0x470, 0x473, 0x477, 0x47A, 0x47E, 0x481, 0x485, 0x488,
    0x48C, 0x48F, 0x492, 0x496, 0x499, 0x49C, 0x49F, 0x4A2, 0x4A6, 0x4A9, 0x4AC, 0x4AF, 0x4B2, 0x4B5, 0x4B7, 0x4BA,
    0x4BD, 0x4C0, 0x4C3, 0x4C5, 0x4C8, 0x4CB, 0x4CD, 0x4D0, 0x4D2, 0x4D5, 0x4D7, 0x4D9, 0x4DC, 0x4DE, 0x4E0, 0x4E3,
    0x4E5, 0x4E7, 0x4E9, 0x4EB, 0x4ED, 0x4EF, 0x4F1, 0x4F3, 0x4F5, 0x4F6, 0x4F8, 0x4FA, 0x4FB, 0x4FD, 0x4FF, 0x500,
    0x502, 0x503, 0x504, 0x506, 0x507, 0x508, 0x50A, 0x50B, 0x50C, 0x50D, 0x50E, 0x50F, 0x510, 0x511, 0x511, 0x512,
    0x513, 0x514, 0x514, 0x515, 0x516, 0x516, 0x517, 0x517, 0x517, 0x518, 0x518, 0x518, 0x518, 0x518, 0x519, 0x519,
];

// From https://problemkaputt.github.io/fullsnes.htm#snesapudspadsrgainenvelope
#[rustfmt::skip]
const ENVELOPE_RATE_TABLE: &[u16; 32] = &[
    u16::MAX, 2048, 1536, 1280,
    1024, 768, 640, 512,
    384, 320, 256, 192,
    160, 128, 96, 80,
    64, 48, 40, 32,
    24, 20, 16, 12,
    10, 8, 6, 5,
    4, 3, 2, 1,
];

// From Anomie's S-DSP doc
#[rustfmt::skip]
const ENVELOPE_OFFSET_TABLE: &[u16; 32] = &[
    u16::MAX, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
         0,
         0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum EnvelopeMode {
    Adsr,
    #[default]
    Gain,
}

impl EnvelopeMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Adsr } else { Self::Gain }
    }

    fn to_bit(self) -> bool {
        self == Self::Adsr
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum GainMode {
    #[default]
    Direct,
    Custom,
}

impl GainMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Custom } else { Self::Direct }
    }

    fn to_bit(self) -> bool {
        self == Self::Custom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum EnvelopePhase {
    Attack,
    Decay,
    Sustain,
    #[default]
    Release,
}

const BRR_BUFFER_LEN: usize = 12;

#[derive(Debug, Clone, Default, Encode, Decode)]
struct BrrRingBuffer {
    buffer: [i16; BRR_BUFFER_LEN],
    fill_idx: usize,
    sample_idx: usize,
}

impl BrrRingBuffer {
    fn reset(&mut self) {
        self.fill_idx = 0;
        self.sample_idx = 0;
    }

    fn write(&mut self, sample: i16) {
        self.buffer[self.fill_idx] = sample;
        self.fill_idx = (self.fill_idx + 1) % BRR_BUFFER_LEN;
    }

    fn shift_sample_idx(&mut self) {
        self.sample_idx = (self.sample_idx + 4) % BRR_BUFFER_LEN;
    }

    fn last_two_written_samples(&self) -> (i16, i16) {
        if self.fill_idx == 0 {
            (self.buffer[BRR_BUFFER_LEN - 2], self.buffer[BRR_BUFFER_LEN - 1])
        } else if self.fill_idx == 1 {
            (self.buffer[BRR_BUFFER_LEN - 1], self.buffer[0])
        } else {
            (self.buffer[self.fill_idx - 2], self.buffer[self.fill_idx - 1])
        }
    }
}

impl Index<u16> for BrrRingBuffer {
    type Output = i16;

    fn index(&self, index: u16) -> &Self::Output {
        let buffer_idx = (self.sample_idx + index as usize) % BRR_BUFFER_LEN;
        &self.buffer[buffer_idx]
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Voice {
    // Registers
    instrument_number: u8,
    sample_rate: u16,
    pitch_modulation_enabled: bool,
    envelope_mode: EnvelopeMode,
    attack_rate: u8,
    decay_rate: u8,
    sustain_rate: u8,
    sustain_level: u8,
    gain_mode: GainMode,
    // Meaning of gain value varies based on gain mode
    gain_value: u8,
    volume_l: i8,
    volume_r: i8,
    keyed_on: bool,
    keyed_off: bool,
    output_noise: bool,
    // Stored in full here to preserve the highest 2 bits which are R/W
    last_pitch_h_write: u8,
    // State
    brr_block_address: u16,
    brr_buffer: BrrRingBuffer,
    brr_decoder_idx: u16,
    pitch_counter: u16,
    envelope_level: u16,
    clipped_envelope_value: u16,
    envelope_phase: EnvelopePhase,
    current_sample: i16,
    restart_pending: bool,
    restart_delay_remaining: u8,
    end_flag_seen: bool,
}

impl Voice {
    fn write_pitch_low(&mut self, value: u8) {
        self.sample_rate = (self.sample_rate & 0xFF00) | u16::from(value);
    }

    fn write_pitch_high(&mut self, value: u8) {
        // Sample rate is 14 bits; drop the highest 2
        self.sample_rate = (self.sample_rate & 0x00FF) | (u16::from(value & 0x3F) << 8);

        // Preserve original value for register reads
        self.last_pitch_h_write = value;
    }

    fn write_adsr_low(&mut self, value: u8) {
        // 4-bit attack rate, 3-bit decay rate, and ADSR/Gain select bit
        self.attack_rate = value & 0x0F;
        self.decay_rate = (value >> 4) & 0x07;
        self.envelope_mode = EnvelopeMode::from_bit(value.bit(7));

        log::trace!(
            "  Attack rate: {:02X}, Decay rate: {:02X}, Envelope Mode: {:?}",
            self.attack_rate,
            self.decay_rate,
            self.envelope_mode
        );
    }

    fn read_adsr_low(&self) -> u8 {
        self.attack_rate | (self.decay_rate << 4) | (u8::from(self.envelope_mode.to_bit()) << 7)
    }

    fn write_adsr_high(&mut self, value: u8) {
        // 5-bit sustain rate, 3-bit sustain level
        self.sustain_rate = value & 0x1F;
        self.sustain_level = value >> 5;

        log::trace!(
            "  Sustain rate: {:02X}, Sustain level: {:02X}",
            self.sustain_rate,
            self.sustain_level
        );
    }

    fn read_adsr_high(&self) -> u8 {
        self.sustain_rate | (self.sustain_level << 5)
    }

    fn write_gain(&mut self, value: u8) {
        self.gain_mode = GainMode::from_bit(value.bit(7));
        self.gain_value = value & 0x7F;

        log::trace!("  Gain mode: {:?}, Gain value: {:02X}", self.gain_mode, self.gain_value);
    }

    fn read_gain(&self) -> u8 {
        self.gain_value | (u8::from(self.gain_mode.to_bit()) << 7)
    }

    fn read_envelope(&self) -> u8 {
        // VxENVX reads return the highest 7 bits of the 11-bit envelope value
        (self.envelope_level >> 4) as u8
    }

    fn read_output(&self) -> u8 {
        // VxOUTX reads return the highest 8 bits of the 15-bit sample value (pre-volume)
        (self.current_sample >> 7) as u8
    }

    fn write_key_on(&mut self, key_on: bool) {
        self.keyed_on = key_on;

        // Keying on immediately moves envelope to attack, resets envelope value, and restarts the channel
        if key_on {
            self.envelope_phase = EnvelopePhase::Attack;
            self.envelope_level = 0;
            self.restart_pending = true;
        }
    }

    fn write_key_off(&mut self, key_off: bool) {
        self.keyed_off = key_off;

        // Keying off immediately moves envelope to release (but does not modify envelope value or other state)
        if key_off {
            self.envelope_phase = EnvelopePhase::Release;
        }
    }

    fn soft_reset(&mut self) {
        // Soft reset keys off the voice and immediately sets envelope to 0
        self.write_key_off(true);
        self.envelope_level = 0;
    }

    fn clock(
        &mut self,
        registers: &DspRegisters,
        audio_ram: &AudioRam,
        prev_voice_sample: i16,
        noise_generator_output: i16,
    ) {
        if self.restart_pending {
            self.restart_pending = false;
            self.restart(registers, audio_ram);
        }

        if self.restart_delay_remaining != 0 {
            // Output empty sample
            self.current_sample = 0;

            // After 2 clocks, prevent the channel from starting if key off or soft reset is set
            if self.restart_delay_remaining <= 3 && (self.keyed_off || registers.soft_reset) {
                self.envelope_phase = EnvelopePhase::Release;
            }

            self.restart_delay_remaining -= 1;
            if self.restart_delay_remaining == 0 {
                // Reset BRR decoder state and decode the first 2 groups
                // Actual hardware decodes 3 groups here, but decoding 2 simplifies the ring buffer handling
                self.brr_buffer.reset();
                self.brr_decoder_idx = 0;

                for _ in 0..2 {
                    self.decode_brr_group(registers.sample_table_address, audio_ram);
                }
            }

            return;
        }

        let interpolated_sample = if self.output_noise {
            // Turning on noise for a voice replaces the output with the noise generator output,
            // but envelope is still used and all of the BRR decoding continues to run in the
            // background
            noise_generator_output
        } else {
            let interpolate_idx = self.pitch_counter >> 12;
            apply_gaussian_filter(GaussArgs {
                pitch_counter: self.pitch_counter,
                oldest: self.brr_buffer[interpolate_idx],
                older: self.brr_buffer[interpolate_idx + 1],
                old: self.brr_buffer[interpolate_idx + 2],
                sample: self.brr_buffer[interpolate_idx + 3],
            })
        };

        // TODO do this after multiplying by sample?
        self.clock_envelope(registers.global_counter);

        // Apply 11-bit envelope; still a signed 15-bit sample
        let sample =
            ((i32::from(interpolated_sample) * i32::from(self.envelope_level)) >> 11) as i16;
        self.current_sample = sample;

        self.pitch_counter += self.sample_rate;
        if self.pitch_modulation_enabled && !self.output_noise {
            // Adjust pitch based on previous voice's output
            let modulation_rate =
                ((i32::from(prev_voice_sample) >> 5) * i32::from(self.sample_rate)) >> 10;
            self.pitch_counter =
                self.pitch_counter.wrapping_add_signed(modulation_rate as i16).clamp(0, 0x7FFF);
        }

        if self.pitch_counter >= 0x4000 {
            self.pitch_counter -= 0x4000;
            self.decode_brr_group(registers.sample_table_address, audio_ram);
            self.brr_buffer.shift_sample_idx();
        }
    }

    fn restart(&mut self, registers: &DspRegisters, audio_ram: &AudioRam) {
        // Table address is located at DIR + 4*instrument
        let table_addr = registers
            .sample_table_address
            .wrapping_add(u16::from(self.instrument_number) << 2) as usize;

        // First 2 bytes of table entry hold start address
        let start_addr = u16::from_le_bytes([audio_ram[table_addr], audio_ram[table_addr + 1]]);
        self.brr_block_address = start_addr;
        self.pitch_counter = 0;

        // Each voice outputs 5 empty samples after a restart
        self.restart_delay_remaining = 5;

        self.end_flag_seen = false;
    }

    // Decode a group of 4 BRR samples, advancing to the next BRR block or looping if necessary
    fn decode_brr_group(&mut self, sample_table_address: u16, audio_ram: &AudioRam) {
        if self.brr_decoder_idx == 16 {
            // Advance to next block
            let prev_block_header = audio_ram[self.brr_block_address as usize];
            let prev_block_end_flag = prev_block_header.bit(0);
            if prev_block_end_flag {
                self.end_flag_seen = true;

                // Jump to loop address
                let table_addr = sample_table_address
                    .wrapping_add(u16::from(self.instrument_number) << 2)
                    as usize;
                let loop_addr =
                    u16::from_le_bytes([audio_ram[table_addr + 2], audio_ram[table_addr + 3]]);
                self.brr_block_address = loop_addr;
            } else {
                self.brr_block_address = self.brr_block_address.wrapping_add(BRR_BLOCK_LEN);
            }

            self.brr_decoder_idx = 0;
        }

        let header = audio_ram[self.brr_block_address as usize];
        let shift = header >> 4;
        let filter = (header >> 2) & 0x03;
        let loop_flag = header.bit(1);
        let end_flag = header.bit(0);

        // Immediately mute channel after decoding a header with end set and loop clear
        if end_flag && !loop_flag {
            self.envelope_phase = EnvelopePhase::Release;
            self.envelope_level = 0;
        }

        let mut brr_nibbles = [0_i8; 4];
        let decoder_idx = self.brr_decoder_idx;
        for i in 0..2 {
            let sample_addr = self.brr_block_address.wrapping_add(1 + (decoder_idx >> 1) + i);
            let sample_pair = audio_ram[sample_addr as usize];

            // High nibble stores the first sample, low nibble stores the second sample
            // Treat both as signed 4-bit integers
            let first_sample = (sample_pair as i8) >> 4;
            let second_sample = ((sample_pair as i8) << 4) >> 4;
            brr_nibbles[(2 * i) as usize] = first_sample;
            brr_nibbles[(2 * i + 1) as usize] = second_sample;
        }
        self.brr_decoder_idx += 4;

        let (mut older_sample, mut old_sample) = self.brr_buffer.last_two_written_samples();
        for brr_nibble in brr_nibbles {
            let shifted = apply_brr_shift(brr_nibble, shift);
            let brr_sample = apply_brr_filter(shifted, filter, old_sample, older_sample);
            self.brr_buffer.write(brr_sample);

            older_sample = old_sample;
            old_sample = brr_sample;
        }
    }

    fn clock_envelope(&mut self, global_counter: u16) {
        if self.envelope_phase == EnvelopePhase::Release {
            // Release ignores other settings and decrements envelope value by 8 every sample
            self.envelope_level = self.envelope_level.saturating_sub(8);
            self.clipped_envelope_value = self.envelope_level.wrapping_sub(8) & 0x7FF;
            return;
        }

        // ADSR transitions are checked every sample, even if envelope is in Gain mode
        if self.envelope_phase == EnvelopePhase::Attack && self.envelope_level >= 0x7E0 {
            self.envelope_phase = EnvelopePhase::Decay;
        }

        if self.envelope_phase == EnvelopePhase::Decay {
            let sustain_level = u16::from(self.sustain_level + 1) << 8;
            if self.envelope_level <= sustain_level {
                self.envelope_phase = EnvelopePhase::Sustain;
            }
        }

        let current_value: i32 = self.envelope_level.into();
        let (rate, step) = match (self.envelope_mode, self.gain_mode) {
            (EnvelopeMode::Gain, GainMode::Direct) => {
                // Force envelope level to 16*N
                let target_value = i32::from(self.gain_value) << 4;
                if current_value == target_value {
                    (0, 0)
                } else {
                    (31, target_value - current_value)
                }
            }
            (EnvelopeMode::Gain, GainMode::Custom) => {
                let rate = self.gain_value & 0x1F;
                let step = match self.gain_value & 0x60 {
                    0x00 => {
                        // Linear decrease (fixed decrement by 32)
                        -32
                    }
                    0x20 => {
                        // Exponential decrease
                        compute_exp_decay(current_value)
                    }
                    0x40 => {
                        // Linear increase (fixed increment by 32)
                        32
                    }
                    0x60 => {
                        // Bent increase (uses clipped value rather than clamped value)
                        if self.clipped_envelope_value < 0x600 { 32 } else { 8 }
                    }
                    _ => unreachable!("value & 0x60 is always one of the above values"),
                };

                (rate, step)
            }
            (EnvelopeMode::Adsr, _) => match self.envelope_phase {
                EnvelopePhase::Attack => {
                    let rate = (self.attack_rate << 1) | 0x01;
                    let step = if rate == 31 { 1024 } else { 32 };
                    (rate, step)
                }
                EnvelopePhase::Decay => {
                    let rate = 0x10 | (self.decay_rate << 1);
                    let step = compute_exp_decay(current_value);
                    (rate, step)
                }
                EnvelopePhase::Sustain => {
                    let step = compute_exp_decay(current_value);
                    (self.sustain_rate, step)
                }
                EnvelopePhase::Release => (31, -8),
            },
        };

        if rate != 0
            && (global_counter + ENVELOPE_OFFSET_TABLE[rate as usize])
                % ENVELOPE_RATE_TABLE[rate as usize]
                == 0
        {
            let new_value = current_value + step;
            self.envelope_level = new_value.clamp(0, 0x7FF) as u16;
            self.clipped_envelope_value = (new_value as u16) & 0x7FF;
        }
    }
}

fn apply_brr_shift(nibble: i8, shift: u8) -> i16 {
    match shift {
        0 => (nibble >> 1).into(),
        1..=12 => i16::from(nibble) << (shift - 1),
        13..=15 => {
            // "Invalid" shift values; resulting sample will always be either 0 or -2048 ($F800)
            if nibble < 0 { -2048 } else { 0 }
        }
        _ => panic!("invalid BRR shift value: {shift}"),
    }
}

fn apply_brr_filter(sample: i16, filter: u8, old: i16, older: i16) -> i16 {
    // Do math in 32 bits to avoid overflows
    let sample: i32 = sample.into();
    let old: i32 = old.into();
    let older: i32 = older.into();

    let filtered = match filter {
        // no filter
        0 => sample,
        // sample + 0.9375 * old
        1 => sample + old + (-old >> 4),
        // sample + 1.90625 * old - 0.9375 * older
        2 => sample + (old << 1) + (-(3 * old) >> 5) - older + (older >> 4),
        // sample + 1.796875 * old - 0.8125 * older
        3 => sample + (old << 1) + (-(13 * old) >> 6) - older + ((3 * older) >> 4),
        _ => panic!("invalid BRR filter value: {filter}"),
    };

    // Clamp to 16 bits
    let clamped = filtered.clamp(i16::MIN.into(), i16::MAX.into()) as i16;

    // Clip to 15 bits
    (clamped << 1) >> 1
}

struct GaussArgs {
    pitch_counter: u16,
    oldest: i16,
    older: i16,
    old: i16,
    sample: i16,
}

fn apply_gaussian_filter(
    GaussArgs { sample, pitch_counter, old, older, oldest }: GaussArgs,
) -> i16 {
    // Do math in 32 bits to avoid overflows
    let sample: i32 = sample.into();
    let old: i32 = old.into();
    let older: i32 = older.into();
    let oldest: i32 = oldest.into();

    // Bits 4-11 of the pitch counter are used as the interpolation index
    let interpolation_idx = ((pitch_counter >> 4) & 0xFF) as usize;

    // Sum the 3 older samples with 15-bit wrapping
    let mut sum = (GAUSSIAN_TABLE[0x0FF - interpolation_idx] * oldest) >> 11;
    sum += (GAUSSIAN_TABLE[0x1FF - interpolation_idx] * older) >> 11;
    sum += (GAUSSIAN_TABLE[0x100 + interpolation_idx] * old) >> 11;

    // Clip to 15 bits
    sum = (((sum as i16) << 1) >> 1).into();

    // Add in the current sample
    sum += (GAUSSIAN_TABLE[interpolation_idx] * sample) >> 11;

    // Clamp the final result to signed 15-bit
    sum.clamp((i16::MIN >> 1).into(), (i16::MAX >> 1).into()) as i16
}

fn compute_exp_decay(current_value: i32) -> i32 {
    -(((current_value - 1) >> 8) + 1)
}

#[derive(Debug, Clone, Encode, Decode)]
struct NoiseGenerator {
    output: i16,
}

impl NoiseGenerator {
    fn new() -> Self {
        Self { output: i16::MIN >> 1 }
    }

    fn clock(&mut self, noise_frequency: u8, global_counter: u16) {
        // Noise generator uses the same rate/offset tables as the envelopes
        let rate = noise_frequency as usize;
        if rate != 0
            && (global_counter + ENVELOPE_OFFSET_TABLE[rate]) % ENVELOPE_RATE_TABLE[rate] == 0
        {
            let new_bit = self.output.bit(0) ^ self.output.bit(1);
            self.output = ((self.output >> 1) & 0x3FFF) | (i16::from(new_bit) << 14);

            // Clip to 15 bits
            self.output = (self.output << 1) >> 1;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct EchoFilter {
    echo_enabled: [bool; 8],
    buffer_start_address: u16,
    buffer_current_offset: u16,
    buffer_samples_remaining: u16,
    buffer_size_samples: u16,
    volume_l: i8,
    volume_r: i8,
    feedback_volume: i8,
    fir_coefficients: [i8; 8],
    sample_buffer_l: [i16; 8],
    sample_buffer_r: [i16; 8],
    sample_buffer_idx: usize,
    // Stored because the highest 4 bits are unused R/W
    last_edl_write: u8,
}

impl EchoFilter {
    fn new() -> Self {
        Self {
            echo_enabled: [false; 8],
            buffer_start_address: 0,
            buffer_current_offset: 0,
            buffer_samples_remaining: 1,
            buffer_size_samples: 1,
            volume_l: 0,
            volume_r: 0,
            feedback_volume: 0,
            fir_coefficients: [0; 8],
            sample_buffer_l: [0; 8],
            sample_buffer_r: [0; 8],
            sample_buffer_idx: 0,
            last_edl_write: 0,
        }
    }

    fn write_echo_enabled(&mut self, eon: u8) {
        for i in 0..8 {
            self.echo_enabled[i] = eon.bit(i as u8);
        }
    }

    fn read_echo_enabled(&self) -> u8 {
        (0..8).map(|i| u8::from(self.echo_enabled[i]) << i).reduce(|a, b| a | b).unwrap()
    }

    fn write_echo_buffer_size(&mut self, edl: u8) {
        self.buffer_size_samples = match edl & 0x0F {
            0 => 1,
            edl => u16::from(edl) << 9,
        };

        self.last_edl_write = edl;
    }

    fn do_filter(
        &mut self,
        echo_buffer_writes_enabled: bool,
        audio_ram: &mut AudioRam,
        voice_samples_l: &[i32; 8],
        voice_samples_r: &[i32; 8],
    ) -> (i32, i32) {
        let current_buffer_addr =
            self.buffer_start_address.wrapping_add(self.buffer_current_offset);
        self.sample_buffer_l[self.sample_buffer_idx] =
            read_echo_sample(audio_ram, current_buffer_addr);
        self.sample_buffer_r[self.sample_buffer_idx] =
            read_echo_sample(audio_ram, current_buffer_addr.wrapping_add(2));

        // Add the 7 older samples with 16-bit wrapping behavior
        let mut fir_sample_l: i32 = 0;
        let mut fir_sample_r: i32 = 0;
        for i in 0..7 {
            let coefficient: i32 = self.fir_coefficients[i].into();

            let buffer_idx = self.sample_buffer_idx.wrapping_add(i + 1) & 0x07;
            let sample_l: i32 = self.sample_buffer_l[buffer_idx].into();
            let sample_r: i32 = self.sample_buffer_r[buffer_idx].into();

            fir_sample_l += (coefficient * sample_l) >> 6;
            fir_sample_r += (coefficient * sample_r) >> 6;
        }

        // Clip to 16 bits before adding the newest sample
        fir_sample_l = (fir_sample_l as i16).into();
        fir_sample_r = (fir_sample_r as i16).into();

        // Add in the newest sample and clamp to 16 bits
        fir_sample_l += (i32::from(self.fir_coefficients[7])
            * i32::from(self.sample_buffer_l[self.sample_buffer_idx]))
            >> 6;
        fir_sample_r += (i32::from(self.fir_coefficients[7])
            * i32::from(self.sample_buffer_r[self.sample_buffer_idx]))
            >> 6;

        let mut fir_sample_l = fir_sample_l.clamp(i16::MIN.into(), i16::MAX.into());
        let mut fir_sample_r = fir_sample_r.clamp(i16::MIN.into(), i16::MAX.into());

        // Force the lowest bit clear in FIR sample
        fir_sample_l &= !1;
        fir_sample_r &= !1;

        if echo_buffer_writes_enabled {
            self.write_to_echo_buffer(
                audio_ram,
                voice_samples_l,
                voice_samples_r,
                fir_sample_l,
                fir_sample_r,
            );
        }

        self.sample_buffer_idx = (self.sample_buffer_idx + 1) & 0x07;

        self.buffer_samples_remaining -= 1;
        if self.buffer_samples_remaining == 0 {
            self.buffer_current_offset = 0;
            self.buffer_samples_remaining = self.buffer_size_samples;
        } else {
            self.buffer_current_offset = self.buffer_current_offset.wrapping_add(4);
        }

        let echo_out_l = (fir_sample_l * i32::from(self.volume_l)) >> 7;
        let echo_out_r = (fir_sample_r * i32::from(self.volume_r)) >> 7;

        (echo_out_l, echo_out_r)
    }

    fn write_to_echo_buffer(
        &self,
        audio_ram: &mut AudioRam,
        voice_samples_l: &[i32; 8],
        voice_samples_r: &[i32; 8],
        fir_sample_l: i32,
        fir_sample_r: i32,
    ) {
        let mut echo_voice_sum_l: i32 = 0;
        let mut echo_voice_sum_r: i32 = 0;
        for i in (0..8).filter(|&i| self.echo_enabled[i]) {
            echo_voice_sum_l += voice_samples_l[i];
            echo_voice_sum_r += voice_samples_r[i];

            echo_voice_sum_l = echo_voice_sum_l.clamp(i16::MIN.into(), i16::MAX.into());
            echo_voice_sum_r = echo_voice_sum_r.clamp(i16::MIN.into(), i16::MAX.into());
        }

        let echo_feedback_l = (fir_sample_l * i32::from(self.feedback_volume)) >> 7;
        let echo_feedback_r = (fir_sample_r * i32::from(self.feedback_volume)) >> 7;

        // Force the lowest bit clear before writing back to echo buffer
        let echo_sample_l =
            (echo_voice_sum_l + echo_feedback_l).clamp(i16::MIN.into(), i16::MAX.into()) & !1;
        let echo_sample_r =
            (echo_voice_sum_r + echo_feedback_r).clamp(i16::MIN.into(), i16::MAX.into()) & !1;

        let current_buffer_addr =
            self.buffer_start_address.wrapping_add(self.buffer_current_offset);
        write_echo_sample(audio_ram, current_buffer_addr, echo_sample_l as i16);
        write_echo_sample(audio_ram, current_buffer_addr.wrapping_add(2), echo_sample_r as i16);
    }
}

fn read_echo_sample(audio_ram: &AudioRam, address: u16) -> i16 {
    let lsb = audio_ram[address as usize];
    let msb = audio_ram[address.wrapping_add(1) as usize];
    i16::from_le_bytes([lsb, msb]) >> 1
}

fn write_echo_sample(audio_ram: &mut AudioRam, address: u16, value: i16) {
    let [value_lsb, value_msb] = value.to_le_bytes();
    audio_ram[address as usize] = value_lsb;
    audio_ram[address.wrapping_add(1) as usize] = value_msb;
}

#[derive(Debug, Clone, Encode, Decode)]
struct DspRegisters {
    sample_table_address: u16,
    master_volume_l: i8,
    master_volume_r: i8,
    noise_frequency: u8,
    echo_buffer_writes_enabled: bool,
    mute_amplifier: bool,
    soft_reset: bool,
    global_counter: u16,
    // Unused R/W registers
    unused_xa_registers: [u8; 8],
    unused_xb_registers: [u8; 8],
    unused_xe_registers: [u8; 8],
    unused_1d_register: u8,
}

impl DspRegisters {
    fn new() -> Self {
        Self {
            sample_table_address: 0,
            master_volume_l: 0,
            master_volume_r: 0,
            noise_frequency: 0,
            echo_buffer_writes_enabled: false,
            mute_amplifier: true,
            soft_reset: true,
            global_counter: 0,
            unused_xa_registers: [0; 8],
            unused_xb_registers: [0; 8],
            unused_xe_registers: [0; 8],
            unused_1d_register: 0,
        }
    }

    fn write_flg(&mut self, value: u8) {
        self.noise_frequency = value & 0x1F;
        self.echo_buffer_writes_enabled = !value.bit(5);
        self.mute_amplifier = value.bit(6);
        self.soft_reset = value.bit(7);

        log::trace!("  Noise frequency: {:02X}", self.noise_frequency);
        log::trace!("  Echo buffer writes enabled: {}", self.echo_buffer_writes_enabled);
        log::trace!("  Mute amplifier: {}", self.mute_amplifier);
        log::trace!("  Soft reset: {}", self.soft_reset);
    }

    fn read_flg(&self) -> u8 {
        self.noise_frequency
            | (u8::from(!self.echo_buffer_writes_enabled) << 5)
            | (u8::from(self.mute_amplifier) << 6)
            | (u8::from(self.soft_reset) << 7)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioDsp {
    voices: [Voice; 8],
    registers: DspRegisters,
    noise_generator: NoiseGenerator,
    echo_filter: EchoFilter,
    register_address: u8,
}

impl AudioDsp {
    pub fn new() -> Self {
        Self {
            voices: array::from_fn(|_| Voice::default()),
            registers: DspRegisters::new(),
            noise_generator: NoiseGenerator::new(),
            echo_filter: EchoFilter::new(),
            register_address: 0,
        }
    }

    pub fn read_address(&self) -> u8 {
        self.register_address
    }

    pub fn write_address(&mut self, address: u8) {
        self.register_address = address;
    }

    pub fn read_register(&self) -> u8 {
        log::trace!("DSP register read: {:02X}", self.register_address);

        // Addresses $80-$FF mirror $00-$7F
        let address = self.register_address & 0x7F;

        // High nibble of register address encodes the voice
        let voice = (address >> 4) as usize;

        match address & 0x0F {
            0x00 => self.voices[voice].volume_l as u8,
            0x01 => self.voices[voice].volume_r as u8,
            0x02 => (self.voices[voice].sample_rate >> 8) as u8,
            0x03 => self.voices[voice].last_pitch_h_write,
            0x04 => self.voices[voice].instrument_number,
            0x05 => self.voices[voice].read_adsr_low(),
            0x06 => self.voices[voice].read_adsr_high(),
            0x07 => self.voices[voice].read_gain(),
            0x08 => self.voices[voice].read_envelope(),
            0x09 => self.voices[voice].read_output(),
            0x0A => self.registers.unused_xa_registers[voice],
            0x0B => self.registers.unused_xb_registers[voice],
            0x0E => self.registers.unused_xe_registers[voice],
            0x0F => self.echo_filter.fir_coefficients[voice] as u8,
            // $xC and $xD registers are not voice-specific
            0x0C | 0x0D => match address {
                0x0C => self.registers.master_volume_l as u8,
                0x1C => self.registers.master_volume_r as u8,
                0x2C => self.echo_filter.volume_l as u8,
                0x3C => self.echo_filter.volume_r as u8,
                0x4C => {
                    // Key on flags
                    (0..8)
                        .map(|voice| u8::from(self.voices[voice].keyed_on) << voice)
                        .reduce(|a, b| a | b)
                        .unwrap()
                }
                0x5C => {
                    // Key off flags
                    (0..8)
                        .map(|voice| u8::from(self.voices[voice].keyed_off) << voice)
                        .reduce(|a, b| a | b)
                        .unwrap()
                }
                0x6C => self.registers.read_flg(),
                0x7C => {
                    // ENDX (end flags)
                    (0..8)
                        .map(|voice| u8::from(self.voices[voice].end_flag_seen) << voice)
                        .reduce(|a, b| a | b)
                        .unwrap()
                }
                0x0D => self.echo_filter.feedback_volume as u8,
                0x1D => self.registers.unused_1d_register,
                0x2D => {
                    // Pitch modulation enable flags (voices 1-7 only)
                    (1..8)
                        .map(|voice| u8::from(self.voices[voice].pitch_modulation_enabled) << voice)
                        .reduce(|a, b| a | b)
                        .unwrap()
                }
                0x3D => {
                    // Output noise flags
                    (0..8)
                        .map(|voice| u8::from(self.voices[voice].output_noise) << voice)
                        .reduce(|a, b| a | b)
                        .unwrap()
                }
                0x4D => self.echo_filter.read_echo_enabled(),
                0x5D => (self.registers.sample_table_address >> 8) as u8,
                0x6D => (self.echo_filter.buffer_start_address >> 8) as u8,
                0x7D => self.echo_filter.last_edl_write,
                _ => unreachable!("all $xC and $xD addresses covered"),
            },
            _ => unreachable!("all addresses <= $7F covered"),
        }
    }

    pub fn write_register(&mut self, value: u8) {
        // Addresses $80-$FF are not writable
        if self.register_address >= 0x80 {
            return;
        }

        log::trace!("DSP register write: {:02X}: {value:02X}", self.register_address);

        // High nibble of register address encodes the voice
        let voice = (self.register_address >> 4) as usize;

        match self.register_address & 0x0F {
            0x00 => {
                self.voices[voice].volume_l = value as i8;
                log::trace!("  Voice {voice} volume L: {value:02X}");
            }
            0x01 => {
                self.voices[voice].volume_r = value as i8;
                log::trace!("  Voice {voice} volume R: {value:02X}");
            }
            0x02 => {
                self.voices[voice].write_pitch_low(value);
                log::trace!("  Voice {voice} sample rate: {:04X}", self.voices[voice].sample_rate);
            }
            0x03 => {
                self.voices[voice].write_pitch_high(value);
                log::trace!("  Voice {voice} sample rate: {:04X}", self.voices[voice].sample_rate);
            }
            0x04 => {
                self.voices[voice].instrument_number = value;
                log::trace!("  Voice {voice} instrument number: {value:02X}");
            }
            0x05 => {
                log::trace!("  Voice {voice} ADSR low write: {value:02X}");
                self.voices[voice].write_adsr_low(value);
            }
            0x06 => {
                log::trace!("  Voice {voice} ADSR high write: {value:02X}");
                self.voices[voice].write_adsr_high(value);
            }
            0x07 => {
                log::trace!("  Voice {voice} gain write: {value:02X}");
                self.voices[voice].write_gain(value);
            }
            0x08 | 0x09 => {
                // Current envelope value and current output value
                // These are technically writable, although the DSP overwrites them at 32000 Hz
                // TODO implement these
            }
            0x0A => {
                self.registers.unused_xa_registers[voice] = value;
            }
            0x0B => {
                self.registers.unused_xb_registers[voice] = value;
            }
            0x0E => {
                self.registers.unused_xe_registers[voice] = value;
            }
            0x0F => {
                self.echo_filter.fir_coefficients[voice] = value as i8;
            }
            // $xC and $xD registers are not voice-specific
            0x0C | 0x0D => match self.register_address {
                0x0C => {
                    self.registers.master_volume_l = value as i8;
                    log::trace!("  Master volume L: {value:02X}");
                }
                0x1C => {
                    self.registers.master_volume_r = value as i8;
                    log::trace!("  Master volume R: {value:02X}");
                }
                0x2C => {
                    self.echo_filter.volume_l = value as i8;
                    log::trace!("  Echo volume L: {}", self.echo_filter.volume_l);
                }
                0x3C => {
                    self.echo_filter.volume_r = value as i8;
                    log::trace!("  Echo volume R: {}", self.echo_filter.volume_r);
                }
                0x4C => {
                    // Key on flags
                    for voice in 0..8 {
                        self.voices[voice].write_key_on(value.bit(voice as u8));
                    }
                    log::trace!("  Key on: {value:02X}");
                }
                0x5C => {
                    // Key off flags
                    for voice in 0..8 {
                        self.voices[voice].write_key_off(value.bit(voice as u8));
                    }
                    log::trace!("  Key off: {value:02X}");
                }
                0x6C => {
                    self.registers.write_flg(value);

                    // Apply soft reset immediately if set
                    if self.registers.soft_reset {
                        for voice in &mut self.voices {
                            voice.soft_reset();
                        }
                    }
                }
                0x7C => {
                    // ENDX; writing any value clears all of the flags
                    for voice in &mut self.voices {
                        voice.end_flag_seen = false;
                    }
                }
                0x0D => {
                    self.echo_filter.feedback_volume = value as i8;

                    log::trace!("  Echo feedback volume: {}", self.echo_filter.feedback_volume);
                }
                0x1D => {
                    self.registers.unused_1d_register = value;
                }
                0x2D => {
                    // Pitch modulation enable flags (voices 1-7 only)
                    for voice in 1..8 {
                        self.voices[voice].pitch_modulation_enabled = value.bit(voice as u8);
                    }
                    log::trace!("  Pitch modulation enabled: {value:02X}");
                }
                0x3D => {
                    // Output noise flags
                    for voice in 0..8 {
                        self.voices[voice].output_noise = value.bit(voice as u8);
                    }
                    log::trace!("  Output noise: {value:02X}");
                }
                0x4D => {
                    self.echo_filter.write_echo_enabled(value);

                    log::trace!("  Echo enabled: {value:02X}");
                }
                0x5D => {
                    // Sample table address is in 256-byte steps
                    self.registers.sample_table_address = u16::from_le_bytes([0x00, value]);
                    log::trace!(
                        "  Sample table address: {:04X}",
                        self.registers.sample_table_address
                    );
                }
                0x6D => {
                    // Echo ring buffer address is in 256-byte steps
                    self.echo_filter.buffer_start_address = u16::from_le_bytes([0x00, value]);
                    log::trace!(
                        "Echo buffer start address: {:04X}",
                        self.echo_filter.buffer_start_address
                    );
                }
                0x7D => {
                    self.echo_filter.write_echo_buffer_size(value);
                    log::trace!("  Echo buffer size: {value:02X}");
                }
                _ => unreachable!("all $xC and $xD addresses covered"),
            },
            _ => unreachable!("all addresses <= $7F covered"),
        }
    }

    #[must_use]
    pub fn clock(&mut self, audio_ram: &mut AudioRam) -> (i16, i16) {
        if self.registers.global_counter == 0 {
            self.registers.global_counter = 0x77FF;
        } else {
            self.registers.global_counter -= 1;
        }

        self.noise_generator.clock(self.registers.noise_frequency, self.registers.global_counter);

        for i in 0..8 {
            let prev_voice_output = if i != 0 { self.voices[i - 1].current_sample } else { 0 };
            self.voices[i].clock(
                &self.registers,
                audio_ram,
                prev_voice_output,
                self.noise_generator.output,
            );
        }

        self.sample(audio_ram)
    }

    fn sample(&mut self, audio_ram: &mut AudioRam) -> (i16, i16) {
        let mut voice_samples_l = [0; 8];
        let mut voice_samples_r = [0; 8];
        let mut voice_sum_l = 0_i32;
        let mut voice_sum_r = 0_i32;
        for (i, voice) in self.voices.iter().enumerate() {
            let voice_sample_l = (i32::from(voice.current_sample) * i32::from(voice.volume_l)) >> 6;
            let voice_sample_r = (i32::from(voice.current_sample) * i32::from(voice.volume_r)) >> 6;

            voice_samples_l[i] = voice_sample_l;
            voice_samples_r[i] = voice_sample_r;

            voice_sum_l += voice_sample_l;
            voice_sum_r += voice_sample_r;

            voice_sum_l = voice_sum_l.clamp(i16::MIN.into(), i16::MAX.into());
            voice_sum_r = voice_sum_r.clamp(i16::MIN.into(), i16::MAX.into());
        }

        voice_sum_l = (voice_sum_l * i32::from(self.registers.master_volume_l)) >> 7;
        voice_sum_l = voice_sum_l.clamp(i16::MIN.into(), i16::MAX.into());

        voice_sum_r = (voice_sum_r * i32::from(self.registers.master_volume_r)) >> 7;
        voice_sum_r = voice_sum_r.clamp(i16::MIN.into(), i16::MAX.into());

        let (echo_l, echo_r) = self.echo_filter.do_filter(
            self.registers.echo_buffer_writes_enabled,
            audio_ram,
            &voice_samples_l,
            &voice_samples_r,
        );

        let (out_l, out_r) = if !self.registers.mute_amplifier {
            let out_l = (voice_sum_l + echo_l).clamp(i16::MIN.into(), i16::MAX.into());
            let out_r = (voice_sum_r + echo_r).clamp(i16::MIN.into(), i16::MAX.into());
            (out_l, out_r)
        } else {
            // All processing continues while muted, but the DSP outputs silence
            (0, 0)
        };

        ((out_l as i16) ^ !0, (out_r as i16) ^ !0)
    }

    pub fn reset(&mut self) {
        // Set soft reset flag, mute amplifier, and block echo buffer writes
        self.registers.write_flg(0xE0);

        for voice in &mut self.voices {
            voice.soft_reset();
        }
    }
}
