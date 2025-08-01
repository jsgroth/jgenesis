#[macro_export]
macro_rules! define_bit_enum {
    ($name:ident, [$zero:ident, $one:ident]) => {
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
