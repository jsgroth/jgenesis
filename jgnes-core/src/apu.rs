const LENGTH_COUNTER_LOOKUP_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LengthCounterChannel {
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

#[derive(Debug, Clone)]
struct LengthCounter {
    channel: LengthCounterChannel,
    counter: u8,
    enabled: bool,
    halted: bool,
}

impl LengthCounter {
    fn new(channel: LengthCounterChannel) -> Self {
        Self {
            channel,
            counter: 0,
            enabled: false,
            halted: false,
        }
    }

    fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        let enabled = snd_chn_value & self.channel.snd_chn_enabled_mask() != 0;
        self.enabled = enabled;

        if !enabled {
            self.counter = 0;
        }
    }

    fn process_vol_update(&mut self, vol_value: u8) {
        assert!(matches!(
            self.channel,
            LengthCounterChannel::Pulse1
                | LengthCounterChannel::Pulse2
                | LengthCounterChannel::Noise
        ));

        self.halted = vol_value & 0x20 != 0;
    }

    fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        assert_eq!(self.channel, LengthCounterChannel::Triangle);

        self.halted = tri_linear_value & 0x80 != 0;
    }

    fn process_hi_update(&mut self, hi_value: u8) {
        if self.enabled {
            self.counter = LENGTH_COUNTER_LOOKUP_TABLE[(hi_value >> 3) as usize];
        }
    }

    fn clock(&mut self) {
        if !self.halted && self.counter > 0 {
            self.counter -= 1;
        }
    }
}

#[derive(Debug, Clone)]
struct Envelope {
    divider: u8,
    divider_period: u8,
    decay_level_counter: u8,
    start_flag: bool,
    loop_flag: bool,
    constant_volume_flag: bool,
}

impl Envelope {
    fn new() -> Self {
        Self {
            divider: 0,
            divider_period: 0,
            decay_level_counter: 0,
            start_flag: false,
            loop_flag: false,
            constant_volume_flag: false,
        }
    }

    fn volume(&self) -> u8 {
        if self.constant_volume_flag {
            self.divider_period
        } else {
            self.decay_level_counter
        }
    }

    fn process_vol_update(&mut self, vol_value: u8) {
        self.loop_flag = vol_value & 0x20 != 0;
        self.constant_volume_flag = vol_value & 0x10 != 0;
        self.divider_period = vol_value & 0x0F;
    }

    fn process_hi_update(&mut self) {
        self.start_flag = true;
    }

    fn clock(&mut self) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DutyCycle {
    OneEighth,
    OneFourth,
    OneHalf,
    ThreeFourths,
}

impl DutyCycle {
    fn from_vol(vol_value: u8) -> Self {
        match vol_value & 0xC0 {
            0x00 => Self::OneEighth,
            0x40 => Self::OneFourth,
            0x80 => Self::OneHalf,
            0xC0 => Self::ThreeFourths,
            _ => panic!("{vol_value} & 0xC0 was not 0x00/0x40/0x80/0xC0"),
        }
    }

    fn waveform(self) -> [u8; 8] {
        match self {
            Self::OneEighth => [0, 1, 0, 0, 0, 0, 0, 0],
            Self::OneFourth => [0, 1, 1, 0, 0, 0, 0, 0],
            Self::OneHalf => [0, 1, 1, 1, 1, 0, 0, 0],
            Self::ThreeFourths => [1, 0, 0, 1, 1, 1, 1, 1],
        }
    }
}

#[derive(Debug, Clone)]
struct PhaseTimer<const MAX_PHASE: u8, const CPU_TICKS_PER_CLOCK: u8> {
    cpu_ticks: u8,
    cpu_divider: u16,
    divider_period: u16,
    phase: u8,
}

impl<const MAX_PHASE: u8, const CPU_TICKS_PER_CLOCK: u8>
    PhaseTimer<MAX_PHASE, CPU_TICKS_PER_CLOCK>
{
    fn new() -> Self {
        Self {
            cpu_ticks: 0,
            cpu_divider: 0,
            divider_period: 0,
            phase: 0,
        }
    }

    fn process_lo_update(&mut self, lo_value: u8) {
        self.divider_period = (self.divider_period & 0xFF00) | u16::from(lo_value);
    }

    fn process_hi_update(&mut self, hi_value: u8) {
        self.divider_period = (u16::from(hi_value & 0x07) << 8) | (self.divider_period & 0x00FF);
        self.phase = 0;
    }

    fn tick(&mut self) {
        self.cpu_ticks += 1;
        if self.cpu_ticks < CPU_TICKS_PER_CLOCK {
            return;
        }
        self.cpu_ticks = 0;

        if self.cpu_divider == 0 {
            self.cpu_divider = self.divider_period;
            self.phase = (self.phase + 1) & (MAX_PHASE - 1);
        } else {
            self.cpu_divider -= 1;
        }
    }
}

type PulsePhaseTimer = PhaseTimer<8, 2>;
type TrianglePhaseTimer = PhaseTimer<32, 1>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SweepNegateBehavior {
    OnesComplement,
    TwosComplement,
}

impl SweepNegateBehavior {
    fn negate(self, value: u16) -> u16 {
        match self {
            Self::OnesComplement => !value,
            Self::TwosComplement => (!value).wrapping_add(1),
        }
    }
}

#[derive(Debug, Clone)]
struct PulseSweep {
    enabled: bool,
    divider: u8,
    divider_period: u8,
    negate_flag: bool,
    negate_behavior: SweepNegateBehavior,
    shift: u8,
    reload_flag: bool,
    target_period: u16,
}

impl PulseSweep {
    fn new(negate_behavior: SweepNegateBehavior) -> Self {
        Self {
            enabled: false,
            divider: 0,
            divider_period: 0,
            negate_flag: false,
            negate_behavior,
            shift: 0,
            reload_flag: false,
            target_period: 0,
        }
    }

