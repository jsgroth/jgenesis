use crate::BusOp;
use wdc65816_emu::traits::BusInterface;

const RAM_LEN: usize = 1 << 24;

#[derive(Debug, Clone)]
pub struct RecordingBus {
    pub(super) ram: Box<[u8; RAM_LEN]>,
    pub(super) ops: Vec<BusOp>,
}

impl RecordingBus {
    pub fn new() -> Self {
        Self { ram: vec![0; RAM_LEN].into_boxed_slice().try_into().unwrap(), ops: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.ops.clear();
    }
}

impl BusInterface for RecordingBus {
    fn read(&mut self, address: u32) -> u8 {
        let value = self.ram[address as usize];
        self.ops.push(BusOp::Read(address, value));
        value
    }

    fn write(&mut self, address: u32, value: u8) {
        self.ops.push(BusOp::Write(address, value));
        self.ram[address as usize] = value;
    }

    fn idle(&mut self) {
        self.ops.push(BusOp::Idle);
    }

    fn nmi(&self) -> bool {
        false
    }

    fn acknowledge_nmi(&mut self) {}

    fn irq(&self) -> bool {
        false
    }
}
