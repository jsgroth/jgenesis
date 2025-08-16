use crate::registers::SystemRegisters;
use std::array;

impl SystemRegisters {
    pub fn dump(
        &self,
        h_interrupt_in_vblank: bool,
        h_interrupt_interval: u16,
        mut callback: impl FnMut(&str, &[(&str, &str)]),
    ) {
        callback(
            "$4000 / $A15100",
            &[
                ("Adapter enabled", bool_str(self.adapter_enabled)),
                ("SH-2 reset", bool_str(self.reset_sh2)),
                ("M PWM interrupt enabled", bool_str(self.master_interrupts.pwm_enabled)),
                ("S PWM interrupt enabled", bool_str(self.slave_interrupts.pwm_enabled)),
                ("M command interrupt enabled", bool_str(self.master_interrupts.command_enabled)),
                ("S command interrupt enabled", bool_str(self.slave_interrupts.command_enabled)),
                ("M horizontal interrupt enabled", bool_str(self.master_interrupts.h_enabled)),
                ("S horizontal interrupt enabled", bool_str(self.slave_interrupts.h_enabled)),
                ("M vertical interrupt enabled", bool_str(self.master_interrupts.v_enabled)),
                ("S vertical interrupt enabled", bool_str(self.slave_interrupts.v_enabled)),
                ("H interrupts during VBlank", bool_str(h_interrupt_in_vblank)),
                ("FM (VDP access)", &self.vdp_access.to_string()),
            ],
        );

        callback(
            "$4002 / $A15102",
            &[
                ("M command interrupt pending", bool_str(self.master_interrupts.command_pending)),
                ("S command interrupt pending", bool_str(self.slave_interrupts.command_pending)),
            ],
        );

        callback("$4004", &[("H interrupt interval", &h_interrupt_interval.to_string())]);

        callback("$A15104", &[("68000 cartridge ROM bank", &rom_bank_str(self.m68k_rom_bank))]);

        callback(
            "$4006 / $A15106",
            &[
                ("RV (ROM-to-VRAM DMA allowed)", bool_str(self.dma.rom_to_vram_dma)),
                ("DREQ active", bool_str(self.dma.active)),
            ],
        );

        callback(
            "$A15108-$A1510A",
            &[("DREQ source address", &format!("${:06X}", self.dma.source_address))],
        );

        callback(
            "$A1510C-$A1510E",
            &[("DREQ destination address", &format!("${:06X}", self.dma.destination_address))],
        );

        callback("$A15110", &[("DREQ length", &format!("0x{:04X}", self.dma.length))]);

        let comm_port_strings: [_; 8] = array::from_fn(|i| {
            (format!("Communication port {i}"), format!("0x{:04X}", self.communication_ports[i]))
        });
        let comm_port_strs: [_; 8] =
            array::from_fn(|i| (comm_port_strings[i].0.as_str(), comm_port_strings[i].1.as_str()));

        callback("Communication ports", &comm_port_strs);
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn rom_bank_str(bank: u8) -> String {
    format!("{bank} (${bank:X}00000-${bank:X}FFFFF)")
}