    fn process_sweep_update(&mut self, sweep_value: u8, timer_period: u16) {
        self.reload_flag = true;

        self.enabled = sweep_value & 0x80 != 0;
        self.divider_period = (sweep_value >> 4) & 0x07;
        self.negate_flag = sweep_value & 0x08 != 0;
        self.shift = sweep_value & 0x07;

        self.target_period = self.compute_target_period(timer_period);
    }

    fn compute_target_period(&self, timer_period: u16) -> u16 {
        let delta = timer_period >> self.shift;
        let signed_delta = if self.negate_flag {
            self.negate_behavior.negate(delta)
        } else {
            delta
        };

        timer_period.wrapping_add(signed_delta)
    }

    fn is_channel_muted(&self, timer_period: u16) -> bool {
        timer_period < 8 || self.target_period > 0x07FF
    }

    fn clock(&mut self, timer_period: &mut u16) {
        if self.divider == 0 && self.enabled && !self.is_channel_muted(*timer_period) {
            *timer_period = self.target_period;
            self.target_period = self.compute_target_period(*timer_period);
        }

        if self.divider == 0 || self.reload_flag {
            self.divider = self.divider_period;
            self.reload_flag = false;
        } else {
            self.divider -= 1;
        }
    }
}

#[derive(Debug, Clone)]
struct PulseChannel {
    timer: PulsePhaseTimer,
    duty_cycle: DutyCycle,
    length_counter: LengthCounter,
    envelope: Envelope,
    sweep: PulseSweep,
}

impl PulseChannel {
    fn new_channel_1() -> Self {
        Self {
            timer: PulsePhaseTimer::new(),
            duty_cycle: DutyCycle::OneEighth,
            length_counter: LengthCounter::new(LengthCounterChannel::Pulse1),
            envelope: Envelope::new(),
            sweep: PulseSweep::new(SweepNegateBehavior::OnesComplement),
        }
    }

    fn new_channel_2() -> Self {
        Self {
            timer: PulsePhaseTimer::new(),
            duty_cycle: DutyCycle::OneEighth,
            length_counter: LengthCounter::new(LengthCounterChannel::Pulse2),
            envelope: Envelope::new(),
            sweep: PulseSweep::new(SweepNegateBehavior::TwosComplement),
        }
    }

    fn process_vol_update(&mut self, vol_value: u8) {
        self.duty_cycle = DutyCycle::from_vol(vol_value);
        self.length_counter.process_vol_update(vol_value);
        self.envelope.process_vol_update(vol_value);
    }

    fn process_sweep_update(&mut self, sweep_value: u8) {
        self.sweep
            .process_sweep_update(sweep_value, self.timer.divider_period);
    }

    fn process_lo_update(&mut self, lo_value: u8) {
        self.timer.process_lo_update(lo_value);
    }

    fn process_hi_update(&mut self, hi_value: u8) {
        self.timer.process_hi_update(hi_value);
        self.length_counter.process_hi_update(hi_value);
        self.envelope.process_hi_update();
    }

    fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    fn clock_half_frame(&mut self) {
        self.length_counter.clock();
        self.sweep.clock(&mut self.timer.divider_period);
    }

    fn tick_cpu(&mut self) {
        self.timer.tick();
    }

    fn sample(&self) -> u8 {
        if self.length_counter.counter == 0
            || self.sweep.is_channel_muted(self.timer.divider_period)
        {
            return 0;
        }

        let wave_step = self.duty_cycle.waveform()[self.timer.phase as usize];
        wave_step * self.envelope.volume()
    }
}
