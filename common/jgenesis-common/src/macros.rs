#[macro_export]
macro_rules! define_bit_enum {
    ($name:ident, [$zero:ident, $one:ident $(,)?]) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode,
        )]
        pub enum $name {
            #[default]
            $zero = 0,
            $one = 1,
        }

        impl $name {
            pub fn from_bit(bit: bool) -> Self {
                if bit { Self::$one } else { Self::$zero }
            }
        }
    };
}

#[macro_export]
macro_rules! define_2_bit_enum {
    ($name:ident, [$zero:ident, $one:ident, $two:ident, $three:ident $(,)?]) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Default, ::bincode::Encode, ::bincode::Decode,
        )]
        pub enum $name {
            #[default]
            $zero = 0,
            $one = 1,
            $two = 2,
            $three = 3,
        }

        impl $name {
            pub fn from_bits(bits: u8) -> Self {
                match bits & 3 {
                    0 => Self::$zero,
                    1 => Self::$one,
                    2 => Self::$two,
                    3 => Self::$three,
                    _ => unreachable!("value & 3 is always <= 3"),
                }
            }
        }
    };
}
