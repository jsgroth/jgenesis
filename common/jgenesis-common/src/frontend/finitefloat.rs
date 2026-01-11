//! Newtypes that exist mainly to make variants of f32/f64 that can safely implement Eq and Hash

use bincode::{Decode, Encode};
use std::fmt::{Display, Formatter};
use std::ops::{Mul, MulAssign};

macro_rules! define_finite_float {
    ($name:ident, $ft:ty, $ut:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
        pub struct $name($ut);

        impl $name {
            pub const ZERO: Self = Self((0 as $ft).to_bits());
            pub const ONE: Self = Self((1 as $ft).to_bits());
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", <$ft>::from_bits(self.0))
            }
        }

        impl TryFrom<$ft> for $name {
            type Error = String;

            fn try_from(value: $ft) -> Result<Self, Self::Error> {
                if value.is_finite() {
                    Ok(Self(value.to_bits()))
                } else {
                    Err(format!("Not a finite value: {value}"))
                }
            }
        }

        impl From<$name> for $ft {
            fn from(value: $name) -> Self {
                <$ft>::from_bits(value.0)
            }
        }

        impl Mul for $name {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self::Output {
                let lhs = <$ft>::from(self);
                let rhs = <$ft>::from(rhs);
                Self((lhs * rhs).to_bits())
            }
        }

        impl MulAssign for $name {
            fn mul_assign(&mut self, rhs: Self) {
                *self = *self * rhs;
            }
        }
    };
}

define_finite_float!(FiniteF32, f32, u32);
define_finite_float!(FiniteF64, f64, u64);
