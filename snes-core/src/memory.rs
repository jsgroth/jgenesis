const MAIN_RAM_LEN: usize = 128 * 1024;

type MainRam = [u8; MAIN_RAM_LEN];

#[derive(Debug, Clone)]
pub struct Memory {
    main_ram: Box<MainRam>,
    cpu_registers: CpuInternalRegisters,
}

#[derive(Debug, Clone)]
struct CpuInternalRegisters {}
