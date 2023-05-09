use crate::apu::ApuState;
use crate::bus::Bus;
use crate::cpu::CpuState;
use crate::ppu::PpuState;
use serde::de::{SeqAccess, Visitor};
use serde::ser::{SerializeSeq, SerializeTuple};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Formatter;
use std::io;
use std::io::{BufReader, BufWriter};
use std::marker::PhantomData;
use thiserror::Error;
use tinyvec::ArrayVec;

#[derive(Serialize, Deserialize)]
pub struct EmulationState {
    pub bus: Bus,
    pub cpu_state: CpuState,
    pub ppu_state: PpuState,
    pub apu_state: ApuState,
}

#[derive(Debug, Error)]
pub enum SaveStateError {
    #[error("error serializing/deserializing state: {source}")]
    Serialization {
        #[from]
        source: bincode::Error,
    },
}

pub fn save_state<W>(
    bus: &Bus,
    cpu_state: &CpuState,
    ppu_state: &PpuState,
    apu_state: &ApuState,
    writer: W,
) -> Result<(), SaveStateError>
where
    W: io::Write,
{
    let mut writer = BufWriter::new(writer);

    bincode::serialize_into(&mut writer, bus)?;
    bincode::serialize_into(&mut writer, cpu_state)?;
    bincode::serialize_into(&mut writer, ppu_state)?;
    bincode::serialize_into(writer, apu_state)?;

    Ok(())
}

pub fn load_state<R>(reader: R) -> Result<EmulationState, SaveStateError>
where
    R: io::Read,
{
    let mut reader = BufReader::new(reader);

    let bus = bincode::deserialize_from(&mut reader)?;
    let cpu_state = bincode::deserialize_from(&mut reader)?;
    let ppu_state = bincode::deserialize_from(&mut reader)?;
    let apu_state = bincode::deserialize_from(reader)?;

    Ok(EmulationState {
        bus,
        cpu_state,
        ppu_state,
        apu_state,
    })
}

pub fn serialize_array<S, T, const N: usize>(
    array: &[T; N],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    let mut state = serializer.serialize_tuple(N)?;
    for value in array {
        state.serialize_element(value)?;
    }
    state.end()
}

struct DeserializeArrayVisitor<T, const N: usize> {
    marker: PhantomData<T>,
}

impl<T, const N: usize> DeserializeArrayVisitor<T, N> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<'de, T, const N: usize> Visitor<'de> for DeserializeArrayVisitor<T, N>
where
    T: Deserialize<'de> + Default + Copy,
{
    type Value = [T; N];

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "an array of size {N}")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut array = [T::default(); N];

        for (i, value) in array.iter_mut().enumerate() {
            let Some(elem) = seq.next_element()?
                else {
                    return Err(de::Error::custom(format!(
                        "expected array to have {N} elements, only got {i}",
                    )));
                };

            *value = elem;
        }

        if seq.next_element::<T>()?.is_some() {
            return Err(de::Error::custom(format!(
                "array has more than {N} elements",
            )));
        }

        Ok(array)
    }
}

pub fn deserialize_array<'de, D, T, const N: usize>(deserializer: D) -> Result<[T; N], D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default + Copy,
{
    deserializer.deserialize_tuple(N, DeserializeArrayVisitor::new())
}

pub fn deserialize_boxed_array<'de, D, T, const N: usize>(
    deserializer: D,
) -> Result<Box<[T; N]>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default + Copy,
{
    Ok(Box::new(deserialize_array(deserializer)?))
}

pub fn serialize_array_vec<S, T, const N: usize>(
    array_vec: &ArrayVec<[T; N]>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
    [T; N]: tinyvec::Array<Item = T>,
{
    let mut state = serializer.serialize_seq(Some(array_vec.len()))?;
    for element in array_vec {
        state.serialize_element(element)?;
    }
    state.end()
}

struct DeserializeArrayVecVisitor<T, const N: usize> {
    marker: PhantomData<T>,
}

impl<T, const N: usize> DeserializeArrayVecVisitor<T, N> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<'de, T, const N: usize> Visitor<'de> for DeserializeArrayVecVisitor<T, N>
where
    T: Deserialize<'de>,
    [T; N]: tinyvec::Array<Item = T>,
{
    type Value = ArrayVec<[T; N]>;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "a sequence representing an ArrayVec of size {N}")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = ArrayVec::new();

        while let Some(element) = seq.next_element()? {
            if result.len() == N {
                return Err(de::Error::custom(format!(
                    "sequence contains more than {N} elements"
                )));
            }

            result.push(element);
        }

        Ok(result)
    }
}

pub fn deserialize_array_vec<'de, D, T, const N: usize>(
    deserializer: D,
) -> Result<ArrayVec<[T; N]>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
    [T; N]: tinyvec::Array<Item = T>,
{
    deserializer.deserialize_seq(DeserializeArrayVecVisitor::new())
}

pub fn serialize_2d_array<S, T, const N: usize, const M: usize>(
    value: &[[T; M]; N],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    let mut state = serializer.serialize_tuple(M * N)?;
    for row in value {
        for value in row {
            state.serialize_element(value)?;
        }
    }
    state.end()
}

struct Deserialize2dArrayVisitor<T, const N: usize, const M: usize> {
    marker: PhantomData<T>,
}

impl<T, const N: usize, const M: usize> Deserialize2dArrayVisitor<T, N, M> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<'de, T, const N: usize, const M: usize> Visitor<'de> for Deserialize2dArrayVisitor<T, N, M>
where
    T: Deserialize<'de> + Default + Copy,
{
    type Value = [[T; M]; N];

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "a 2D array with {N} rows and {M} cols")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut array = [[T::default(); M]; N];

        for row in array.iter_mut() {
            for value in row.iter_mut() {
                let Some(elem) = seq.next_element()?
                    else {
                        return Err(de::Error::custom(format!("array has fewer than {M}*{N} elements")));
                    };
                *value = elem;
            }
        }

        if seq.next_element::<T>()?.is_some() {
            return Err(de::Error::custom(format!(
                "array has more than {M}*{N} elements",
            )));
        }

        Ok(array)
    }
}

pub fn deserialize_2d_array<'de, D, T, const N: usize, const M: usize>(
    deserializer: D,
) -> Result<[[T; M]; N], D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default + Copy,
{
    deserializer.deserialize_tuple(M * N, Deserialize2dArrayVisitor::new())
}
