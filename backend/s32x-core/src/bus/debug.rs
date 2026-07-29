use crate::WhichCpu;
use crate::api::debug::{Sega32XDebugView, Sega32XDebugger};
use crate::bus::{OtherCpu, Sh2Bus, Sh2BusGuard};
use genesis_components::cartridge::Cartridge;
use sh2_emu::Sh2;
use sh2_emu::bus::{AccessContext, BusInterface, OpSize};
use sh2_emu::debug::Sh2Debugger;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::{cmp, ptr};

pub struct DebugSh2Bus {
    bus: Sh2Bus,
    debugger: NonNull<dyn Sega32XDebugger>,
    other_sh2: *mut Sh2,
}

impl DebugSh2Bus {
    pub fn cycle_counter(&self) -> u64 {
        self.bus.cycle_counter
    }

    pub fn cycle_limit(&self) -> u64 {
        self.bus.cycle_limit
    }

    fn maybe_sync_other_cpu(&mut self, address: u32, cpu: &mut Sh2) {
        if !self.bus.need_to_sync_other(address) {
            return;
        }

        let Some(OtherCpu { cpu: other_cpu, cycle_counter: other_cycle_counter }) =
            &mut self.bus.other_sh2
        else {
            return;
        };

        let cycle_limit = cmp::min(self.bus.cycle_counter, self.bus.cycle_limit);

        unsafe {
            let mut other_bus = Self {
                bus: Sh2Bus {
                    s32x_bus: self.bus.s32x_bus,
                    cartridge: self.bus.cartridge,
                    other_sh2: None,
                    which: self.bus.which.other(),
                    cycle_counter: other_cycle_counter.read(),
                    cycle_limit,
                },
                debugger: self.debugger,
                other_sh2: cpu as *mut _,
            };

            while other_bus.bus.cycle_counter < cycle_limit {
                other_cpu.as_mut().execute(crate::api::SH2_EXECUTION_SLICE_LEN, &mut other_bus);
            }
            other_cycle_counter.write(other_bus.bus.cycle_counter);
        }
    }
}

pub struct DebugSh2BusGuard<'debug, 'bus, 'cartridge, 'other> {
    bus: DebugSh2Bus,
    _debug_marker: PhantomData<&'debug ()>,
    _bus_marker: PhantomData<&'bus ()>,
    _cartridge_marker: PhantomData<&'cartridge ()>,
    _other_marker: PhantomData<&'other ()>,
}

impl DebugSh2Bus {
    #[inline]
    pub fn create<'debug, 'bus, 'cartridge, 'other, Debugger: Sega32XDebugger>(
        bus_guard: Sh2BusGuard<'bus, 'cartridge, 'other>,
        debugger: &'debug mut Debugger,
    ) -> DebugSh2BusGuard<'debug, 'bus, 'cartridge, 'other> {
        let debugger: &mut dyn Sega32XDebugger = debugger;
        let debug_bus = DebugSh2Bus {
            bus: bus_guard.bus,
            debugger: debugger.into(),
            other_sh2: ptr::null_mut(),
        };

        DebugSh2BusGuard {
            bus: debug_bus,
            _debug_marker: PhantomData,
            _bus_marker: PhantomData,
            _cartridge_marker: PhantomData,
            _other_marker: PhantomData,
        }
    }
}

impl Deref for DebugSh2BusGuard<'_, '_, '_, '_> {
    type Target = DebugSh2Bus;

    fn deref(&self) -> &Self::Target {
        &self.bus
    }
}

impl DerefMut for DebugSh2BusGuard<'_, '_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bus
    }
}

pub struct DebugSh2BusView<'bus>(&'bus mut DebugSh2Bus);

impl DebugSh2BusView<'_> {
    fn which(&self) -> WhichCpu {
        self.0.bus.which
    }

    fn as_debug_view<'slf, 'cpu, 'ret>(
        &'slf mut self,
        cpu: &'cpu mut Sh2,
    ) -> (Sega32XDebugView<'ret>, Option<&'slf mut Cartridge>)
    where
        'slf: 'ret,
        'cpu: 'ret,
    {
        let s32x_bus = unsafe { self.0.bus.s32x_bus.as_mut() };

        let other_sh2 = unsafe {
            if !self.0.other_sh2.is_null() {
                self.0.other_sh2.as_mut().unwrap()
            } else if let Some(other_sh2) = self.0.bus.other_sh2.as_mut() {
                other_sh2.cpu.as_mut()
            } else {
                todo!("should never happen")
            }
        };

        let (sh2_master, sh2_slave) = match self.0.bus.which {
            WhichCpu::Master => (cpu, other_sh2),
            WhichCpu::Slave => (other_sh2, cpu),
        };

        let debug_view = Sega32XDebugView {
            sdram: s32x_bus.sdram.as_mut_slice(),
            sh2_master,
            sh2_slave,
            system_registers: &mut s32x_bus.registers,
            vdp: &mut s32x_bus.vdp,
            pwm: &mut s32x_bus.pwm,
        };

        let cartridge = unsafe { self.0.bus.cartridge.as_mut() };

        (debug_view, cartridge)
    }
}

impl Sh2Debugger for DebugSh2BusView<'_> {
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2) {
        let which = self.which();

        unsafe {
            let debugger = self.0.debugger.as_mut();
            if debugger.check_sh2_read_breakpoint(which, address, OpSize::enum_value::<SIZE>()) {
                let (debug_view, cartridge) = self.as_debug_view(cpu);
                debugger.handle_sh2_breakpoint(which, cartridge, debug_view);
            }
        }
    }

    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32 {
        self.0.maybe_sync_other_cpu(address, cpu);
        self.0.bus.read::<SIZE>(address, ctx)
    }

    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2) {
        let which = self.which();

        unsafe {
            let debugger = self.0.debugger.as_mut();
            if debugger.check_sh2_write_breakpoint(
                which,
                address,
                value,
                OpSize::enum_value::<SIZE>(),
            ) {
                let (debug_view, cartridge) = self.as_debug_view(cpu);
                debugger.handle_sh2_breakpoint(which, cartridge, debug_view);
            }
        }
    }

    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) {
        self.0.maybe_sync_other_cpu(address, cpu);
        self.0.bus.write::<SIZE>(address, value, ctx);
    }

    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8] {
        self.0.maybe_sync_other_cpu(address, cpu);
        self.0.bus.read_cache_line(address, ctx)
    }

    fn check_execute(&mut self, pc: u32, opcode: u16, cpu: &mut Sh2) {
        let which = self.which();

        unsafe {
            let debugger = self.0.debugger.as_mut();
            if debugger.check_sh2_execute_breakpoint(which, pc, opcode) {
                let (debug_view, cartridge) = self.as_debug_view(cpu);
                debugger.handle_sh2_breakpoint(which, cartridge, debug_view);
            }
        }
    }

    fn check_interrupt(&mut self, interrupt_level: u8, cpu: &mut Sh2) {
        let which = self.which();

        unsafe {
            let debugger = self.0.debugger.as_mut();
            if debugger.check_sh2_interrupt_breakpoint(which, interrupt_level) {
                let (debug_view, cartridge) = self.as_debug_view(cpu);
                debugger.handle_sh2_breakpoint(which, cartridge, debug_view);
            }
        }
    }
}

impl BusInterface for DebugSh2Bus {
    type DebugView<'a>
        = DebugSh2BusView<'a>
    where
        Self: 'a;

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
        Some(DebugSh2BusView(self))
    }

    sh2_emu::impl_sh2_opcode_table!(DebugSh2Bus);
}
