//! SH-2 load and store instructions

use crate::Sh2;
use crate::bus::BusInterface;
use crate::instructions::{extend_i8, extend_i16, rm, rn};

impl<Bus: BusInterface> Sh2<Bus> {
    // MOV Rm, Rn
    // Load from a register
    pub(crate) fn mov_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gpr[m];
    }

    // MOV #imm, Rn
    // Loads an 8-bit immediate (sign extended) into a register
    pub(crate) fn mov_b_immediate_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = opcode as i8 as u32;
    }

    // MOV.B @Rm, Rn
    // Loads a byte from memory using indirect register addressing
    pub(crate) fn mov_b_indirect_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.read_byte(self.registers.gpr[m], bus);
        self.registers.gpr[n] = extend_i8(value);
    }

    // MOV.W @Rm, Rn
    // Loads a word from memory using indirect register addressing
    pub(crate) fn mov_w_indirect_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.read_word(self.registers.gpr[m], bus);
        self.registers.gpr[n] = extend_i16(value);
    }

    // MOV.L @Rm, Rn
    // Loads a longword from memory using indirect register addressing
    pub(crate) fn mov_l_indirect_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        self.registers.gpr[n] = self.read_longword(self.registers.gpr[m], bus);
    }

    // MOV.B Rm, @Rn
    // Stores a byte into memory using indirect register addressing
    pub(crate) fn mov_b_rm_indirect(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m] as u8;
        self.write_byte(self.registers.gpr[n], value, bus);
    }

    // MOV.W Rm, @Rn
    // Stores a word into memory using indirect register addressing
    pub(crate) fn mov_w_rm_indirect(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m] as u16;
        self.write_word(self.registers.gpr[n], value, bus);
    }

    // MOV.L Rm, @Rn
    // Stores a longword into memory using indirect register addressing
    pub(crate) fn mov_l_rm_indirect(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m];
        self.write_longword(self.registers.gpr[n], value, bus);
    }

    // MOV.B @Rm+, Rn
    // Loads a byte into a register using post-increment indirect register addressing
    pub(crate) fn mov_b_postinc_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.read_byte(self.registers.gpr[m], bus);
        self.registers.gpr[m] = self.registers.gpr[m].wrapping_add(1);

        self.registers.gpr[n] = extend_i8(value);
    }

    // MOV.W @Rm+, Rn
    // Loads a word into a register using post-increment indirect register addressing
    pub(crate) fn mov_w_postinc_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.read_word(self.registers.gpr[m], bus);
        self.registers.gpr[m] = self.registers.gpr[m].wrapping_add(2);

        self.registers.gpr[n] = extend_i16(value);
    }

    // MOV.L @Rm+, Rn
    // Loads a longword into a register using post-increment indirect register addressing
    pub(crate) fn mov_l_postinc_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.read_longword(self.registers.gpr[m], bus);
        self.registers.gpr[m] = self.registers.gpr[m].wrapping_add(4);

        self.registers.gpr[n] = value;
    }

    // MOV.B Rm, @-Rn
    // Stores a byte into memory using pre-decrement indirect register addressing
    pub(crate) fn mov_b_rm_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m] as u8;

        let address = self.registers.gpr[n].wrapping_sub(1);
        self.registers.gpr[n] = address;

        self.write_byte(address, value, bus);
    }

    // MOV.W Rm, @-Rn
    // Stores a word into memory using pre-decrement indirect register addressing
    pub(crate) fn mov_w_rm_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m] as u16;

        let address = self.registers.gpr[n].wrapping_sub(2);
        self.registers.gpr[n] = address;

        self.write_word(address, value, bus);
    }

    // MOV.L Rm, @-Rn
    // Stores a longword into memory using pre-decrement indirect register addressing
    pub(crate) fn mov_l_rm_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m];

        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, value, bus);
    }

    // MOV.W @(disp,PC), Rn
    // Loads a word (sign extended) into a register using PC relative with displacement addressing
    pub(crate) fn mov_w_immediate_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 1;
        let address = self.registers.next_pc.wrapping_add(disp.into());
        let value = self.read_word(address, bus);

        let n = rn(opcode);
        self.registers.gpr[n] = extend_i16(value);
    }

    // MOV.L @(disp,PC), Rn
    // Loads a longword into a register using PC relative with displacement addressing
    pub(crate) fn mov_l_immediate_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 2;
        let address = (self.registers.next_pc & !3).wrapping_add(disp.into());
        let value = self.read_longword(address, bus);

        let n = rn(opcode);
        self.registers.gpr[n] = value;
    }

    // MOV.B R0, @(disp,Rn)
    // Stores a byte into memory using indirect register with displacement addressing
    pub(crate) fn mov_b_r0_rn_displacement(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = opcode & 0xF;
        let n = rm(opcode);
        let address = self.registers.gpr[n].wrapping_add(disp.into());
        self.write_byte(address, self.registers.gpr[0] as u8, bus);
    }

    // MOV.W R0, @(disp,Rn)
    // Stores a word into memory using indirect register with displacement addressing
    pub(crate) fn mov_w_r0_rn_displacement(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xF) << 1;
        let n = rm(opcode);
        let address = self.registers.gpr[n].wrapping_add(disp.into());
        self.write_word(address, self.registers.gpr[0] as u16, bus);
    }

    // MOV.B @(disp,Rm), R0
    // Load a byte from memory using indirect register with displacement addressing
    pub(crate) fn mov_b_rm_displacement_r0(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = opcode & 0xF;
        let m = rm(opcode);

        let address = self.registers.gpr[m].wrapping_add(disp.into());
        let value = self.read_byte(address, bus);
        self.registers.gpr[0] = extend_i8(value);
    }

    // MOV.W @(disp,Rm), R0
    // Load a word from memory using indirect register with displacement addressing
    pub(crate) fn mov_w_rm_displacement_r0(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xF) << 1;
        let m = rm(opcode);

        let address = self.registers.gpr[m].wrapping_add(disp.into());
        let value = self.read_word(address, bus);
        self.registers.gpr[0] = extend_i16(value);
    }

    // MOV.L Rm, @(disp,Rn)
    // Stores a longword into memory using indirect register with displacement addressing
    pub(crate) fn mov_l_rm_rn_displacement(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let disp = (opcode & 0xF) << 2;
        let address = self.registers.gpr[n].wrapping_add(disp.into());
        self.write_longword(address, self.registers.gpr[m], bus);
    }

    // MOV.L @(disp,Rm), Rn
    // Loads a longword from memory using indirect register with displacement addressing
    pub(crate) fn mov_l_rm_displacement_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let disp = (opcode & 0xF) << 2;
        let address = self.registers.gpr[m].wrapping_add(disp.into());
        self.registers.gpr[n] = self.read_longword(address, bus);
    }

    // MOV.B Rm, @(R0,Rn)
    // Stores a byte into memory using indirect indexed register addressing
    pub(crate) fn mov_b_rm_indirect_indexed(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[n]);
        self.write_byte(address, self.registers.gpr[m] as u8, bus);
    }

    // MOV.W Rm, @(R0,Rn)
    // Stores a word into memory using indirect indexed register addressing
    pub(crate) fn mov_w_rm_indirect_indexed(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[n]);
        self.write_word(address, self.registers.gpr[m] as u16, bus);
    }

    // MOV.L Rm, @(R0,Rn)
    // Stores a longword into memory using indirect indexed register addressing
    pub(crate) fn mov_l_rm_indirect_indexed(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[n]);
        self.write_longword(address, self.registers.gpr[m], bus);
    }

    // MOV.B @(R0,Rm), Rn
    // Loads a byte from memory using indirect indexed register addressing
    pub(crate) fn mov_b_indirect_indexed_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[m]);
        let value = self.read_byte(address, bus);
        self.registers.gpr[n] = extend_i8(value);
    }

    // MOV.W @(R0,Rm), Rn
    // Loads a word from memory using indirect indexed register addressing
    pub(crate) fn mov_w_indirect_indexed_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[m]);
        let value = self.read_word(address, bus);
        self.registers.gpr[n] = extend_i16(value);
    }

    // MOV.L @(R0,Rm), Rn
    // Loads a longword from memory using indirect indexed register addressing
    pub(crate) fn mov_l_indirect_indexed_rn(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rm(opcode);
        let n = rn(opcode);

        let address = self.registers.gpr[0].wrapping_add(self.registers.gpr[m]);
        self.registers.gpr[n] = self.read_longword(address, bus);
    }

    // MOV.B @(disp,GBR), R0
    // Loads a byte into R0 using indirect GBR with displacement addressing
    pub(crate) fn mov_b_disp_gbr_r0(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = opcode & 0xFF;
        let address = self.registers.gbr.wrapping_add(disp.into());
        let value = self.read_byte(address, bus);
        self.registers.gpr[0] = extend_i8(value);
    }

    // MOV.W @(disp,GBR), R0
    // Loads a word into R0 using indirect GBR with displacement addressing
    pub(crate) fn mov_w_disp_gbr_r0(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 1;
        let address = self.registers.gbr.wrapping_add(disp.into());
        let value = self.read_word(address, bus);
        self.registers.gpr[0] = extend_i16(value);
    }

    // MOV.L @(disp,GBR), R0
    // Loads a longword into R0 using indirect GBR with displacement addressing
    pub(crate) fn mov_l_disp_gbr_r0(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 2;
        let address = self.registers.gbr.wrapping_add(disp.into());
        self.registers.gpr[0] = self.read_longword(address, bus);
    }

    // MOV.B R0, @(disp,GBR)
    // Stores a byte into memory using indirect GBR with displacement addressing
    pub(crate) fn mov_b_r0_disp_gbr(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = opcode & 0xFF;
        let address = self.registers.gbr.wrapping_add(disp.into());
        self.write_byte(address, self.registers.gpr[0] as u8, bus);
    }

    // MOV.W R0, @(disp,GBR)
    // Stores a word into memory using indirect GBR with displacement addressing
    pub(crate) fn mov_w_r0_disp_gbr(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 1;
        let address = self.registers.gbr.wrapping_add(disp.into());
        self.write_word(address, self.registers.gpr[0] as u16, bus);
    }

    // MOV.L R0, @(disp,GBR)
    // Stores a longword into memory using indirect GBR with displacement addressing
    pub(crate) fn mov_l_r0_disp_gbr(&mut self, opcode: u16, bus: &mut Bus) {
        let disp = (opcode & 0xFF) << 2;
        let address = self.registers.gbr.wrapping_add(disp.into());
        self.write_longword(address, self.registers.gpr[0], bus);
    }

    // MOVA @(disp,PC), R0
    // Move effective address
    pub(crate) fn mova(&mut self, opcode: u16) {
        let disp = (opcode & 0xFF) << 2;
        self.registers.gpr[0] = (self.registers.next_pc & !3).wrapping_add(disp.into());
    }

    // MOVT Rn
    // Loads T into a general-purpose register
    pub(crate) fn movt(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.sr.t.into();
    }

    // LDC Rm, SR
    // Loads the status register from a general-purpose register
    pub(crate) fn ldc_rm_sr(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.sr = self.registers.gpr[n].into();
    }

    // LDC Rm, GBR
    // Loads GBR from a general-purpose register
    pub(crate) fn ldc_rm_gbr(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gbr = self.registers.gpr[n];
    }

    // LDC Rm, VBR
    // Loads VBR from a general-purpose register
    pub(crate) fn ldc_rm_vbr(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.vbr = self.registers.gpr[n];
    }

    // LDC.L @Rm+, SR
    // Load SR from memory using post-increment register indirect addressing
    pub(crate) fn ldc_postinc_sr(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rn(opcode);
        let address = self.registers.gpr[m];
        self.registers.gpr[m] = address.wrapping_add(4);

        self.registers.sr = self.read_longword(address, bus).into();

        bus.increment_cycle_counter(2);
    }

    // LDC.L @Rm+, GBR
    // Load GBR from memory using post-increment register indirect addressing
    pub(crate) fn ldc_postinc_gbr(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rn(opcode);
        let address = self.registers.gpr[m];
        self.registers.gpr[m] = address.wrapping_add(4);

        self.registers.gbr = self.read_longword(address, bus);

        bus.increment_cycle_counter(2);
    }

    // LDC.L @Rm+, VBR
    // Load VBR from memory using post-increment register indirect addressing
    pub(crate) fn ldc_postinc_vbr(&mut self, opcode: u16, bus: &mut Bus) {
        let m = rn(opcode);
        let address = self.registers.gpr[m];
        self.registers.gpr[m] = address.wrapping_add(4);

        self.registers.vbr = self.read_longword(address, bus);

        bus.increment_cycle_counter(2);
    }

    // LDS Rm, MACH
    // Loads MACH from a general-purpose register
    pub(crate) fn lds_rm_mach(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.mach = self.registers.gpr[n];
    }

    // LDS Rm, MACL
    // Loads MACL from a general-purpose register
    pub(crate) fn lds_rm_macl(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.macl = self.registers.gpr[n];
    }

    // LDS Rm, PR
    // Loads PR from a general-purpose register
    pub(crate) fn lds_rm_pr(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.pr = self.registers.gpr[n];
    }

    // LDS.L @Rm+, MACH
    // Loads MACH from memory using post-increment indirect register addressing
    pub(crate) fn lds_postinc_mach(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n];
        self.registers.gpr[n] = address.wrapping_add(4);

        self.registers.mach = self.read_longword(address, bus);
    }

    // LDS.L @Rm+, MACL
    // Loads MACL from memory using post-increment indirect register addressing
    pub(crate) fn lds_postinc_macl(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n];
        self.registers.gpr[n] = address.wrapping_add(4);

        self.registers.macl = self.read_longword(address, bus);
    }

    // LDS.L @Rm+, PR
    // Loads PR from memory using post-increment indirect register addressing
    pub(crate) fn lds_postinc_pr(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n];
        self.registers.gpr[n] = address.wrapping_add(4);

        self.registers.pr = self.read_longword(address, bus);
    }

    // STC SR, Rn
    // Store SR into a general-purpose register
    pub(crate) fn stc_sr_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.sr.into();
    }

    // STC GBR, Rn
    // Store GBR into a general-purpose register
    pub(crate) fn stc_gbr_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.gbr;
    }

    // STC VBR, Rn
    // Store VBR into a general-purpose register
    pub(crate) fn stc_vbr_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.vbr;
    }

    // STC.L SR, @-Rn
    // Store SR in memory using pre-decrement register addressing
    pub(crate) fn stc_sr_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.sr.into(), bus);

        bus.increment_cycle_counter(1);
    }

    // STC.L GBR, @-Rn
    // Store GBR in memory using pre-decrement register addressing
    pub(crate) fn stc_gbr_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.gbr, bus);

        bus.increment_cycle_counter(1);
    }

    // STC.L VBR, @-Rn
    // Store VBR in memory using pre-decrement register addressing
    pub(crate) fn stc_vbr_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.vbr, bus);

        bus.increment_cycle_counter(1);
    }

    // STS MACH, Rn
    // Store MACH into a general-purpose register
    pub(crate) fn sts_mach_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.mach;
    }

    // STS MACL, Rn
    // Store MACL into a general-purpose register
    pub(crate) fn sts_macl_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.macl;
    }

    // STS PR, Rn
    // Store PR into a general-purpose register
    pub(crate) fn sts_pr_rn(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] = self.registers.pr;
    }

    // STS.L MACH, @-Rn
    // Store MACH in memory using pre-decrement register addressing
    pub(crate) fn sts_mach_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.mach, bus);
    }

    // STS.L MACL, @-Rn
    // Store MACL in memory using pre-decrement register addressing
    pub(crate) fn sts_macl_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.macl, bus);
    }

    // STS.L PR, @-Rn
    // Store PR in memory using pre-decrement indirect register addressing
    pub(crate) fn sts_pr_rn_predec(&mut self, opcode: u16, bus: &mut Bus) {
        let n = rn(opcode);
        let address = self.registers.gpr[n].wrapping_sub(4);
        self.registers.gpr[n] = address;

        self.write_longword(address, self.registers.pr, bus);
    }

    // CLRT
    // Clear the T flag
    pub(crate) fn clrt(&mut self) {
        self.registers.sr.t = false;
    }

    // SETT
    // Set the T flag
    pub(crate) fn sett(&mut self) {
        self.registers.sr.t = true;
    }

    // CLRMAC
    // Clear the MAC registers
    pub(crate) fn clrmac(&mut self) {
        self.registers.macl = 0;
        self.registers.mach = 0;
    }

    // SWAP.B Rm, Rn
    // Swaps the lowest two bytes of a register
    pub(crate) fn swap_b(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m];
        self.registers.gpr[n] =
            (value & 0xFFFF0000) | ((value & 0xFF) << 8) | ((value >> 8) & 0xFF);
    }

    // SWAP.W Rm, Rn
    // Swaps the words of a register
    pub(crate) fn swap_w(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        let value = self.registers.gpr[m];
        self.registers.gpr[n] = value.rotate_left(16);
    }

    // XTRCT Rm, Rn
    // Extract the center 32 bits of the 64-bit value formed by Rm and Rn
    pub(crate) fn xtrct(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);

        self.registers.gpr[n] = (self.registers.gpr[m] << 16) | (self.registers.gpr[n] >> 16);
    }
}
