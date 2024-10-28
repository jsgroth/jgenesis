//! Master System / Game Gear audio resampling code

#![allow(clippy::excessive_precision)]

use bincode::{Decode, Encode};
use jgenesis_common::audio::SignalResampler;
use jgenesis_common::frontend::{AudioOutput, TimingMode};

pub const NTSC_MCLK_FREQUENCY: f64 = 53_693_175.0;
pub const PAL_MCLK_FREQUENCY: f64 = 53_203_424.0;

pub(crate) trait TimingModeExt {
    fn mclk_frequency(self) -> f64;
}

impl TimingModeExt for TimingMode {
    fn mclk_frequency(self) -> f64 {
        match self {
            Self::Ntsc => NTSC_MCLK_FREQUENCY,
            Self::Pal => PAL_MCLK_FREQUENCY,
        }
    }
}

const PSG_LPF_COEFFICIENT_0: f64 = -0.0005280106524700121;
const PSG_LPF_COEFFICIENTS: [f64; 83] = [
    -0.0005280106524700121,
    -0.000617859705570362,
    -0.0006863105585432678,
    -0.0007244738076134367,
    -0.0007168415136258806,
    -0.0006428322762498399,
    -0.0004800223937970845,
    -0.0002087519043582088,
    0.000182463136958003,
    0.0006914809974583107,
    0.001297927800061201,
    0.001960316226481437,
    0.002615653212751422,
    0.003182067952719604,
    0.003564664445685833,
    0.003664413939839759,
    0.003389494473880131,
    0.002668105095033223,
    0.001461478920922637,
    -0.0002243659866931583,
    -0.002329634836357198,
    -0.004735890777320608,
    -0.007266390085440696,
    -0.009692254909598111,
    -0.01174457712578106,
    -0.01313194797464798,
    -0.01356231396713684,
    -0.01276753290036646,
    -0.01052860374260526,
    -0.006699319269941674,
    -0.00122607334217566,
    0.005838241795684831,
    0.01432789832732392,
    0.02396661192384056,
    0.03437877043165732,
    0.04510915961972669,
    0.05564995818540019,
    0.0654730847194125,
    0.07406544404892251,
    0.08096429420895093,
    0.08578987375715055,
    0.08827260451043097,
    0.08827260451043097,
    0.08578987375715057,
    0.08096429420895095,
    0.07406544404892251,
    0.06547308471941252,
    0.05564995818540019,
    0.04510915961972671,
    0.03437877043165732,
    0.02396661192384055,
    0.01432789832732392,
    0.005838241795684829,
    -0.00122607334217566,
    -0.006699319269941676,
    -0.01052860374260526,
    -0.01276753290036646,
    -0.01356231396713684,
    -0.01313194797464798,
    -0.01174457712578106,
    -0.009692254909598118,
    -0.007266390085440697,
    -0.004735890777320608,
    -0.002329634836357198,
    -0.0002243659866931578,
    0.001461478920922638,
    0.002668105095033223,
    0.003389494473880133,
    0.00366441393983976,
    0.003564664445685832,
    0.003182067952719607,
    0.002615653212751421,
    0.001960316226481438,
    0.001297927800061202,
    0.00069148099745831,
    0.0001824631369580029,
    -0.0002087519043582094,
    -0.0004800223937970846,
    -0.0006428322762498405,
    -0.0007168415136258812,
    -0.0007244738076134372,
    -0.0006863105585432677,
    -0.0006178597055703623,
];

const PSG_HPF_CHARGE_FACTOR: f64 = 0.999212882632514;

pub type PsgResampler = SignalResampler<83, 0>;

#[must_use]
pub fn new_psg_resampler(console_mclk_frequency: f64) -> PsgResampler {
    let psg_frequency = compute_psg_frequency(console_mclk_frequency);
    PsgResampler::new(
        psg_frequency,
        PSG_LPF_COEFFICIENT_0,
        PSG_LPF_COEFFICIENTS,
        PSG_HPF_CHARGE_FACTOR,
    )
}

fn compute_psg_frequency(console_mclk_frequency: f64) -> f64 {
    console_mclk_frequency / 15.0 / 16.0
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct AudioResampler {
    psg_resampler: PsgResampler,
}

impl AudioResampler {
    pub fn new(timing_mode: TimingMode) -> Self {
        let psg_resampler = new_psg_resampler(timing_mode.mclk_frequency());
        Self { psg_resampler }
    }

    pub fn update_timing_mode(&mut self, timing_mode: TimingMode) {
        let psg_frequency = compute_psg_frequency(timing_mode.mclk_frequency());
        self.psg_resampler.update_source_frequency(psg_frequency);
    }

    pub fn collect_sample(&mut self, sample_l: f64, sample_r: f64) {
        self.psg_resampler.collect_sample(sample_l, sample_r);
    }

    pub fn output_samples<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        while let Some((sample_l, sample_r)) = self.psg_resampler.output_buffer_pop_front() {
            audio_output.push_sample(sample_l, sample_r)?;
        }

        Ok(())
    }

    pub fn update_output_frequency(&mut self, output_frequency: u64) {
        self.psg_resampler.update_output_frequency(output_frequency);
    }
}
