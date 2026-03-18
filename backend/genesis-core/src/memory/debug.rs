use crate::api::debug::{CartridgeDebugView, GenesisDebuggerFor68k, GenesisEmulatorDebugView};
use crate::cartridge::Cartridge;
use crate::memory::{MainBus, PhysicalMedium};
use m68000_emu::debug::M68000Debugger;
use m68000_emu::{BusInterface, M68000};

pub struct Debug68kBus<'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger> {
    pub bus: &'busref mut MainBus<'bus, Medium, REFRESH_INTERVAL>,
    pub debugger: Debugger,
}

pub trait MainBusDebugger<Medium> {
    fn check_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool;

    fn check_write_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool;

    fn check_execute_breakpoint(&mut self, pc: u32) -> bool;

    fn check_break_step(&mut self) -> bool;

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut M68000,
        bus: &mut MainBus<'_, Medium, REFRESH_INTERVAL>,
    );
}

pub struct M68kDebugView<'slf, 'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger>(
    &'slf mut Debug68kBus<'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>,
);

impl<const REFRESH_INTERVAL: u32, Medium, Debugger> M68000Debugger
    for M68kDebugView<'_, '_, '_, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBusDebugger<Medium>,
{
    fn check_read<const WORD: bool>(&mut self, address: u32, cpu: &mut M68000) {
        let address = address & 0xFFFFFF;

        if self.0.debugger.check_read_breakpoint::<WORD>(address) {
            log::info!(
                "68000 {address:06X} {} read triggered breakpoint",
                if WORD { "word" } else { "byte" }
            );

            self.0.debugger.handle_breakpoint(cpu, self.0.bus);
        }
    }

    fn check_write<const WORD: bool>(&mut self, address: u32, value: u16, cpu: &mut M68000) {
        let address = address & 0xFFFFFF;

        if self.0.debugger.check_write_breakpoint::<WORD>(address) {
            log::info!(
                "68000 {address:06X} {} write {value:04X} triggered breakpoint",
                if WORD { "word" } else { "byte" }
            );

            self.0.debugger.handle_breakpoint(cpu, self.0.bus);
        }
    }

    fn check_execute(&mut self, pc: u32, cpu: &mut M68000) {
        let pc = pc & 0xFFFFFF;

        let check_step = self.0.debugger.check_break_step();
        let check_execute = self.0.debugger.check_execute_breakpoint(pc);

        if check_step || check_execute {
            if check_execute {
                log::info!("68000 PC={pc:06X} triggered execute breakpoint");
            }

            self.0.debugger.handle_breakpoint(cpu, self.0.bus);
        }
    }
}

impl<'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger> BusInterface
    for Debug68kBus<'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBusDebugger<Medium>,
{
    type DebugView<'a>
        = M68kDebugView<'a, 'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
    where
        Self: 'a;

    fn read_byte(&mut self, address: u32) -> u8 {
        self.bus.read_byte(address)
    }

    fn read_word(&mut self, address: u32) -> u16 {
        self.bus.read_word(address)
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        self.bus.write_byte(address, value);
    }

    fn write_word(&mut self, address: u32, value: u16) {
        self.bus.write_word(address, value);
    }

    fn interrupt_level(&self) -> u8 {
        self.bus.interrupt_level()
    }

    fn acknowledge_interrupt(&mut self, interrupt_level: u8) {
        self.bus.acknowledge_interrupt(interrupt_level);
    }

    fn halt(&self) -> bool {
        self.bus.halt()
    }

    fn reset(&self) -> bool {
        self.bus.reset()
    }

    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(M68kDebugView(self))
    }
}

impl MainBusDebugger<Cartridge> for GenesisDebuggerFor68k<'_> {
    fn check_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.check_read_breakpoint::<WORD>(address)
    }

    fn check_write_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.check_write_breakpoint::<WORD>(address)
    }

    fn check_execute_breakpoint(&mut self, pc: u32) -> bool {
        self.debugger.update_pc(pc);
        self.debugger.check_execute_breakpoint(pc)
    }

    fn check_break_step(&mut self) -> bool {
        self.debugger.check_break_step()
    }

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut M68000,
        bus: &mut MainBus<'_, Cartridge, REFRESH_INTERVAL>,
    ) {
        let mut debug_view = GenesisEmulatorDebugView {
            m68k: cpu,
            z80: self.z80,
            memory: bus.memory.as_debug_view(|cartridge| CartridgeDebugView { cartridge }),
            vdp: &mut bus.vdp,
        };

        self.debugger.handle_68k_breakpoint(&mut debug_view);
    }
}
