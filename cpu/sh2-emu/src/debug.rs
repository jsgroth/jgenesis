use crate::Sh2;
use crate::bus::{AccessContext, BusInterface, OpSize};

pub trait Sh2Debugger {
    /// Called for all reads, including reads to internal SH-2/SH7604 addresses and reads that hit in cache
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2);

    /// Called for reads that access the external bus; should apply the read
    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32;

    /// Called for all writes, including writes to internal SH-2/SH7604 addresses
    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2);

    /// Called for writes that access the external bus; should apply the write
    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    );

    /// Called on cache miss reads; should apply the read
    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8];

    /// Called for each instruction before it executes
    fn check_execute(&mut self, pc: u32, opcode: u16, cpu: &mut Sh2);
}

/// Dummy [`Sh2Debugger`] implementation that exists only to satisfy type constraints.
/// Will panic if any methods are actually invoked
pub struct DummySh2Debugger;

impl Sh2Debugger for DummySh2Debugger {
    fn check_read<const SIZE: u8>(&mut self, _address: u32, _cpu: &mut Sh2) {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }

    fn apply_read<const SIZE: u8>(
        &mut self,
        _address: u32,
        _ctx: AccessContext,
        _cpu: &mut Sh2,
    ) -> u32 {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }

    fn check_write<const SIZE: u8>(&mut self, _address: u32, _value: u32, _cpu: &mut Sh2) {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }

    fn apply_write<const SIZE: u8>(
        &mut self,
        _address: u32,
        _value: u32,
        _ctx: AccessContext,
        _cpu: &mut Sh2,
    ) {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }

    fn apply_read_cache_line(
        &mut self,
        _address: u32,
        _ctx: AccessContext,
        _cpu: &mut Sh2,
    ) -> [u16; 8] {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }

    fn check_execute(&mut self, _pc: u32, _opcode: u16, _cpu: &mut Sh2) {
        unimplemented!("NullSh2Debugger is not a real debugger implementation")
    }
}

pub(crate) trait BusDebugExt {
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2);

    fn check_read_byte(&mut self, address: u32, cpu: &mut Sh2) {
        self.check_read::<{ OpSize::BYTE }>(address, cpu);
    }

    fn check_read_word(&mut self, address: u32, cpu: &mut Sh2) {
        self.check_read::<{ OpSize::WORD }>(address, cpu);
    }

    fn check_read_longword(&mut self, address: u32, cpu: &mut Sh2) {
        self.check_read::<{ OpSize::LONGWORD }>(address, cpu);
    }

    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32;

    fn apply_read_byte(&mut self, address: u32, ctx: AccessContext, cpu: &mut Sh2) -> u8 {
        self.apply_read::<{ OpSize::BYTE }>(address, ctx, cpu) as u8
    }

    fn apply_read_word(&mut self, address: u32, ctx: AccessContext, cpu: &mut Sh2) -> u16 {
        self.apply_read::<{ OpSize::WORD }>(address, ctx, cpu) as u16
    }

    fn apply_read_longword(&mut self, address: u32, ctx: AccessContext, cpu: &mut Sh2) -> u32 {
        self.apply_read::<{ OpSize::LONGWORD }>(address, ctx, cpu)
    }

    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2);

    fn check_write_byte(&mut self, address: u32, value: u8, cpu: &mut Sh2) {
        self.check_write::<{ OpSize::BYTE }>(address, value.into(), cpu);
    }

    fn check_write_word(&mut self, address: u32, value: u16, cpu: &mut Sh2) {
        self.check_write::<{ OpSize::WORD }>(address, value.into(), cpu);
    }

    fn check_write_longword(&mut self, address: u32, value: u32, cpu: &mut Sh2) {
        self.check_write::<{ OpSize::LONGWORD }>(address, value, cpu);
    }

    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    );

    fn apply_write_byte(&mut self, address: u32, value: u8, ctx: AccessContext, cpu: &mut Sh2) {
        self.apply_write::<{ OpSize::BYTE }>(address, value.into(), ctx, cpu);
    }

    fn apply_write_word(&mut self, address: u32, value: u16, ctx: AccessContext, cpu: &mut Sh2) {
        self.apply_write::<{ OpSize::WORD }>(address, value.into(), ctx, cpu);
    }

    fn apply_write_longword(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) {
        self.apply_write::<{ OpSize::LONGWORD }>(address, value, ctx, cpu);
    }

    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8];

    fn check_execute(&mut self, pc: u32, opcode: u16, cpu: &mut Sh2);
}

impl<Bus: BusInterface> BusDebugExt for Bus {
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2) {
        let Some(mut debugger) = self.debug_view() else { return };
        debugger.check_read::<SIZE>(address, cpu);
    }

    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32 {
        if let Some(mut debugger) = self.debug_view() {
            debugger.apply_read::<SIZE>(address, ctx, cpu)
        } else {
            self.read::<SIZE>(address, ctx)
        }
    }

    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2) {
        let Some(mut debugger) = self.debug_view() else { return };
        debugger.check_write::<SIZE>(address, value, cpu);
    }

    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) {
        if let Some(mut debugger) = self.debug_view() {
            debugger.apply_write::<SIZE>(address, value, ctx, cpu);
        } else {
            self.write::<SIZE>(address, value, ctx);
        }
    }

    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8] {
        if let Some(mut debugger) = self.debug_view() {
            debugger.apply_read_cache_line(address, ctx, cpu)
        } else {
            self.read_cache_line(address, ctx)
        }
    }

    fn check_execute(&mut self, pc: u32, opcode: u16, cpu: &mut Sh2) {
        let Some(mut debugger) = self.debug_view() else { return };
        debugger.check_execute(pc, opcode, cpu);
    }
}
