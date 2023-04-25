use crate::bus::{CpuBus, InterruptLines, IoRegister, IrqSource};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameCounterMode {
    FourStep,
    FiveStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameCounterResetState {
    Joy2Updated,
    PendingReset,
    JustReset,
    None,
}

#[derive(Debug, Clone)]
struct FrameCounter {
    cpu_ticks: u16,
    mode: FrameCounterMode,
    interrupt_inhibit_flag: bool,
    reset_state: FrameCounterResetState,
}

impl FrameCounter {
    fn new() -> Self {
        Self {
            cpu_ticks: 0,
            mode: FrameCounterMode::FourStep,
            interrupt_inhibit_flag: false,
            reset_state: FrameCounterResetState::None,
        }
    }

    fn process_joy2_update(&mut self, joy2_value: u8) {
        self.mode = if joy2_value & 0x80 != 0 {
            FrameCounterMode::FiveStep
        } else {
            FrameCounterMode::FourStep
        };
        self.interrupt_inhibit_flag = joy2_value & 0x40 != 0;

        self.reset_state = FrameCounterResetState::Joy2Updated;
    }

    fn tick(&mut self) {
        if self.reset_state == FrameCounterResetState::JustReset {
            self.reset_state = FrameCounterResetState::None;
        }

        self.cpu_ticks += 1;

        if (self.cpu_ticks == 29830 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37282
        {
            self.cpu_ticks = 0;
        }

        if self.cpu_ticks & 0x01 == 0 {
            match self.reset_state {
                FrameCounterResetState::Joy2Updated => {
                    self.reset_state = FrameCounterResetState::PendingReset;
                }
                FrameCounterResetState::PendingReset => {
                    self.cpu_ticks = 0;
                    self.reset_state = FrameCounterResetState::JustReset;
                }
                _ => {}
            }
        }
    }

    fn divider_clock(&self) -> bool {
        self.cpu_ticks & 0x01 == 0
    }

    fn generate_quarter_frame_clock(&self) -> bool {
        (self.cpu_ticks == 7457
            || self.cpu_ticks == 14913
            || self.cpu_ticks == 22371
            || (self.cpu_ticks == 29829 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37281)
            || (self.reset_state == FrameCounterResetState::JustReset
                && self.mode == FrameCounterMode::FiveStep)
    }

    fn generate_half_frame_clock(&self) -> bool {
        (self.cpu_ticks == 14913
            || (self.cpu_ticks == 29829 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37281)
            || (self.reset_state == FrameCounterResetState::JustReset
                && self.mode == FrameCounterMode::FiveStep)
    }

    fn should_set_interrupt_flag(&self) -> bool {
        !self.interrupt_inhibit_flag && self.cpu_ticks == 29828
    }
}

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
struct PhaseTimer<const MAX_PHASE: u8, const CPU_TICKS_PER_CLOCK: u8, const CAN_RESET_PHASE: bool> {
    cpu_ticks: u8,
    cpu_divider: u16,
    divider_period: u16,
    phase: u8,
}

impl<const MAX_PHASE: u8, const CPU_TICKS_PER_CLOCK: u8, const CAN_RESET_PHASE: bool>
    PhaseTimer<MAX_PHASE, CPU_TICKS_PER_CLOCK, CAN_RESET_PHASE>
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
        if CAN_RESET_PHASE {
            self.phase = 0;
        }
    }

    fn tick(&mut self, sequencer_enabled: bool) {
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

type PulsePhaseTimer = PhaseTimer<8, 2, true>;
type TrianglePhaseTimer = PhaseTimer<32, 1, false>;

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

    fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        self.length_counter.process_snd_chn_update(snd_chn_value);
    }

    fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    fn clock_half_frame(&mut self) {
        self.length_counter.clock();
        self.sweep.clock(&mut self.timer.divider_period);
    }

    fn tick_cpu(&mut self) {
        self.timer.tick(true);
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

#[derive(Debug, Clone)]
struct LinearCounter {
    counter: u8,
    reload_value: u8,
    control_flag: bool,
    reload_flag: bool,
}

impl LinearCounter {
    fn new() -> Self {
        Self {
            counter: 0,
            reload_value: 0,
            control_flag: false,
            reload_flag: false,
        }
    }

    fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        self.control_flag = tri_linear_value & 0x80 != 0;
        self.reload_value = tri_linear_value & 0x7F;
    }

    fn process_hi_update(&mut self) {
        self.reload_flag = true;
    }

    fn clock(&mut self) {
        if self.reload_flag {
            self.counter = self.reload_value;
        } else if self.counter > 0 {
            self.counter -= 1;
        }

        if !self.control_flag {
            self.reload_flag = false;
        }
    }
}

const TRIANGLE_WAVEFORM: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[derive(Debug, Clone)]
struct TriangleChannel {
    timer: TrianglePhaseTimer,
    linear_counter: LinearCounter,
    length_counter: LengthCounter,
}

impl TriangleChannel {
    fn new() -> Self {
        Self {
            timer: TrianglePhaseTimer::new(),
            linear_counter: LinearCounter::new(),
            length_counter: LengthCounter::new(LengthCounterChannel::Triangle),
        }
    }

    fn process_tri_linear_update(&mut self, tri_linear_value: u8) {
        self.linear_counter
            .process_tri_linear_update(tri_linear_value);
        self.length_counter
            .process_tri_linear_update(tri_linear_value);
    }

    fn process_lo_update(&mut self, lo_value: u8) {
        self.timer.process_lo_update(lo_value);
    }

    fn process_hi_update(&mut self, hi_value: u8) {
        self.timer.process_hi_update(hi_value);
        self.linear_counter.process_hi_update();
        self.length_counter.process_hi_update(hi_value);
    }

    fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        self.length_counter.process_snd_chn_update(snd_chn_value);
    }

    fn clock_quarter_frame(&mut self) {
        self.linear_counter.clock();
    }

    fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    fn silenced(&self) -> bool {
        if self.linear_counter.counter == 0 || self.length_counter.counter == 0 {
            return true;
        }

        // TODO remove once a low-pass filter is in place
        self.timer.divider_period < 2
    }

    fn tick_cpu(&mut self) {
        self.timer.tick(!self.silenced());
    }

    fn sample(&self) -> u8 {
        TRIANGLE_WAVEFORM[self.timer.phase as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LfsrMode {
    Bit1Feedback,
    Bit6Feedback,
}

impl LfsrMode {}

#[derive(Debug, Clone)]
struct LinearFeedbackShiftRegister {
    register: u16,
    mode: LfsrMode,
}

impl LinearFeedbackShiftRegister {
    fn new() -> Self {
        Self {
            register: 1,
            mode: LfsrMode::Bit1Feedback,
        }
    }

    fn clock(&mut self) {
        let feedback = match self.mode {
            LfsrMode::Bit1Feedback => (self.register & 0x01) ^ ((self.register & 0x02) >> 1),
            LfsrMode::Bit6Feedback => (self.register & 0x01) ^ ((self.register & 0x40) >> 6),
        };

        self.register = (self.register >> 1) | (feedback << 14);
    }

    fn sample(&self) -> u8 {
        (!self.register & 0x01) as u8
    }
}

const NOISE_PERIOD_LOOKUP_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

#[derive(Debug, Clone)]
struct NoiseChannel {
    lfsr: LinearFeedbackShiftRegister,
    timer_counter: u16,
    timer_period: u16,
    length_counter: LengthCounter,
    envelope: Envelope,
}

impl NoiseChannel {
    fn new() -> Self {
        Self {
            lfsr: LinearFeedbackShiftRegister::new(),
            timer_counter: 0,
            timer_period: 1,
            length_counter: LengthCounter::new(LengthCounterChannel::Noise),
            envelope: Envelope::new(),
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    fn tick_cpu(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period - 1;
            self.lfsr.clock();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn process_vol_update(&mut self, vol_value: u8) {
        self.envelope.process_vol_update(vol_value);
        self.length_counter.process_vol_update(vol_value);
    }

    fn process_lo_update(&mut self, lo_value: u8) {
        self.lfsr.mode = if lo_value & 0x80 != 0 {
            LfsrMode::Bit6Feedback
        } else {
            LfsrMode::Bit1Feedback
        };

        self.timer_period = NOISE_PERIOD_LOOKUP_TABLE[(lo_value & 0x0F) as usize];
    }

    fn process_hi_update(&mut self, hi_value: u8) {
        self.envelope.process_hi_update();
        self.length_counter.process_hi_update(hi_value);
    }

    fn process_snd_chn_update(&mut self, snd_chn_value: u8) {
        self.length_counter.process_snd_chn_update(snd_chn_value);
    }

    fn sample(&self) -> u8 {
        if self.length_counter.counter == 0 {
            0
        } else {
            self.lfsr.sample() * self.envelope.volume()
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApuState {
    channel_1: PulseChannel,
    channel_2: PulseChannel,
    channel_3: TriangleChannel,
    channel_4: NoiseChannel,
    frame_counter: FrameCounter,
    frame_counter_interrupt_flag: bool,
    sample_queue: VecDeque<f32>,
    hpf_capacitor: f64,
    total_ticks: u64,
}

impl ApuState {
    pub fn new() -> Self {
        Self {
            channel_1: PulseChannel::new_channel_1(),
            channel_2: PulseChannel::new_channel_2(),
            channel_3: TriangleChannel::new(),
            channel_4: NoiseChannel::new(),
            frame_counter: FrameCounter::new(),
            frame_counter_interrupt_flag: false,
            sample_queue: VecDeque::new(),
            hpf_capacitor: 0.0,
            total_ticks: 0,
        }
    }

    fn process_register_updates(&mut self, iter: impl Iterator<Item = (IoRegister, u8)>) {
        for (register, value) in iter {
            match register {
                IoRegister::SQ1_VOL => {
                    self.channel_1.process_vol_update(value);
                }
                IoRegister::SQ1_SWEEP => {
                    self.channel_1.process_sweep_update(value);
                }
                IoRegister::SQ1_LO => {
                    self.channel_1.process_lo_update(value);
                }
                IoRegister::SQ1_HI => {
                    self.channel_1.process_hi_update(value);
                }
                IoRegister::SQ2_VOL => {
                    self.channel_2.process_vol_update(value);
                }
                IoRegister::SQ2_SWEEP => {
                    self.channel_2.process_sweep_update(value);
                }
                IoRegister::SQ2_LO => {
                    self.channel_2.process_lo_update(value);
                }
                IoRegister::SQ2_HI => {
                    self.channel_2.process_hi_update(value);
                }
                IoRegister::TRI_LINEAR => {
                    self.channel_3.process_tri_linear_update(value);
                }
                IoRegister::TRI_LO => {
                    self.channel_3.process_lo_update(value);
                }
                IoRegister::TRI_HI => {
                    self.channel_3.process_hi_update(value);
                }
                IoRegister::NOISE_VOL => {
                    self.channel_4.process_vol_update(value);
                }
                IoRegister::NOISE_LO => {
                    self.channel_4.process_lo_update(value);
                }
                IoRegister::NOISE_HI => {
                    self.channel_4.process_hi_update(value);
                }
                IoRegister::SND_CHN => {
                    self.channel_1.process_snd_chn_update(value);
                    self.channel_2.process_snd_chn_update(value);
                    self.channel_3.process_snd_chn_update(value);
                    self.channel_4.process_snd_chn_update(value);
                }
                IoRegister::JOY2 => {
                    self.frame_counter.process_joy2_update(value);
                }
                _ => {}
            }
        }
    }

    fn tick_cpu(&mut self, interrupt_lines: &mut InterruptLines) {
        self.channel_1.tick_cpu();
        self.channel_2.tick_cpu();
        self.channel_3.tick_cpu();
        self.channel_4.tick_cpu();
        self.frame_counter.tick();

        if self.frame_counter.generate_quarter_frame_clock() {
            self.channel_1.clock_quarter_frame();
            self.channel_2.clock_quarter_frame();
            self.channel_3.clock_quarter_frame();
            self.channel_4.clock_quarter_frame();
        }

        if self.frame_counter.generate_half_frame_clock() {
            self.channel_1.clock_half_frame();
            self.channel_2.clock_half_frame();
            self.channel_3.clock_half_frame();
            self.channel_4.clock_half_frame();
        }

        if self.frame_counter.should_set_interrupt_flag() {
            self.frame_counter_interrupt_flag = true;
        } else if self.frame_counter.interrupt_inhibit_flag {
            self.frame_counter_interrupt_flag = false;
        }

        interrupt_lines.set_irq_low_pull(
            IrqSource::ApuFrameCounter,
            self.frame_counter_interrupt_flag,
        );
    }

    fn mix_samples(&self) -> f32 {
        let pulse1_sample = self.channel_1.sample();
        let pulse2_sample = self.channel_2.sample();
        let triangle_sample = self.channel_3.sample();
        let noise_sample = self.channel_4.sample();

        // TODO this could be a lookup table, will be helpful when sampling every cycle
        // for a low-pass filter

        // Formulas from https://www.nesdev.org/wiki/APU_Mixer
        let pulse_mix = if pulse1_sample > 0 || pulse2_sample > 0 {
            95.88 / (8128.0 / (f64::from(pulse1_sample + pulse2_sample)) + 100.0)
        } else {
            0.0
        };

        let tnd_mix = if triangle_sample > 0 {
            159.79
                / (1.0 / (f64::from(triangle_sample) / 8227.0 + f64::from(noise_sample) / 12241.0)
                    + 100.0)
        } else {
            0.0
        };

        (pulse_mix + tnd_mix - 0.5) as f32
    }

    fn high_pass_filter(&mut self, sample: f32) -> f32 {
        let filtered_sample = f64::from(sample) - self.hpf_capacitor;

        // TODO figure out something better to do than copy-pasting what I did for the Game Boy
        self.hpf_capacitor = f64::from(sample) - 0.996336 * filtered_sample;

        filtered_sample as f32
    }

    pub fn get_sample_queue_mut(&mut self) -> &mut VecDeque<f32> {
        &mut self.sample_queue
    }
}

pub fn tick(state: &mut ApuState, bus: &mut CpuBus<'_>) {
    state.process_register_updates(bus.get_io_registers_mut().drain_dirty_registers());

    state.tick_cpu(bus.interrupt_lines());

    let prev_ticks = state.total_ticks;
    state.total_ticks += 1;

    // TODO don't hardcode frequencies
    if (prev_ticks as f64 * 48000.0 / 1789772.73).round() as u64
        != (state.total_ticks as f64 * 48000.0 / 1789772.73).round() as u64
    {
        let mixed_sample = state.mix_samples();
        let mixed_sample = state.high_pass_filter(mixed_sample);
        state.sample_queue.push_back(mixed_sample);
        state.sample_queue.push_back(mixed_sample);
    }
}
