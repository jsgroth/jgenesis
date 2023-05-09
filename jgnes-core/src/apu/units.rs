use bincode::{Decode, Encode};

const LENGTH_COUNTER_LOOKUP_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum LengthCounterChannel {
    Pulse1,
    Pulse2,
    Triangle,
    Noise,
}

impl LengthCounterChannel {
    fn snd_chn_enabled_mask(self) -> u8 {
        match self {
            Self::Pulse1 => 0x01,
            Self::Pulse2 => 0x02,
            Self::Triangle => 0x04,
            Self::Noise => 0x08,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct LengthCounter {
    channel: LengthCounterChannel,
    pub counter: u8,
    enabled: bool,
    halted: bool,
}

impl LengthCounter {
    pub fn new(channel: LengthCounterChannel) -> Self {
        Self {
            channel,
            counter: 0,
            enabled: false,
            halted: false,
        }
    }

    pub fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        let enabled = snd_chn_value & self.channel.snd_chn_enabled_mask() != 0;
        self.enabled = enabled;

        if !enabled {
            self.counter = 0;
        }
    }

    pub fn process_vol_update(&mut self, vol_value: u8) {
        assert!(matches!(
            self.channel,
            LengthCounterChannel::Pulse1
                | LengthCounterChannel::Pulse2
                | LengthCounterChannel::Noise
        ));

        self.halted = vol_value & 0x20 != 0;
    }

    pub fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        assert_eq!(self.channel, LengthCounterChannel::Triangle);

        self.halted = tri_linear_value & 0x80 != 0;
    }

    pub fn process_hi_update(&mut self, hi_value: u8) {
        if self.enabled {
            self.counter = LENGTH_COUNTER_LOOKUP_TABLE[(hi_value >> 3) as usize];
        }
    }

    pub fn clock(&mut self) {
        if !self.halted && self.counter > 0 {
            self.counter -= 1;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Envelope {
    divider: u8,
    divider_period: u8,
    decay_level_counter: u8,
    start_flag: bool,
    loop_flag: bool,
    constant_volume_flag: bool,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            divider: 0,
            divider_period: 0,
            decay_level_counter: 0,
            start_flag: false,
            loop_flag: false,
            constant_volume_flag: false,
        }
    }

    pub fn volume(&self) -> u8 {
        if self.constant_volume_flag {
            self.divider_period
        } else {
            self.decay_level_counter
        }
    }

    pub fn process_vol_update(&mut self, vol_value: u8) {
        self.loop_flag = vol_value & 0x20 != 0;
        self.constant_volume_flag = vol_value & 0x10 != 0;
        self.divider_period = vol_value & 0x0F;
    }

    pub fn process_hi_update(&mut self) {
        self.start_flag = true;
    }

    pub fn clock(&mut self) {
        if self.start_flag {
            self.start_flag = false;

            self.divider = self.divider_period;
            self.decay_level_counter = 0x0F;
        } else if self.divider == 0 {
            self.divider = self.divider_period;

            if self.decay_level_counter > 0 {
                self.decay_level_counter -= 1;
            } else if self.loop_flag {
                self.decay_level_counter = 0x0F;
            }
        } else {
            self.divider -= 1;
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PhaseTimer<
    const MAX_PHASE: u8,
    const CPU_TICKS_PER_CLOCK: u8,
    const DIVIDER_BITS: u8,
    const CAN_RESET_PHASE: bool,
> {
    cpu_ticks: u8,
    cpu_divider: u16,
    pub divider_period: u16,
    pub phase: u8,
}

impl<
        const MAX_PHASE: u8,
        const CPU_TICKS_PER_CLOCK: u8,
        const DIVIDER_BITS: u8,
        const CAN_RESET_PHASE: bool,
    > PhaseTimer<MAX_PHASE, CPU_TICKS_PER_CLOCK, DIVIDER_BITS, CAN_RESET_PHASE>
{
    pub fn new() -> Self {
        assert!(
            DIVIDER_BITS == 11 || DIVIDER_BITS == 12,
            "DIVIDER_BITS must be 11 or 12"
        );

        Self {
            cpu_ticks: 0,
            cpu_divider: 0,
            divider_period: 0,
            phase: 0,
        }
    }

    pub fn process_lo_update(&mut self, lo_value: u8) {
        self.divider_period = (self.divider_period & 0xFF00) | u16::from(lo_value);
    }

    pub fn process_hi_update(&mut self, hi_value: u8) {
        let hi_mask = match DIVIDER_BITS {
            11 => 0x07,
            12 => 0x0F,
            _ => panic!("DIVIDER_BITS must be 11 or 12"),
        };

        self.divider_period = (u16::from(hi_value & hi_mask) << 8) | (self.divider_period & 0x00FF);
        if CAN_RESET_PHASE {
            self.phase = 0;
        }
    }

    pub fn tick(&mut self, sequencer_enabled: bool) {
        self.cpu_ticks += 1;
        if self.cpu_ticks < CPU_TICKS_PER_CLOCK {
            return;
        }
        self.cpu_ticks = 0;

        if self.cpu_divider == 0 {
            self.cpu_divider = self.divider_period;
            if sequencer_enabled {
                self.phase = (self.phase + 1) & (MAX_PHASE - 1);
            }
        } else {
            self.cpu_divider -= 1;
        }
    }
}
