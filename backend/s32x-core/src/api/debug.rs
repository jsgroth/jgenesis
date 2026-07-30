use crate::WhichCpu;
use crate::api::Sega32X;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use crate::vdp::debug::VdpDebugState;
use genesis_components::cartridge::Cartridge;
use genesis_components::debug::CramEntry;
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use sh2_emu::Sh2;
use sh2_emu::bus::OpSizeEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum S32XMemoryArea {
    Sdram,
    MasterSh2Cache,
    SlaveSh2Cache,
    FrameBuffer0,
    FrameBuffer1,
    PaletteRam,
}

#[derive(Debug, Clone)]
pub struct Sega32XDebugState {
    pub sdram: Box<[u16]>,
    pub sh2_master: Sh2,
    pub sh2_slave: Sh2,
    system_registers: SystemRegisters,
    vdp: VdpDebugState,
    pwm: PwmChip,
}

impl Sega32XDebugState {
    #[must_use]
    pub fn sh2(&mut self, which: WhichCpu) -> &mut Sh2 {
        match which {
            WhichCpu::Master => &mut self.sh2_master,
            WhichCpu::Slave => &mut self.sh2_slave,
        }
    }

    #[must_use]
    pub fn m68k_rom_bank(&self) -> u8 {
        self.system_registers.m68k_rom_bank
    }

    pub fn copy_palette(&mut self, out: &mut [CramEntry]) {
        self.vdp.copy_palette(out);
    }

    pub fn dump_32x_system_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        let h_interrupt_in_vblank = self.vdp.hen_bit();
        let h_interrupt_interval = self.vdp.h_interrupt_interval();

        self.system_registers.dump(h_interrupt_in_vblank, h_interrupt_interval, callback);
    }

    pub fn dump_32x_vdp_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.vdp.dump_registers(callback);
    }

    pub fn dump_pwm_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.pwm.dump_registers(callback);
    }

    #[must_use]
    pub fn s32x_memory_view(
        &mut self,
        memory_area: S32XMemoryArea,
    ) -> Box<dyn DebugMemoryView + '_> {
        match memory_area {
            S32XMemoryArea::Sdram => Box::new(DebugWordsView(&mut self.sdram, Endian::Big)),
            S32XMemoryArea::MasterSh2Cache => Box::new(self.sh2_master.debug_cache_view()),
            S32XMemoryArea::SlaveSh2Cache => Box::new(self.sh2_slave.debug_cache_view()),
            S32XMemoryArea::FrameBuffer0 => Box::new(self.vdp.debug_frame_buffer_view(0)),
            S32XMemoryArea::FrameBuffer1 => Box::new(self.vdp.debug_frame_buffer_view(1)),
            S32XMemoryArea::PaletteRam => Box::new(self.vdp.debug_palette_ram_view()),
        }
    }
}

pub struct Sega32XDebugView<'s32x> {
    pub(crate) sdram: &'s32x mut [u16],
    pub(crate) sh2_master: &'s32x mut Sh2,
    pub(crate) sh2_slave: &'s32x mut Sh2,
    pub(crate) system_registers: &'s32x mut SystemRegisters,
    pub(crate) vdp: &'s32x mut Vdp,
    pub(crate) pwm: &'s32x mut PwmChip,
}

impl Sega32XDebugView<'_> {
    #[must_use]
    pub fn to_debug_state(&self) -> Sega32XDebugState {
        Sega32XDebugState {
            sdram: self.sdram.to_vec().into_boxed_slice(),
            sh2_master: self.sh2_master.clone(),
            sh2_slave: self.sh2_slave.clone(),
            system_registers: self.system_registers.clone(),
            vdp: self.vdp.to_debug_state(),
            pwm: self.pwm.clone(),
        }
    }

    pub fn apply_memory_edit(&mut self, memory_area: S32XMemoryArea, address: usize, value: u8) {
        match memory_area {
            S32XMemoryArea::Sdram => {
                DebugWordsView(self.sdram, Endian::Big).write(address, value);
            }
            S32XMemoryArea::MasterSh2Cache => {
                self.sh2_master.debug_cache_view().write(address, value);
            }
            S32XMemoryArea::SlaveSh2Cache => {
                self.sh2_slave.debug_cache_view().write(address, value);
            }
            S32XMemoryArea::FrameBuffer0 => {
                self.vdp.debug_frame_buffer_view(0).write(address, value);
            }
            S32XMemoryArea::FrameBuffer1 => {
                self.vdp.debug_frame_buffer_view(1).write(address, value);
            }
            S32XMemoryArea::PaletteRam => {
                self.vdp.debug_palette_ram_view().write(address, value);
            }
        }
    }
}

impl Sega32X {
    pub fn as_debug_view(&mut self) -> (Sega32XDebugView<'_>, Option<&mut Cartridge>) {
        let debug_view = Sega32XDebugView {
            sdram: self.bus.sdram.as_mut_slice(),
            sh2_master: &mut self.sh2_master,
            sh2_slave: &mut self.sh2_slave,
            system_registers: &mut self.bus.registers,
            vdp: &mut self.bus.vdp,
            pwm: &mut self.bus.pwm,
        };

        let cartridge = self.bus.cartridge.as_mut();

        (debug_view, cartridge)
    }
}

pub trait Sega32XDebugger: 'static {
    fn check_sh2_read_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        size: OpSizeEnum,
    ) -> bool;

    fn check_sh2_write_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        value: u32,
        size: OpSizeEnum,
    ) -> bool;

    fn check_sh2_execute_breakpoint(&mut self, which: WhichCpu, pc: u32, opcode: u16) -> bool;

    fn check_sh2_interrupt_breakpoint(&mut self, which: WhichCpu, interrupt_level: u8) -> bool;

    fn handle_sh2_breakpoint(
        &mut self,
        which: WhichCpu,
        cartridge: Option<&mut Cartridge>,
        debug_view: Sega32XDebugView<'_>,
    );
}

pub struct Dummy32XDebugger;

#[allow(unused_variables)]
impl Sega32XDebugger for Dummy32XDebugger {
    #[inline(always)]
    fn check_sh2_read_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        size: OpSizeEnum,
    ) -> bool {
        false
    }

    #[inline(always)]
    fn check_sh2_write_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        value: u32,
        size: OpSizeEnum,
    ) -> bool {
        false
    }

    #[inline(always)]
    fn check_sh2_execute_breakpoint(&mut self, which: WhichCpu, pc: u32, opcode: u16) -> bool {
        false
    }

    #[inline(always)]
    fn check_sh2_interrupt_breakpoint(&mut self, which: WhichCpu, interrupt_level: u8) -> bool {
        false
    }

    #[inline(always)]
    fn handle_sh2_breakpoint(
        &mut self,
        which: WhichCpu,
        cartridge: Option<&mut Cartridge>,
        debug_view: Sega32XDebugView<'_>,
    ) {
    }
}
