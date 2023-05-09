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
    todo!()
}

pub fn load_state<R>(reader: R) -> Result<EmulationState, SaveStateError>
where
    R: io::Read,
{
    todo!()
}