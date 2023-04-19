mod instructions;

pub struct CpuRegisters {
    pub accumulator: u8,
    pub x: u8,
    pub y: u8,
    pub status: u8,
    pub pc: u16,
    pub sp: u8,
}

pub struct CpuState {
    registers: CpuRegisters,
}
