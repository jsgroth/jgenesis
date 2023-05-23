use crate::api::EmulationState;
use crate::apu::ApuState;
use crate::bus::Bus;
use crate::cpu::CpuState;
use crate::ppu::PpuState;
use bincode::config::{Fixint, LittleEndian};
use bincode::error::{DecodeError, EncodeError};
use std::io;
use std::io::{BufReader, BufWriter};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveStateError {
    #[error("error saving state: {source}")]
    Serialization {
        #[from]
        source: EncodeError,
    },
    #[error("error loading state: {source}")]
    Deserialization {
        #[from]
        source: DecodeError,
    },
}

const BINCODE_CONFIG: bincode::config::Configuration<LittleEndian, Fixint> =
    bincode::config::standard()
        .with_little_endian()
        .with_fixed_int_encoding();

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

    bincode::encode_into_std_write(bus, &mut writer, BINCODE_CONFIG)?;
    bincode::encode_into_std_write(cpu_state, &mut writer, BINCODE_CONFIG)?;
    bincode::encode_into_std_write(ppu_state, &mut writer, BINCODE_CONFIG)?;
    bincode::encode_into_std_write(apu_state, &mut writer, BINCODE_CONFIG)?;

    Ok(())
}

pub fn load_state<R>(reader: R) -> Result<EmulationState, SaveStateError>
where
    R: io::Read,
{
    let mut reader = BufReader::new(reader);

    let bus = bincode::decode_from_std_read(&mut reader, BINCODE_CONFIG)?;
    let cpu_state = bincode::decode_from_std_read(&mut reader, BINCODE_CONFIG)?;
    let ppu_state = bincode::decode_from_std_read(&mut reader, BINCODE_CONFIG)?;
    let apu_state = bincode::decode_from_std_read(&mut reader, BINCODE_CONFIG)?;

    Ok(EmulationState {
        bus,
        cpu_state,
        ppu_state,
        apu_state,
    })
}
