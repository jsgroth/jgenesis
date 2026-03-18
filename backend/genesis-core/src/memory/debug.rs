use crate::api::debug::{
    CartridgeDebugView, GenesisCpu, GenesisDebuggerFor68k, GenesisDebuggerForZ80,
    GenesisEmulatorDebugView,
};
use crate::cartridge::Cartridge;
use crate::memory::{MainBus, PhysicalMedium};
use m68000_emu::M68000;
use m68000_emu::debug::M68000Debugger;
use z80_emu::Z80;
use z80_emu::debug::Z80Debugger;
use z80_emu::traits::InterruptLine;

pub struct DebugMainBus<'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger> {
    pub bus: &'busref mut MainBus<'bus, Medium, REFRESH_INTERVAL>,
    pub debugger: Debugger,
}

pub trait MainBus68kDebugger<Medium> {
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

pub trait MainBusZ80Debugger<Medium> {
    fn check_read_breakpoint(&self, address: u16) -> bool;

    fn check_write_breakpoint(&self, address: u16) -> bool;

    fn check_execute_breakpoint(&self, pc: u16) -> bool;

    fn check_break_step(&mut self) -> bool;

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut Z80,
        bus: &mut MainBus<'_, Medium, REFRESH_INTERVAL>,
    );
}

pub struct DebugMainBusView<'slf, 'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger>(
    &'slf mut DebugMainBus<'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>,
);

impl<const REFRESH_INTERVAL: u32, Medium, Debugger> M68000Debugger
    for DebugMainBusView<'_, '_, '_, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBus68kDebugger<Medium>,
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

impl<const REFRESH_INTERVAL: u32, Medium, Debugger> Z80Debugger
    for DebugMainBusView<'_, '_, '_, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBusZ80Debugger<Medium>,
{
    fn check_read_memory(&mut self, address: u16, cpu: &mut Z80) {
        if self.0.debugger.check_read_breakpoint(address) {
            log::info!("Z80 {address:04X} read triggered breakpoint");
            self.0.debugger.handle_breakpoint(cpu, self.0.bus);
        }
    }

    fn check_read_io(&mut self, _address: u16, _cpu: &mut Z80) {
        // I/O address space is unused in Genesis
    }

    fn check_write_memory(&mut self, address: u16, value: u8, cpu: &mut Z80) {
        if self.0.debugger.check_write_breakpoint(address) {
            log::info!("Z80 {address:04X} write {value:02X} triggered breakpoint");
            self.0.debugger.handle_breakpoint(cpu, self.0.bus);
        }
    }

    fn check_write_io(&mut self, _address: u16, _value: u8, _cpu: &mut Z80) {
        // I/O address space is unused in Genesis
    }

    fn check_execute(&mut self, pc: u16, cpu: &mut Z80) {
        let check_step = self.0.debugger.check_break_step();
        let check_execute = self.0.debugger.check_execute_breakpoint(pc);

        if check_step || check_execute {
            if check_execute {
                log::info!("Z80 PC={pc:04X} triggered execute breakpoint");
                self.0.debugger.handle_breakpoint(cpu, self.0.bus);
            }
        }
    }
}

impl<'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger> m68000_emu::BusInterface
    for DebugMainBus<'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBus68kDebugger<Medium>,
{
    type DebugView<'a>
        = DebugMainBusView<'a, 'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
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
        Some(DebugMainBusView(self))
    }
}

impl<'busref, 'bus, const REFRESH_INTERVAL: u32, Medium, Debugger> z80_emu::BusInterface
    for DebugMainBus<'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
where
    Medium: PhysicalMedium,
    Debugger: MainBusZ80Debugger<Medium>,
{
    type DebugView<'a>
        = DebugMainBusView<'a, 'busref, 'bus, REFRESH_INTERVAL, Medium, Debugger>
    where
        Self: 'a;

    fn read_memory(&mut self, address: u16) -> u8 {
        self.bus.read_memory(address)
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        self.bus.write_memory(address, value);
    }

    fn read_io(&mut self, address: u16) -> u8 {
        self.bus.read_io(address)
    }

    fn write_io(&mut self, address: u16, value: u8) {
        self.bus.write_io(address, value);
    }

    fn nmi(&self) -> InterruptLine {
        self.bus.nmi()
    }

    fn int(&self) -> InterruptLine {
        self.bus.int()
    }

    fn busreq(&self) -> bool {
        self.bus.busreq()
    }

    fn reset(&self) -> bool {
        self.bus.reset()
    }

    fn debug_view(&mut self) -> Option<Self::DebugView<'_>> {
        Some(DebugMainBusView(self))
    }
}

impl MainBus68kDebugger<Cartridge> for GenesisDebuggerFor68k<'_> {
    fn check_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.m68k_breakpoints().check_read::<WORD>(address)
    }

    fn check_write_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.m68k_breakpoints().check_write::<WORD>(address)
    }

    fn check_execute_breakpoint(&mut self, pc: u32) -> bool {
        self.debugger.update_68k_pc(pc);
        self.debugger.m68k_breakpoints().check_execute(pc)
    }

    fn check_break_step(&mut self) -> bool {
        self.debugger.check_68k_break_step()
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

        self.debugger.handle_breakpoint(GenesisCpu::M68k, &mut debug_view);
    }
}

impl MainBusZ80Debugger<Cartridge> for GenesisDebuggerForZ80<'_> {
    fn check_read_breakpoint(&self, address: u16) -> bool {
        self.debugger.z80_breakpoints().check_read(address)
    }

    fn check_write_breakpoint(&self, address: u16) -> bool {
        self.debugger.z80_breakpoints().check_write(address)
    }

    fn check_execute_breakpoint(&self, pc: u16) -> bool {
        self.debugger.z80_breakpoints().check_execute(pc)
    }

    fn check_break_step(&mut self) -> bool {
        self.debugger.check_z80_break_step()
    }

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut Z80,
        bus: &mut MainBus<'_, Cartridge, REFRESH_INTERVAL>,
    ) {
        let mut debug_view = GenesisEmulatorDebugView {
            m68k: self.m68k,
            z80: cpu,
            memory: bus.memory.as_debug_view(|cartridge| CartridgeDebugView { cartridge }),
            vdp: bus.vdp,
        };
        self.debugger.handle_breakpoint(GenesisCpu::Z80, &mut debug_view);
    }
}
