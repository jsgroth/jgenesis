mod dmc;
mod noise;
mod pulse;
mod triangle;
mod units;

use crate::apu::dmc::DeltaModulationChannel;
use crate::apu::noise::NoiseChannel;
use crate::apu::pulse::PulseChannel;
use crate::apu::triangle::TriangleChannel;
use crate::bus::{CpuBus, IoRegister, IrqSource};
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

        if (self.cpu_ticks == 29830 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37282
        {
            self.cpu_ticks = 1;
        } else {
            self.cpu_ticks += 1;
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

    fn generate_quarter_frame_clock(&self) -> bool {
        (self.cpu_ticks == 7456
            || self.cpu_ticks == 14912
            || self.cpu_ticks == 22370
            || (self.cpu_ticks == 29828 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37280)
            || (self.reset_state == FrameCounterResetState::JustReset
                && self.mode == FrameCounterMode::FiveStep)
    }

    fn generate_half_frame_clock(&self) -> bool {
        (self.cpu_ticks == 14912
            || (self.cpu_ticks == 29828 && self.mode == FrameCounterMode::FourStep)
            || self.cpu_ticks == 37280)
            || (self.reset_state == FrameCounterResetState::JustReset
                && self.mode == FrameCounterMode::FiveStep)
    }

    fn should_set_interrupt_flag(&self) -> bool {
        !self.interrupt_inhibit_flag
            && self.mode == FrameCounterMode::FourStep
            && (29827..29830).contains(&self.cpu_ticks)
    }
}

#[derive(Debug, Clone)]
pub struct ApuState {
    channel_1: PulseChannel,
    channel_2: PulseChannel,
    channel_3: TriangleChannel,
    channel_4: NoiseChannel,
    channel_5: DeltaModulationChannel,
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
            channel_5: DeltaModulationChannel::new(),
            frame_counter: FrameCounter::new(),
            frame_counter_interrupt_flag: false,
            sample_queue: VecDeque::new(),
            hpf_capacitor: 0.0,
            total_ticks: 0,
        }
    }

    pub fn dmc_dma_started(&self) -> bool {
        self.channel_5.started_dma_this_cycle()
    }

    fn process_register_updates(
        &mut self,
        iter: impl Iterator<Item = (IoRegister, u8)>,
        bus: &mut CpuBus<'_>,
    ) {
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
                IoRegister::DMC_FREQ => {
                    self.channel_5.process_dmc_freq_update(value);
                }
                IoRegister::DMC_RAW => {
                    self.channel_5.process_dmc_raw_update(value);
                }
                IoRegister::DMC_START => {
                    self.channel_5.process_dmc_start_update(value);
                }
                IoRegister::DMC_LEN => {
                    self.channel_5.process_dmc_len_update(value);
                }
                IoRegister::SND_CHN => {
                    self.channel_1.process_snd_chn_update(value);
                    self.channel_2.process_snd_chn_update(value);
                    self.channel_3.process_snd_chn_update(value);
                    self.channel_4.process_snd_chn_update(value);
                    self.channel_5.process_snd_chn_update(value, bus);
                }
                IoRegister::JOY2 => {
                    self.frame_counter.process_joy2_update(value);
                }
                _ => {}
            }
        }
    }

    fn tick_cpu(&mut self, bus: &mut CpuBus<'_>) {
        self.channel_1.tick_cpu();
        self.channel_2.tick_cpu();
        self.channel_3.tick_cpu();
        self.channel_4.tick_cpu();
        self.channel_5.tick_cpu(bus);
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

        bus.interrupt_lines().set_irq_low_pull(
            IrqSource::ApuFrameCounter,
            self.frame_counter_interrupt_flag,
        );

        bus.interrupt_lines()
            .set_irq_low_pull(IrqSource::ApuDmc, self.channel_5.interrupt_flag());
    }

    fn get_apu_status(&self) -> u8 {
        (u8::from(self.channel_5.interrupt_flag()) << 7)
            | (u8::from(self.frame_counter_interrupt_flag) << 6)
            | (u8::from(self.channel_5.sample_bytes_remaining() > 0) << 4)
            | (u8::from(self.channel_4.length_counter() > 0) << 3)
            | (u8::from(self.channel_3.length_counter() > 0) << 2)
            | (u8::from(self.channel_2.length_counter() > 0) << 1)
            | u8::from(self.channel_1.length_counter() > 0)
    }

    fn mix_samples(&self, config: &ApuConfig) -> f32 {
        let pulse1_sample = if config.ch1_enabled {
            self.channel_1.sample()
        } else {
            0
        };

        let pulse2_sample = if config.ch2_enabled {
            self.channel_2.sample()
        } else {
            0
        };

        let triangle_sample = if config.ch3_enabled {
            self.channel_3.sample()
        } else {
            0
        };

        let noise_sample = if config.ch4_enabled {
            self.channel_4.sample()
        } else {
            0
        };

        let dmc_sample = if config.ch5_enabled {
            self.channel_5.sample()
        } else {
            0
        };

        // TODO this could be a lookup table, will be helpful when sampling every cycle
        // for a low-pass filter

        // Formulas from https://www.nesdev.org/wiki/APU_Mixer
        let pulse_mix = if pulse1_sample > 0 || pulse2_sample > 0 {
            95.88 / (8128.0 / (f64::from(pulse1_sample + pulse2_sample)) + 100.0)
        } else {
            0.0
        };

        let tnd_mix = if triangle_sample > 0 || noise_sample > 0 || dmc_sample > 0 {
            159.79
                / (1.0
                    / (f64::from(triangle_sample) / 8227.0
                        + f64::from(noise_sample) / 12241.0
                        + f64::from(dmc_sample) / 22638.0)
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

pub struct ApuConfig {
    pub ch1_enabled: bool,
    pub ch2_enabled: bool,
    pub ch3_enabled: bool,
    pub ch4_enabled: bool,
    pub ch5_enabled: bool,
}

impl ApuConfig {
    pub fn new() -> Self {
        Self {
            ch1_enabled: true,
            ch2_enabled: true,
            ch3_enabled: true,
            ch4_enabled: true,
            ch5_enabled: true,
        }
    }
}

pub fn tick(state: &mut ApuState, config: &ApuConfig, bus: &mut CpuBus<'_>) {
    log::trace!("APU: Frame counter state: {:?}", state.frame_counter);
    log::trace!("APU: Pulse 1 state: {:?}", state.channel_1);
    log::trace!("APU: Pulse 2 state: {:?}", state.channel_2);
    log::trace!("APU: DMC state: {:?}", state.channel_5);

    if bus.get_io_registers_mut().get_and_clear_snd_chn_read() {
        state.frame_counter_interrupt_flag = false;
    }

    // Clear DMC DMA flag at the start of every cycle, before potentially setting it
    state.channel_5.clear_dma_this_cycle();

    let dirty_registers: Vec<_> = bus.get_io_registers_mut().drain_dirty_registers().collect();
    state.process_register_updates(dirty_registers.into_iter(), bus);

    state.tick_cpu(bus);

    bus.get_io_registers_mut()
        .set_apu_status(state.get_apu_status());
    log::trace!("APU: Status set to {:02X}", state.get_apu_status());

    let prev_ticks = state.total_ticks;
    state.total_ticks += 1;

    // TODO don't hardcode frequencies
    // the 60.0988 / 60 factor is because the actual NES displays at slightly higher than 60Hz
    if (prev_ticks as f64 * 48000.0 / 1789772.73 * 60.0988 / 60.0).round() as u64
        != (state.total_ticks as f64 * 48000.0 / 1789772.73 * 60.0988 / 60.0).round() as u64
    {
        let mixed_sample = state.mix_samples(config);
        let mixed_sample = state.high_pass_filter(mixed_sample);
        state.sample_queue.push_back(mixed_sample);
        state.sample_queue.push_back(mixed_sample);
    }
}
