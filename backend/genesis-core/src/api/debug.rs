use crate::GenesisEmulator;
use jgenesis_common::frontend::{ViewableBytes, ViewableWordsBigEndian};
use m68000_emu::disassembler::Disassembly;

pub struct GenesisDebugView<'a>(&'a mut GenesisEmulator);

impl GenesisEmulator {
    #[must_use]
    pub fn debug_view(&mut self) -> GenesisDebugView<'_> {
        GenesisDebugView(self)
    }
}

impl<'emu> GenesisDebugView<'emu> {
    #[must_use]
    pub fn working_ram_view(self) -> ViewableBytes<'emu> {
        self.0.memory.debug_main_ram_view()
    }

    #[must_use]
    pub fn audio_ram_view(self) -> ViewableBytes<'emu> {
        self.0.memory.debug_audio_ram_view()
    }

    #[must_use]
    pub fn vram_view(self) -> ViewableBytes<'emu> {
        self.0.vdp.debug_vram_view()
    }

    #[must_use]
    pub fn cram_view(self) -> ViewableWordsBigEndian<'emu> {
        self.0.vdp.debug_cram_view()
    }

    #[must_use]
    pub fn vsram_view(self) -> ViewableBytes<'emu> {
        self.0.vdp.debug_vsram_view()
    }

    #[inline]
    #[must_use]
    pub fn m68k_pc(&self) -> u32 {
        self.0.m68k.pc()
    }

    #[inline]
    pub fn m68k_disassemble(&self, start: u32, out: &mut Vec<Disassembly>, num_instructions: u32) {
        let mut pc = start;
        for _ in 0..num_instructions {
            let read_word: &dyn Fn(u32) -> u16 = &|address| self.0.memory.peek_word(address);
            let disassembly = m68000_emu::disassembler::disassemble(pc, read_word);

            pc = disassembly.new_pc;

            out.push(disassembly);
        }
    }
}
