//! Wrappers around `Box<[u8; LEN]>` and similar types with a custom `bincode::Decode`
//! implementation that deserializes directly into heap memory.
//!
//! This exists because the implementation that `#[derive(Decode)]` generates for `Box<[u8; LEN]>`
//! deserializes into stack memory and then moves to the heap, which is problematic when deserializing
//! large arrays (particularly on Windows).
//!
//! For types other than `u8`, these implementations also feature significantly more efficient
//! `bincode::Encode` and `bincode::Decode` implementations than what bincode's derive macros
//! would produce.

use crate::frontend::Color;
use bincode::de::read::Reader;
use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::Encoder;
use bincode::enc::write::Writer;
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use bytemuck::Pod;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone)]
pub struct BoxedArray<T, const LEN: usize>(Box<[T; LEN]>);

impl<T: Debug + Default + Copy, const LEN: usize> Default for BoxedArray<T, LEN> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Debug + Default + Copy, const LEN: usize> BoxedArray<T, LEN> {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new() -> Self {
        Self(vec![T::default(); LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl<T, const LEN: usize> From<Box<[T; LEN]>> for BoxedArray<T, LEN> {
    fn from(value: Box<[T; LEN]>) -> Self {
        Self(value)
    }
}

impl<T, const LEN: usize> Deref for BoxedArray<T, LEN> {
    type Target = Box<[T; LEN]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const LEN: usize> DerefMut for BoxedArray<T, LEN> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Pod, const LEN: usize> Encode for BoxedArray<T, LEN> {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        // This is _significantly_ faster than the #[derive(Encode)] implementation for non-u8 types
        let bytes = bytemuck::cast_slice(self.0.as_slice());
        encoder.writer().write(bytes)
    }
}

impl<T: Debug + Default + Copy + Pod, Context, const LEN: usize> Decode<Context>
    for BoxedArray<T, LEN>
{
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        // Similarly, this is _significantly_ faster than the #[derive(Decode)] implementation for non-u8 types
        let mut array = vec![T::default(); LEN];
        decoder.reader().read(bytemuck::cast_slice_mut(&mut array))?;

        Ok(Self(array.into_boxed_slice().try_into().unwrap()))
    }
}

impl<'de, T: Debug + Default + Copy + Pod, Context, const LEN: usize> BorrowDecode<'de, Context>
    for BoxedArray<T, LEN>
{
    fn borrow_decode<D: BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        Self::decode(decoder)
    }
}

pub type BoxedByteArray<const LEN: usize> = BoxedArray<u8, LEN>;
pub type BoxedWordArray<const LEN: usize> = BoxedArray<u16, LEN>;
pub type BoxedColorArray<const LEN: usize> = BoxedArray<Color, LEN>;
