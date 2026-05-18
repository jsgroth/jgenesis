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
use rand::distr::StandardUniform;
use rand::prelude::Distribution;
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

impl<T: Debug + Default + Copy, const LEN: usize> BoxedArray<T, LEN>
where
    StandardUniform: Distribution<T>,
{
    #[must_use]
    pub fn new_random() -> Self {
        let mut array = Self::new();
        array.fill_with(rand::random);
        array
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

#[derive(Debug, Clone)]
pub struct Boxed2DArray<T, const ROWS: usize, const COLS: usize>(Box<[[T; COLS]; ROWS]>);

impl<T: Debug + Default + Copy, const ROWS: usize, const COLS: usize> Default
    for Boxed2DArray<T, ROWS, COLS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Debug + Default + Copy, const ROWS: usize, const COLS: usize> Boxed2DArray<T, ROWS, COLS> {
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: Memory is fully filled with T::default() before calling assume_init()
        // Total allocation length is (ROWS * COLS * size_of::<T>()), and it is only accessed through
        // a *mut T with offset strictly less than (ROWS * COLS)
        unsafe {
            let mut array = Box::<[[T; COLS]; ROWS]>::new_uninit();
            let ptr = array.as_mut_ptr().cast::<T>();
            for i in 0..ROWS * COLS {
                ptr.add(i).write(T::default());
            }
            Self(array.assume_init())
        }
    }
}

impl<T, const ROWS: usize, const COLS: usize> From<Box<[[T; COLS]; ROWS]>>
    for Boxed2DArray<T, ROWS, COLS>
{
    fn from(value: Box<[[T; COLS]; ROWS]>) -> Self {
        Self(value)
    }
}

impl<T, const ROWS: usize, const COLS: usize> Deref for Boxed2DArray<T, ROWS, COLS> {
    type Target = Box<[[T; COLS]; ROWS]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const ROWS: usize, const COLS: usize> DerefMut for Boxed2DArray<T, ROWS, COLS> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Pod, const ROWS: usize, const COLS: usize> Encode for Boxed2DArray<T, ROWS, COLS> {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        let writer = encoder.writer();
        for row in self.as_slice() {
            writer.write(bytemuck::cast_slice(row))?;
        }

        Ok(())
    }
}

impl<T: Debug + Default + Copy + Pod, Context, const ROWS: usize, const COLS: usize> Decode<Context>
    for Boxed2DArray<T, ROWS, COLS>
{
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let mut array = Self::new();
        let reader = decoder.reader();
        for row in array.as_mut_slice() {
            reader.read(bytemuck::cast_slice_mut(row))?;
        }

        Ok(array)
    }
}

impl<'de, T: Debug + Default + Copy + Pod, Context, const ROWS: usize, const COLS: usize>
    BorrowDecode<'de, Context> for Boxed2DArray<T, ROWS, COLS>
{
    fn borrow_decode<D: BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        Self::decode(decoder)
    }
}

pub type Boxed2DWordArray<const ROWS: usize, const COLS: usize> = Boxed2DArray<u16, ROWS, COLS>;

#[cfg(test)]
mod tests {
    use super::*;

    // Test should be run with miri:
    //   $ cargo +nightly miri test -p jgenesis-common
    #[test]
    fn new_boxed_2d_array() {
        let array: Boxed2DArray<Color, 10, 10> = Boxed2DArray::new();

        for row in array.as_slice() {
            for &color in row {
                assert_eq!(color, Color::default());
            }
        }
    }
}
