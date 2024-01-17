use crate::sm83::bus::BusInterface;
use crate::sm83::{BusExt, Sm83};

impl Sm83 {
    // LD r, r': Load value into r from r'
    pub(super) fn ld_r_r<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.read_register(bus, opcode);
        self.write_register(bus, opcode >> 3, value);
    }

    // LD r, u8: Load 8-bit immediate value into r
    pub(super) fn ld_r_imm<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let value = self.fetch_operand(bus);
        self.write_register(bus, opcode >> 3, value);
    }

    // LD (BC), A: Load value into address specified by BC from A
    pub(super) fn ld_bc_a<B: BusInterface>(&mut self, bus: &mut B) {
        bus.write(self.registers.bc(), self.registers.a);
    }

    // LD (DE), A: Load value into address specified by DE from A
    pub(super) fn ld_de_a<B: BusInterface>(&mut self, bus: &mut B) {
        bus.write(self.registers.de(), self.registers.a);
    }

    // LD (HL+), A: Load value into address specified by HL from A, then increment HL
    pub(super) fn ld_hl_a_postinc<B: BusInterface>(&mut self, bus: &mut B) {
        bus.write(self.registers.hl(), self.registers.a);
        self.registers.increment_hl();
    }

    // LD (HL-), A: Load value into address specified by HL from A, then decrement HL
    pub(super) fn ld_hl_a_postdec<B: BusInterface>(&mut self, bus: &mut B) {
        bus.write(self.registers.hl(), self.registers.a);
        self.registers.decrement_hl();
    }

    // LD A, (BC): Load value into A from address specified by BC
    pub(super) fn ld_a_bc<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.a = bus.read(self.registers.bc());
    }

    // LD A, (DE): Load value into A from address specified by DE
    pub(super) fn ld_a_de<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.a = bus.read(self.registers.de());
    }

    // LD A, (HL+): Load value into A from address specified by HL, then increment HL
    pub(super) fn ld_a_hl_postinc<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.a = bus.read(self.registers.hl());
        self.registers.increment_hl();
    }

    // LD A, (HL-): Load value into A from address specified by HL, then decrement HL
    pub(super) fn ld_a_hl_postdec<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.a = bus.read(self.registers.hl());
        self.registers.decrement_hl();
    }

    // LD (u16), A: Load value into address specified by 16-bit immediate operand from A
    pub(super) fn ld_indirect_a<B: BusInterface>(&mut self, bus: &mut B) {
        let address = self.fetch_operand_u16(bus);
        bus.write(address, self.registers.a);
    }

    // LD A, (u16): Load value into A from address specified by 16-bit immediate operand
    pub(super) fn ld_a_indirect<B: BusInterface>(&mut self, bus: &mut B) {
        let address = self.fetch_operand_u16(bus);
        self.registers.a = bus.read(address);
    }

    // LDH (u8), A: Load value into address $FFxx from A, where xx is the 8-bit immediate operand
    pub(super) fn ldh_imm_a<B: BusInterface>(&mut self, bus: &mut B) {
        let address_lsb = self.fetch_operand(bus);
        let address = u16::from_le_bytes([address_lsb, 0xFF]);
        bus.write(address, self.registers.a);
    }

    // LDH A, (u8): Load value into A from address $FFxx, where xx is the 8-bit immediate operand
    pub(super) fn ldh_a_imm<B: BusInterface>(&mut self, bus: &mut B) {
        let address_lsb = self.fetch_operand(bus);
        let address = u16::from_le_bytes([address_lsb, 0xFF]);
        self.registers.a = bus.read(address);
    }

    // LD ($FF00+C), A: Load value into address $FFxx from A, where xx is the value in C
    pub(super) fn ld_c_a_high_page<B: BusInterface>(&mut self, bus: &mut B) {
        let address = u16::from_le_bytes([self.registers.c, 0xFF]);
        bus.write(address, self.registers.a);
    }

    // LD A, ($FF00+C): Load value into A from address $FFxx, where xx is the value in C
    pub(super) fn ld_a_c_high_page<B: BusInterface>(&mut self, bus: &mut B) {
        let address = u16::from_le_bytes([self.registers.c, 0xFF]);
        self.registers.a = bus.read(address);
    }

    // LD rr, nn: Load 16-bit immediate value into register pair (or SP)
    pub(super) fn ld_rr_nn<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let lsb = self.fetch_operand(bus);
        let msb = self.fetch_operand(bus);

        match (opcode >> 4) & 0x3 {
            0x0 => {
                self.registers.b = msb;
                self.registers.c = lsb;
            }
            0x1 => {
                self.registers.d = msb;
                self.registers.e = lsb;
            }
            0x2 => {
                self.registers.h = msb;
                self.registers.l = lsb;
            }
            0x3 => {
                self.registers.sp = u16::from_le_bytes([lsb, msb]);
            }
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }
    }

    // LD (u16), SP: Write stack pointer to address specified by 16-bit immediate value
    pub(super) fn ld_indirect_sp<B: BusInterface>(&mut self, bus: &mut B) {
        let address = self.fetch_operand_u16(bus);
        bus.write_u16(address, self.registers.sp);
    }

    // LD SP, HL: Load into SP from HL
    pub(super) fn ld_sp_hl<B: BusInterface>(&mut self, bus: &mut B) {
        self.registers.sp = self.registers.hl();
        bus.idle();
    }

    // PUSH: Push onto stack
    pub(super) fn push_rr<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        bus.idle();

        let value = match (opcode >> 4) & 0x3 {
            0x0 => self.registers.bc(),
            0x1 => self.registers.de(),
            0x2 => self.registers.hl(),
            0x3 => self.registers.af(),
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        };
        self.push_stack_u16(bus, value);
    }

    // POP: Pop off of stack
    pub(super) fn pop_rr<B: BusInterface>(&mut self, bus: &mut B, opcode: u8) {
        let lsb = self.pop_stack(bus);
        let msb = self.pop_stack(bus);

        match (opcode >> 4) & 0x3 {
            0x0 => {
                self.registers.b = msb;
                self.registers.c = lsb;
            }
            0x1 => {
                self.registers.d = msb;
                self.registers.e = lsb;
            }
            0x2 => {
                self.registers.h = msb;
                self.registers.l = lsb;
            }
            0x3 => {
                self.registers.a = msb;
                self.registers.f = lsb.into();
            }
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }
    }
}
