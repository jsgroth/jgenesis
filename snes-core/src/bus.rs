use crate::memory::Memory;
use crate::ppu::Ppu;
use wdc65816_emu::traits::BusInterface;

struct Bus<'a> {
    memory: &'a mut Memory,
    ppu: &'a mut Ppu,
}

impl<'a> BusInterface for Bus<'a> {
    fn read(&mut self, address: u32) -> u8 {
        todo!("read")
    }

    fn write(&mut self, address: u32, value: u8) {
        todo!("write")
    }

    fn idle(&mut self) {
        todo!("idle")
    }

    fn nmi(&self) -> bool {
        todo!("nmi")
    }

    fn irq(&self) -> bool {
        todo!("irq")
    }
}
