use bincode::{Decode, Encode};
use std::{array, iter};

#[derive(Debug, Clone, Encode, Decode)]
pub struct IirFilter<const N: usize> {
    b0: f64,
    b: [f64; N],
    a: [f64; N],
    prev_samples: [f64; N],
    prev_outputs: [f64; N],
}

impl<const N: usize> IirFilter<N> {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new(b: &[f64], a: &[f64]) -> Self {
        assert!(N > 0);
        assert_eq!(b.len(), N + 1);
        assert_eq!(a.len(), N + 1);

        Self {
            b0: b[0],
            b: b[1..].try_into().unwrap(),
            a: a[1..].try_into().unwrap(),
            prev_samples: array::from_fn(|_| 0.0),
            prev_outputs: array::from_fn(|_| 0.0),
        }
    }

    #[must_use]
    pub fn filter(&mut self, sample: f64) -> f64 {
        let output = self.b0 * sample
            + iter::zip(&self.b, &self.prev_samples).map(|(&coeff, &n)| coeff * n).sum::<f64>()
            - iter::zip(&self.a, &self.prev_outputs).map(|(&coeff, &n)| coeff * n).sum::<f64>();

        for i in (1..N).rev() {
            self.prev_samples[i] = self.prev_samples[i - 1];
            self.prev_outputs[i] = self.prev_outputs[i - 1];
        }
        self.prev_samples[0] = sample;
        self.prev_outputs[0] = output;

        output
    }

    pub fn reset(&mut self) {
        self.prev_samples.fill(0.0);
        self.prev_outputs.fill(0.0);
    }
}

pub type FirstOrderIirFilter = IirFilter<1>;
pub type SecondOrderIirFilter = IirFilter<2>;
