#[allow(clippy::wildcard_imports)]
use super::*;

macro_rules! impl_psw_flag_op {
    ($name:ident, $($field:ident = $value:expr),* $(,)?) => {
        impl_registers_op!($name, |registers| {
            $(
                registers.psw.$field = $value;
            )*
        });
    }
}

impl_psw_flag_op!(clrc, carry = false);
impl_psw_flag_op!(setc, carry = true);
impl_psw_flag_op!(clrv, overflow = false, half_carry = false);
impl_psw_flag_op!(clrp, direct_page = false);
impl_psw_flag_op!(setp, direct_page = true);

impl_long_registers_op!(notc, |registers| {
    registers.psw.carry = !registers.psw.carry;
});

impl_long_registers_op!(ei, |registers| {
    registers.psw.interrupt_enabled = true;
});

impl_long_registers_op!(di, |registers| {
    registers.psw.interrupt_enabled = false;
});
