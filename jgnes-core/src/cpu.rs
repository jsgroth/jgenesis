mod instructions;

pub struct CpuRegisters {
    pub accumulator: u8,
    pub x: u8,
    pub y: u8,
    pub status: u8,
    pub pc: u16,
    pub sp: u8,
}

impl CpuRegisters {
    fn status_flags(&mut self) -> StatusFlags<'_> {
        StatusFlags(&mut self.status)
    }
}

pub struct StatusFlags<'a>(&'a mut u8);

impl<'a> StatusFlags<'a> {
    fn negative(&self) -> bool {
        *self.0 & 0x80 != 0
    }

    fn set_negative(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x80;
        } else {
            *self.0 &= !0x80;
        }
        self
    }

    fn overflow(&self) -> bool {
        *self.0 & 0x40 != 0
    }

    fn set_overflow(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x40;
        } else {
            *self.0 &= !0x40;
        }
        self
    }

    fn break_flag(&self) -> bool {
        *self.0 & 0x10 != 0
    }

    fn set_break(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x10;
        } else {
            *self.0 &= !0x10;
        }
        self
    }

    fn set_decimal(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x08;
        } else {
            *self.0 &= !0x08;
        }
        self
    }

    fn interrupt_disable(&self) -> bool {
        *self.0 & 0x04 != 0
    }

    fn set_interrupt_disable(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x04;
        } else {
            *self.0 &= !0x04;
        }
        self
    }

    fn zero(&self) -> bool {
        *self.0 & 0x02 != 0
    }

    fn set_zero(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x02;
        } else {
            *self.0 &= !0x02;
        }
        self
    }

    fn carry(&self) -> bool {
        *self.0 & 0x01 != 0
    }

    fn set_carry(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x01;
        } else {
            *self.0 &= !0x01;
        }
        self
    }
}

enum State {
    InstructionStart,
}

pub struct CpuState {
    registers: CpuRegisters,
    state: State,
}
