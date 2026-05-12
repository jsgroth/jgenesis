use crate::input::InputState;
use crate::memory::{HuCard, Memory};
use crate::psg::Huc6280Psg;
use crate::video::VideoSubsystem;
use huc6280_emu::bus::{BusInterface, ClockSpeed, InterruptLines};

pub struct Bus<'a> {
    pub memory: &'a mut Memory,
    pub video: &'a mut VideoSubsystem,
    pub psg: &'a mut Huc6280Psg,
    pub cartridge: &'a HuCard,
    pub input: &'a mut InputState,
    pub cycle_counter: &'a mut u64,
}

impl Bus<'_> {
    fn cpu_cycle(&mut self) {
        *self.cycle_counter += self.memory.cpu_clock_divider();

        // TODO it's really not necessary to sync everything at every CPU cycle
        self.video.step_to(*self.cycle_counter, self.memory.cpu_registers().irq1_pending_mut());
        self.psg.step_to(*self.cycle_counter);
        self.memory.cpu_registers().step_to(*self.cycle_counter);
    }

    #[allow(clippy::match_same_arms)]
    fn read_io(&mut self, address: u32) -> u8 {
        if address < 0x1FE800 {
            // Wait cycle on VDC/VCE access
            self.cpu_cycle();
        }

        match address {
            0x1FE000..=0x1FE3FF => self.video.read_vdc(
                address,
                self.cycle_counter,
                self.memory.cpu_registers().irq1_pending_mut(),
            ),
            0x1FE400..=0x1FE7FF => self.video.read_vce(address),
            0x1FE800..=0x1FEBFF => self.memory.cpu_registers().io_buffer(), // PSG, write-only
            0x1FEC00..=0x1FEFFF => self.memory.cpu_registers().read_timer_register(),
            0x1FF000..=0x1FF3FF => {
                let value = self.input.read_port();
                self.memory.cpu_registers().update_io_buffer(value, !0)
            }
            0x1FF400..=0x1FF7FF => self.memory.cpu_registers().read_interrupt_register(address),
            0x1FF800..=0x1FFBFF => 0xFF, // CD-ROM
            0x1FFC00..=0x1FFFFF => 0xFF, // Unused
            _ => todo!("read IO {address:06X}"),
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_io(&mut self, address: u32, value: u8) {
        if address < 0x1FE800 {
            // Wait cycle on VDC/VCE access
            self.cpu_cycle();
        }

        match address {
            0x1FE000..=0x1FE3FF => self.video.write_vdc(
                address,
                value,
                self.cycle_counter,
                self.memory.cpu_registers().irq1_pending_mut(),
            ),
            0x1FE400..=0x1FE7FF => self.video.write_vce(address, value),
            0x1FE800..=0x1FEBFF => {
                self.psg.write(address, value);
                self.memory.cpu_registers().update_io_buffer(value, !0);
            }
            0x1FEC00..=0x1FEFFF => {
                self.memory.cpu_registers().write_timer_register(address, value);
            }
            0x1FF000..=0x1FF3FF => {
                self.input.write_port(value);
                self.memory.cpu_registers().update_io_buffer(value, !0);
            }
            0x1FF400..=0x1FF7FF => {
                self.memory.cpu_registers().write_interrupt_register(address, value);
            }
            0x1FF800..=0x1FFBFF => {} // CD-ROM
            0x1FFC00..=0x1FFFFF => {} // Unused
            _ => todo!("write IO {address:06X} {value:02X}"),
        }
    }
}

impl BusInterface for Bus<'_> {
    #[inline]
    #[allow(clippy::match_same_arms)]
    fn read(&mut self, address: u32) -> u8 {
        debug_assert!(address < 0x200000);

        self.cpu_cycle();

        match address {
            0x000000..=0x0FFFFF => self.cartridge.read_rom(address),
            0x100000..=0x1EFFFF => 0xFF, // CD-ROM
            0x1F0000..=0x1F7FFF => self.memory.read_working_ram(address),
            0x1F8000..=0x1FDFFF => 0xFF, // Unused memory
            0x1FE000..=0x1FFFFF => self.read_io(address),
            0x200000..=0xFFFFFFFF => panic!("invalid HuC6280 address {address:06X}"),
        }
    }

    #[inline]
    #[allow(clippy::match_same_arms)]
    fn write(&mut self, address: u32, value: u8) {
        debug_assert!(address < 0x200000);

        self.cpu_cycle();

        match address {
            0x000000..=0x0FFFFF => {} // Cartridge
            0x100000..=0x1EFFFF => {} // CD-ROM
            0x1F0000..=0x1F7FFF => self.memory.write_working_ram(address, value),
            0x1F8000..=0x1FDFFF => {} // Unused memory
            0x1FE000..=0x1FFFFF => self.write_io(address, value),
            0x200000..=0xFFFFFFFF => panic!("invalid HuC6280 address {address:06X}"),
        }
    }

    #[inline]
    fn idle(&mut self) {
        self.cpu_cycle();
    }

    #[inline]
    fn interrupt_lines(&self) -> InterruptLines {
        self.memory.interrupt_lines()
    }

    #[inline]
    fn set_clock_speed(&mut self, speed: ClockSpeed) {
        self.memory.set_clock_speed(speed);
    }
}
