use crate::GenesisVdp;
use crate::api::debug::{
    Sega32XDebugger, Sega32XDebuggerForSh2, Sega32XDebuggerForSh2Raw, Sega32XEmulatorDebugView,
    Sega32XMediumView, Sh2Breakpoints,
};
use crate::bus::{OtherCpu, Sh2Bus, WhichCpu};
use crate::core::Sega32XBus;
use genesis_core::api::debug::{BaseGenesisDebugView, GenesisMemoryDebugView};
use sh2_emu::Sh2;
use sh2_emu::bus::{AccessContext, BusInterface, OpSize};
use sh2_emu::debug::Sh2Debugger;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

pub(crate) struct DebugSh2Bus {
    pub(crate) bus: Sh2Bus,
    pub(crate) debugger: Sega32XDebuggerForSh2Raw,
    pub(crate) other_sh2: Option<NonNull<Sh2>>,
}

impl DebugSh2Bus {
    pub(crate) fn create<'bus, 'other, 'debug, 'genram, 'genvdp>(
        s32x_bus: &'bus mut Sega32XBus,
        which: WhichCpu,
        cycle_counter: u64,
        cycle_limit: u64,
        other_sh2: Option<(&'other mut Sh2, &'other mut u64)>,
        genesis_vdp: &'genvdp mut GenesisVdp,
        debugger: &'debug mut Sega32XDebuggerForSh2<'genram>,
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
                        debugger: None,
                    },
                    debugger: debugger.as_raw(genesis_vdp),
                    other_sh2: None,
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

impl<'a> Sh2BusDebugView<'a> {
    fn as_32x_debug_view_and_debugger<'view, 'slf, 'cpu>(
        &'slf mut self,
        cpu: &'cpu mut Sh2,
    ) -> (Sega32XEmulatorDebugView<'view>, &'slf mut Sega32XDebugger)
    where
        'cpu: 'view,
        'slf: 'view,
        'a: 'view,
    {
        unsafe {
            let mut other_sh2 = match self.0.bus.other_sh2 {
                Some(OtherCpu { cpu, .. }) => cpu,
                None => self
                    .0
                    .other_sh2
                    .expect("other_sh2 is None on both inner bus and debug bus; this is a bug"),
            };
            let other_sh2 = other_sh2.as_mut();

            let (sh2_master, sh2_slave) = match self.0.bus.which {
                WhichCpu::Master => (cpu, other_sh2),
                WhichCpu::Slave => (other_sh2, cpu),
            };

            let s32x_bus = self.0.bus.s32x_bus.as_mut();

            let debug_view = Sega32XEmulatorDebugView {
                genesis: BaseGenesisDebugView::new(
                    self.0.debugger.m68k.as_mut(),
                    self.0.debugger.z80.as_mut(),
                    GenesisMemoryDebugView {
                        medium_view: Sega32XMediumView {
                            cartridge_rom: s32x_bus.cartridge.debug_rom_view(),
                            sdram: s32x_bus.sdram.as_mut_slice(),
                            sh2_master,
                            sh2_slave,
                            system_registers: &mut s32x_bus.registers,
                            s32x_vdp: &mut s32x_bus.vdp,
                            pwm: &mut s32x_bus.pwm,
                        },
                        working_ram: self.0.debugger.working_ram.as_mut(),
                        audio_ram: self.0.debugger.audio_ram.as_mut(),
                    },
                    self.0.debugger.vdp.as_mut(),
                ),
            };

            let debugger = self.0.debugger.debugger.as_mut();

            (debug_view, debugger)
        }
    }

    fn breakpoints(&self) -> &Sh2Breakpoints {
        unsafe { self.0.debugger.debugger.as_ref().breakpoints(self.0.bus.which) }
    }

    fn check_break_step(&mut self, which: WhichCpu) -> bool {
        unsafe { self.0.debugger.debugger.as_mut().should_break_on_step(which) }
    }

    fn handle_breakpoint(&mut self, cpu: &mut Sh2) {
        let which = self.0.bus.which;
        let (mut debug_view, debugger) = self.as_32x_debug_view_and_debugger(cpu);
        debugger.handle_breakpoint(which, &mut debug_view);
    }

    fn with_debugger_on_inner_bus<T>(
        &mut self,
        cpu: &mut Sh2,
        op: impl FnOnce(&mut Sh2Bus) -> T,
    ) -> T {
        self.0.bus.debugger = Some((self.0.debugger.clone(), cpu.into()));
        let value = op(&mut self.0.bus);
        self.0.bus.debugger = None;

        value
    }
}

