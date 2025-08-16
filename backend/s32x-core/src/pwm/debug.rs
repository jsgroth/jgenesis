use crate::pwm::PwmChip;

impl PwmChip {
    pub fn dump_registers(&self, mut callback: impl FnMut(&str, &[(&str, &str)])) {
        callback(
            "$4030 / $A15130",
            &[
                ("L channel output", &self.control.l_out.to_string()),
                ("R channel output", &self.control.r_out.to_string()),
                ("DREQ 1 enabled", bool_str(self.control.dreq1_enabled)),
                ("Timer interval", &self.control.effective_timer_interval().to_string()),
            ],
        );

        let sample_rate = 53_693_175.0 * 3.0
            / 7.0
            / f64::from(self.cycle_register.wrapping_sub(1) & ((1 << 12) - 1));
        callback(
            "$4032 / $A15132",
            &[
                ("Cycle register", &self.cycle_register.to_string()),
                ("Sample rate", &format!("{:.0} Hz", sample_rate.round())),
            ],
        );
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}
