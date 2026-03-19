//! SH-2 logical and bitwise instructions

use crate::Sh2;
use crate::bus::BusInterface;
use crate::instructions::{rm, rn};
use jgenesis_common::num::GetBit;

impl Sh2 {
    // AND Rm, Rn
    // Logical and
    pub(crate) fn and_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] &= self.registers.gpr[m];
    }

    // AND #imm, R0
    // Logical and
    pub(crate) fn and_imm_r0(&mut self, opcode: u16) {
        let imm: u32 = (opcode & 0xFF).into();
        self.registers.gpr[0] &= imm;
    }

    // AND.B #imm @(R0,GBR)
    // Logical and, in memory
    pub(crate) fn and_imm_gbr_indexed(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        bus.increment_cycle_counter(2);

        let imm = opcode as u8;
        let address = self.registers.gbr.wrapping_add(self.registers.gpr[0]);
        let value = self.read_byte(address, bus);
        self.write_byte(address, value & imm, bus);
    }

    // OR Rm, Rn
    // Logical or
    pub(crate) fn or_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] |= self.registers.gpr[m];
    }

    // OR #imm, R0
    // Logical or
    pub(crate) fn or_imm_r0(&mut self, opcode: u16) {
        let imm: u32 = (opcode & 0xFF).into();
        self.registers.gpr[0] |= imm;
    }

    // OR.B #imm @(R0,GBR)
    // Logical or, in memory
    pub(crate) fn or_imm_gbr_indexed(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        bus.increment_cycle_counter(2);

        let imm = opcode as u8;
        let address = self.registers.gbr.wrapping_add(self.registers.gpr[0]);
        let value = self.read_byte(address, bus);
        self.write_byte(address, value | imm, bus);
    }

    // XOR Rm, Rn
    // Exclusive or
    pub(crate) fn xor_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] ^= self.registers.gpr[m];
    }

    // XOR #imm, R0
    // Exclusive or
    pub(crate) fn xor_imm_r0(&mut self, opcode: u16) {
        let imm: u32 = (opcode & 0xFF).into();
        self.registers.gpr[0] ^= imm;
    }

    // XOR.B #imm @(R0,GBR)
    // Exclusive or, in memory
    pub(crate) fn xor_imm_gbr_indexed(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        bus.increment_cycle_counter(2);

        let imm = opcode as u8;
        let address = self.registers.gbr.wrapping_add(self.registers.gpr[0]);
        let value = self.read_byte(address, bus);
        self.write_byte(address, value ^ imm, bus);
    }

    // NOT Rm, Rn
    // Logical complement
    pub(crate) fn not(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.gpr[n] = !self.registers.gpr[m];
    }

    // SHLL Rn
    // Logical shift left
    pub(crate) fn shll(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.sr.t = self.registers.gpr[n].bit(31);
        self.registers.gpr[n] <<= 1;
    }

    // SHLLn Rn
    // Logical shift left by N bits
    pub(crate) fn shlln<const N: usize>(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] <<= N;
    }

    // SHAR Rn
    // Arithmetic shift right
    pub(crate) fn shar(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.sr.t = self.registers.gpr[n].bit(0);
        self.registers.gpr[n] = (self.registers.gpr[n] >> 1) | (self.registers.gpr[n] & (1 << 31));
    }

    // SHLR Rn
    // Logical shift right
    pub(crate) fn shlr(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.sr.t = self.registers.gpr[n].bit(0);
        self.registers.gpr[n] >>= 1;
    }

    // SHLRn Rn
    // Logical shift right by N bits
    pub(crate) fn shlrn<const N: usize>(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.gpr[n] >>= N;
    }

    // ROTL Rn
    // Rotate left
    pub(crate) fn rotl(&mut self, opcode: u16) {
        let n = rn(opcode);
        self.registers.sr.t = self.registers.gpr[n].bit(31);
        self.registers.gpr[n] = self.registers.gpr[n].rotate_left(1);
    }

    // ROTCL Rn
    // Rotate with carry left
    pub(crate) fn rotcl(&mut self, opcode: u16) {
        let n = rn(opcode);
        let carry_out = self.registers.gpr[n].bit(31);
        self.registers.gpr[n] = (self.registers.gpr[n] << 1) | u32::from(self.registers.sr.t);
        self.registers.sr.t = carry_out;
    }

    // ROTR Rn
    // Rotate right
    pub(crate) fn rotr(&mut self, opcode: u16) {
        let n = rn(opcode);
        let carry = self.registers.gpr[n].bit(0);
        self.registers.sr.t = carry;
        self.registers.gpr[n] = self.registers.gpr[n].rotate_right(1);
    }

    // ROTCR Rn
    // Rotate with carry right
    pub(crate) fn rotcr(&mut self, opcode: u16) {
        let n = rn(opcode);
        let carry_out = self.registers.gpr[n].bit(0);
        self.registers.gpr[n] =
            (self.registers.gpr[n] >> 1) | (u32::from(self.registers.sr.t) << 31);
        self.registers.sr.t = carry_out;
    }

    // TST Rm, Rn
    // Sets the T bit if (Rm & Rn) is 0
    pub(crate) fn tst_rm_rn(&mut self, opcode: u16) {
        let m = rm(opcode);
        let n = rn(opcode);
        self.registers.sr.t = self.registers.gpr[m] & self.registers.gpr[n] == 0;
    }

    // TST #imm, R0
    // Sets the T bit if (#imm & R0) is 0
    pub(crate) fn tst_imm_r0(&mut self, opcode: u16) {
        let imm: u32 = (opcode & 0xFF).into();
        self.registers.sr.t = imm & self.registers.gpr[0] == 0;
    }

    // TST.B #imm, @(R0,GBR)
    // Sets the T bit if (#imm & MEM[GBR+R0]) is 0
    pub(crate) fn tst_imm_gbr_indexed(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        bus.increment_cycle_counter(2);

        let imm = opcode as u8;
        let address = self.registers.gbr.wrapping_add(self.registers.gpr[0]);
        let value = self.read_byte(address, bus);
        self.registers.sr.t = imm & value == 0;
    }

    // TAS.B @Rn
    // Tests the value at the specified address and sets bit 7
    pub(crate) fn tas(&mut self, opcode: u16, bus: &mut impl BusInterface) {
        bus.increment_cycle_counter(3);

        let n = rn(opcode);
        let address = self.registers.gpr[n];

        // TAS never performs cached reads; read from the bus directly
        let value = bus.read_byte(address & crate::EXTERNAL_ADDRESS_MASK, self.data_ctx);
        self.write_byte(address, value | 0x80, bus);

        self.registers.sr.t = value == 0;
    }
}
