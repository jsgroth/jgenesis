//! Wrappers around `Box<[u8; LEN]>` and `Box<[u16; LEN]>` with a custom `bincode::Decode`
//! implementation that deserializes directly into heap memory.
//!
//! This exists because the implementation that `#[derive(Decode)]` generates for `Box<[u8; LEN]>`
//! deserializes into stack memory and then moves to the heap, which is problematic when deserializing
//! large arrays (particularly on Windows).

use bincode::de::read::Reader;
use bincode::de::{BorrowDecoder, Decoder};
use bincode::error::DecodeError;
use bincode::{BorrowDecode, Decode, Encode};
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Encode)]
pub struct BoxedByteArray<const LEN: usize>(Box<[u8; LEN]>);

impl<const LEN: usize> BoxedByteArray<LEN> {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new() -> Self {
        Self(vec![0; LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl<const LEN: usize> Default for BoxedByteArray<LEN> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const LEN: usize> From<Box<[u8; LEN]>> for BoxedByteArray<LEN> {
    fn from(value: Box<[u8; LEN]>) -> Self {
        Self(value)
    }
}

impl<const LEN: usize> Deref for BoxedByteArray<LEN> {
    type Target = Box<[u8; LEN]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const LEN: usize> DerefMut for BoxedByteArray<LEN> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const LEN: usize, Context> Decode<Context> for BoxedByteArray<LEN> {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let mut array: Box<[u8; LEN]> = vec![0; LEN].into_boxed_slice().try_into().unwrap();
        decoder.reader().read(array.as_mut())?;
        Ok(Self(array))
    }
}

impl<'de, const LEN: usize, Context> BorrowDecode<'de, Context> for BoxedByteArray<LEN> {
    fn borrow_decode<D: BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        let mut array: Box<[u8; LEN]> = vec![0; LEN].into_boxed_slice().try_into().unwrap();
        decoder.reader().read(array.as_mut())?;
        Ok(Self(array))
    }
}

#[derive(Debug, Clone, Encode)]
pub struct BoxedWordArray<const LEN: usize>(Box<[u16; LEN]>);

impl<const LEN: usize> Default for BoxedWordArray<LEN> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const LEN: usize> BoxedWordArray<LEN> {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new() -> Self {
        Self(vec![0; LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl<const LEN: usize> From<Box<[u16; LEN]>> for BoxedWordArray<LEN> {
    fn from(value: Box<[u16; LEN]>) -> Self {
        Self(value)
    }
}

impl<const LEN: usize> Deref for BoxedWordArray<LEN> {
    type Target = Box<[u16; LEN]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const LEN: usize> DerefMut for BoxedWordArray<LEN> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const LEN: usize, Context> Decode<Context> for BoxedWordArray<LEN> {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let mut array: Box<[u16; LEN]> = vec![0; LEN].into_boxed_slice().try_into().unwrap();

        for value in array.as_mut() {
            *value = u16::decode(decoder)?;
        }

        Ok(Self(array))
    }
}

impl<'de, const LEN: usize, Context> BorrowDecode<'de, Context> for BoxedWordArray<LEN> {
    fn borrow_decode<D: BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        let mut array: Box<[u16; LEN]> = vec![0; LEN].into_boxed_slice().try_into().unwrap();

        for value in array.as_mut() {
            *value = u16::decode(decoder)?;
        }

        Ok(Self(array))
    }
}