impl Sh2Debugger for Sh2BusDebugView<'_> {
    fn check_read<const SIZE: u8>(&mut self, address: u32, cpu: &mut Sh2) {
        if self.breakpoints().should_break_read::<SIZE>(address) {
            log::info!(
                "[{:?}] {address:08X} {} read triggered breakpoint",
                self.0.bus.which,
                OpSize::display::<SIZE>()
            );
            self.handle_breakpoint(cpu);
        }
    }

    fn apply_read<const SIZE: u8>(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> u32 {
        self.with_debugger_on_inner_bus(cpu, |bus| bus.read::<SIZE>(address, ctx))
    }

    fn check_write<const SIZE: u8>(&mut self, address: u32, value: u32, cpu: &mut Sh2) {
        if self.breakpoints().should_break_write::<SIZE>(address) {
            log::info!(
                "[{:?}] {address:08X} {} write {value:08X} triggered breakpoint",
                self.0.bus.which,
                OpSize::display::<SIZE>()
            );
            self.handle_breakpoint(cpu);
        }
    }

    fn apply_write<const SIZE: u8>(
        &mut self,
        address: u32,
        value: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) {
        self.with_debugger_on_inner_bus(cpu, |bus| bus.write::<SIZE>(address, value, ctx));
    }

    fn apply_read_cache_line(
        &mut self,
        address: u32,
        ctx: AccessContext,
        cpu: &mut Sh2,
    ) -> [u16; 8] {
        self.with_debugger_on_inner_bus(cpu, |bus| bus.read_cache_line(address, ctx))
    }

    fn check_execute(&mut self, pc: u32, _opcode: u16, cpu: &mut Sh2) {
        let which = self.0.bus.which;

        unsafe {
            self.0.debugger.debugger.as_mut().update_sh2_pc(which, pc);
        }

        let break_step = self.check_break_step(which);
        let break_execute = self.breakpoints().should_break_execute(pc);

        if break_execute {
            log::info!("[{which:?}] PC={pc:08X} triggered execute breakpoint");
        }

        if break_step || break_execute {
            self.handle_breakpoint(cpu);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Sega32XEmulatorConfig;
    use crate::api::debug::{S32XMemoryArea, Sega32XDebugCommand, Sh2Breakpoint};
    use crate::core::SerialInterface;
    use crate::pwm::PwmChip;
    use crate::registers::SystemRegisters;
    use crate::vdp::Vdp;
    use genesis_core::api::debug::GenesisMemoryArea;
    use genesis_core::cartridge::Cartridge;
    use genesis_core::vdp::DarkenColors;
    use jgenesis_common::boxedarray::BoxedWordArray;
    use jgenesis_common::frontend::TimingMode;
    use m68000_emu::M68000;
    use z80_emu::Z80;

    const COMM_PORT_0: u32 = 0x20004020;

    // Meant to be run through miri:
    //   $ cargo +nightly miri test -p s32x_core memory_model
    //
    // This test is not exhaustive but should hit most of the major code paths that use unsafe blocks.
    #[test]
    fn check_for_memory_model_violations() {
        let emu_config = Sega32XEmulatorConfig::default();

        let (state_sender, _state_receiver) = jgenesis_common::sync::new_shared_var();
        let (mut debugger, debugger_handle) = Sega32XDebugger::new(state_sender);

        let mut s32x_bus = Sega32XBus {
            cartridge: Cartridge::from_rom(vec![0xFF; 1024], None, None),
            vdp: Vdp::new(TimingMode::Ntsc, &emu_config),
            pwm: PwmChip::new(TimingMode::Ntsc),
            registers: SystemRegisters::new(),
            sdram: BoxedWordArray::new(),
            serial: SerialInterface::default(),
        };

        let mut sh2_master = Sh2::new("Master".into());
        let mut sh2_slave = Sh2::new("Slave".into());
        let mut sh2_slave_cycles = 0;

        let mut bus = Sh2Bus::create(
            &mut s32x_bus,
            WhichCpu::Master,
            0,
            1024,
            Some((&mut sh2_slave, &mut sh2_slave_cycles)),
        );

        for _ in 0..10 {
            sh2_master.execute(50, &mut *bus);
        }

        bus.read_word(COMM_PORT_0, AccessContext::Fetch);

        bus.cycle_counter = 0;
        bus.other_sh2 = None;

        for _ in 0..10 {
            sh2_master.execute(50, &mut *bus);
        }

        bus.read_word(COMM_PORT_0, AccessContext::Fetch);

        sh2_slave_cycles = 0;

        let mut m68k = M68000::default();
        let mut z80 = Z80::new();
        let mut genesis_vdp =
            GenesisVdp::new(TimingMode::Ntsc, emu_config.genesis.to_vdp_config(DarkenColors::Yes));
        let mut working_ram = vec![0; 64 * 1024];
        let mut audio_ram = vec![0; 8 * 1024];

        let mut debugger_for_sh2 = debugger.for_sh2_exec(
            &mut m68k,
            &mut z80,
            working_ram.as_mut_slice(),
            audio_ram.as_mut_slice(),
        );
        let mut debug_bus = DebugSh2Bus::create(
            &mut s32x_bus,
            WhichCpu::Master,
            0,
            1024,
            Some((&mut sh2_slave, &mut sh2_slave_cycles)),
            &mut genesis_vdp,
            &mut debugger_for_sh2,
        );

        for _ in 0..10 {
            sh2_master.execute(50, &mut *debug_bus);
        }

        debug_bus.debug_view().unwrap().apply_read::<{ OpSize::WORD }>(
            COMM_PORT_0,
            AccessContext::Fetch,
            &mut sh2_master,
        );

        {
            let mut debug_view = debug_bus.debug_view().unwrap();
            let (mut s32x_debug_view, debugger) =
                debug_view.as_32x_debug_view_and_debugger(&mut sh2_master);

            debugger_handle
                .send_command(Sega32XDebugCommand::Edit32XMemory(
                    S32XMemoryArea::PaletteRam,
                    0,
                    0xFF,
                ))
                .unwrap();
            debugger.process_commands(&mut s32x_debug_view);
        }

        // Run SH-2 again after debugger interaction
        debug_bus.bus.bus.cycle_counter = 0;
        unsafe {
            debug_bus.bus.bus.other_sh2.as_mut().unwrap().cycle_counter.write(0);
        }

        for _ in 0..10 {
            sh2_master.execute(50, &mut *debug_bus);
        }

        debug_bus.debug_view().unwrap().apply_read::<{ OpSize::WORD }>(
            COMM_PORT_0,
            AccessContext::Fetch,
            &mut sh2_master,
        );

        debugger_handle
            .send_command(Sega32XDebugCommand::UpdateBreakpoints(
                WhichCpu::Master,
                vec![Sh2Breakpoint {
                    start_address: COMM_PORT_0,
                    end_address: COMM_PORT_0,
                    read: true,
                    write: true,
                    execute: true,
                }],
            ))
            .unwrap();

        {
            let mut debug_view = debug_bus.debug_view().unwrap();
            let (mut s32x_debug_view, debugger) =
                debug_view.as_32x_debug_view_and_debugger(&mut sh2_master);
            debugger.process_commands(&mut s32x_debug_view);
        }

        for memory_area in GenesisMemoryArea::ALL {
            debugger_handle
                .send_command(Sega32XDebugCommand::EditGenesisMemory(memory_area, 0, 0))
                .unwrap();
        }

        for memory_area in S32XMemoryArea::ALL {
            debugger_handle
                .send_command(Sega32XDebugCommand::Edit32XMemory(memory_area, 0, 0))
                .unwrap();
        }

        debugger_handle.send_command(Sega32XDebugCommand::BreakResume).unwrap();

        debug_bus
            .debug_view()
            .unwrap()
            .check_read::<{ OpSize::WORD }>(COMM_PORT_0, &mut sh2_master);
    }
}
