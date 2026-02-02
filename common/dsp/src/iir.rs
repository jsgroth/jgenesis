use bincode::{Decode, Encode};
use std::{array, iter};

#[derive(Debug, Clone, Encode, Decode)]
pub struct IirFilter<const N: usize> {
    b0: f64,
    b: [f64; N],
    a: [f64; N],
    prev_samples: [f64; N],
    prev_outputs: [f64; N],
    tiny_offset: f64,
}

impl<const N: usize> IirFilter<N> {
    /// # Panics
    ///
    /// Will panic if the const generic `N` is zero, or if either of the coefficients arrays is not
    /// length `N + 1`.
    #[must_use]
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
            tiny_offset: 1e-30,
        }
    }

    /// Returns a filter that simply returns input samples as-is
    #[must_use]
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b: array::from_fn(|_| 0.0),
            a: array::from_fn(|_| 0.0),
            prev_samples: array::from_fn(|_| 0.0),
            prev_outputs: array::from_fn(|_| 0.0),
            tiny_offset: 0.0,
        }
    }

    #[must_use]
    pub fn filter(&mut self, sample: f64) -> f64 {
        // Hack to avoid the filter getting stuck at a subnormal value
        // See <https://www.earlevel.com/main/2019/04/19/floating-point-denormals/>
        let sample = sample + self.tiny_offset;
        self.tiny_offset = -self.tiny_offset;

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
