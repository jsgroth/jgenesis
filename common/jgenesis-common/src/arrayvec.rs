use std::array;
use std::ops::{Index, IndexMut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayVec<T, const LEN: usize> {
    arr: [T; LEN],
    len: u32,
}

impl<T: Copy + Default, const LEN: usize> ArrayVec<T, LEN> {
    #[must_use]
    pub fn new() -> Self {
        Self { arr: array::from_fn(|_| T::default()), len: 0 }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        assert!(self.len < LEN as u32, "ArrayVec exceeded length of {LEN}");

        self.arr[self.len as usize] = value;
        self.len += 1;
    }

    #[inline]
    #[must_use]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }

        self.len -= 1;
        Some(self.arr[self.len as usize])
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    #[must_use]
    pub fn iter(&self) -> ArrayVecIter<'_, T, LEN> {
        ArrayVecIter { v: self, idx: 0 }
    }
}

impl<T, const LEN: usize> Index<usize> for ArrayVec<T, LEN> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        assert!(
            index < self.len as usize,
            "ArrayVec index {index} is out of bounds for length {}",
            self.len
        );

        &self.arr[index]
    }
}

impl<T, const LEN: usize> IndexMut<usize> for ArrayVec<T, LEN> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        assert!(
            index < self.len as usize,
            "ArrayVec index {index} is out of bounds for length {}",
            self.len
        );

        &mut self.arr[index]
    }
}

pub struct ArrayVecIter<'vec, T, const LEN: usize> {
    v: &'vec ArrayVec<T, LEN>,
    idx: u32,
}

impl<T: Copy, const LEN: usize> Iterator for ArrayVecIter<'_, T, LEN> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.v.len {
            return None;
        }

        let value = self.v.arr[self.idx as usize];
        self.idx += 1;

        Some(value)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.v.len - self.idx) as usize;
        (remaining, Some(remaining))
    }
}
