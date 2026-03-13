use crate::GenesisVdp;
use crate::api::debug::{Sega32XDebuggerGenesisRam, Sega32XDebuggerGenesisRamRaw};
use crate::bus::{OtherCpu, Sh2Bus, WhichCpu};
use crate::core::Sega32XBus;
use sh2_emu::Sh2;
use sh2_emu::bus::{AccessContext, BusInterface};
use sh2_emu::debug::Sh2Debugger;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

pub(crate) struct DebugSh2Bus {
    pub(crate) bus: Sh2Bus,
    pub(crate) debugger: Sega32XDebuggerGenesisRamRaw,
}

impl DebugSh2Bus {
    pub(crate) fn create<'bus, 'other, 'debug, 'genram, 'genvdp>(
        s32x_bus: &'bus mut Sega32XBus,
        which: WhichCpu,
        cycle_counter: u64,
        cycle_limit: u64,
        other_sh2: Option<(&'other mut Sh2, &'other mut u64)>,
        genesis_vdp: &'genvdp mut GenesisVdp,
        debugger: &'debug mut Sega32XDebuggerGenesisRam<'genram>,
    ) -> DebugSh2BusGuard<'bus, 'other, 'debug, 'genram, 'genvdp> {
        unsafe {
            DebugSh2BusGuard {
                bus: Self {
                    bus: Sh2Bus {
                        s32x_bus: s32x_bus.into(),
                        other_sh2: other_sh2.map(|(cpu, cycle_counter)| OtherCpu {
                            cpu: cpu.into(),
                            cycle_counter: cycle_counter.into(),
                        }),
                        which,
                        cycle_counter,
                        cycle_limit,
                    },
                    debugger: debugger.to_raw(genesis_vdp),
                },
                _bus_marker: PhantomData,
                _other_marker: PhantomData,
                _debug_marker: PhantomData,
                _genram_marker: PhantomData,
                _genvdp_marker: PhantomData,
            }
        }
    }

    pub fn cycle_counter(&self) -> u64 {
        self.bus.cycle_counter
    }
}

sh2_emu::impl_sh2_lookup_table!(DebugSh2Bus);

pub(crate) struct DebugSh2BusGuard<'bus, 'other, 'debug, 'genram, 'genvdp> {
    bus: DebugSh2Bus,
    _bus_marker: PhantomData<&'bus ()>,
    _other_marker: PhantomData<&'other ()>,
    _debug_marker: PhantomData<&'debug ()>,
    _genram_marker: PhantomData<&'genram ()>,
    _genvdp_marker: PhantomData<&'genvdp ()>,
}

impl Deref for DebugSh2BusGuard<'_, '_, '_, '_, '_> {
    type Target = DebugSh2Bus;

    fn deref(&self) -> &Self::Target {
        &self.bus
    }
}

impl DerefMut for DebugSh2BusGuard<'_, '_, '_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bus
    }
}

pub(crate) struct Sh2BusDebugView<'a>(&'a mut DebugSh2Bus);

impl Sh2Debugger for Sh2BusDebugView<'_> {
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2) {}

    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32 {
        self.0.read::<SIZE>(address, ctx)
    }

    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2) {}

    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) {
        self.0.write::<SIZE>(address, value, ctx);
    }

    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8] {
        self.0.read_cache_line(address, ctx)
    }

    fn check_execute(&mut self, pc: u32, opcode: u16, cpu: &mut Sh2) {}
}

impl BusInterface for DebugSh2Bus {
    type DebugView<'a> = Sh2BusDebugView<'a>;

    fn read<const SIZE: u8>(&mut self, address: u32, ctx: AccessContext) -> u32 {
        self.bus.read::<SIZE>(address, ctx)
    }

    fn read_cache_line(&mut self, address: u32, ctx: AccessContext) -> [u16; 8] {
        self.bus.read_cache_line(address, ctx)
    }

    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, ctx: AccessContext) {
        self.bus.write::<SIZE>(address, value, ctx);
    }

    fn reset(&self) -> bool {
        self.bus.reset()
    }

    fn interrupt_level(&self) -> u8 {
        self.bus.interrupt_level()
    }

    fn dma_request_0(&self) -> bool {
        self.bus.dma_request_0()
    }

    fn dma_request_1(&self) -> bool {
        self.bus.dma_request_1()
    }

    fn acknowledge_dreq_1(&mut self) {
        self.bus.acknowledge_dreq_1();
    }

    fn serial_rx(&mut self) -> Option<u8> {
        self.bus.serial_rx()
    }

    fn serial_tx(&mut self, value: u8) {
        self.bus.serial_tx(value);
    }

    fn increment_cycle_counter(&mut self, cycles: u64) {
        self.bus.increment_cycle_counter(cycles);
    }

    fn should_stop_execution(&self) -> bool {
        self.bus.should_stop_execution()
    }

    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(Sh2BusDebugView(self))
    }
}
