use super::*;
use crate::api::GenesisHardware;
use bincode::{Decode, Encode};
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::frontend::SaveWriter;
use s32x_core::api::debug::Dummy32XDebugger;

struct NullOutput;

#[allow(unused_variables)]
impl SaveWriter for NullOutput {
    type Err = String;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        Err(String::new())
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        Ok(())
    }

    fn load_serialized<D: Decode<()>>(&mut self, extension: &str) -> Result<D, Self::Err> {
        Err(String::new())
    }

    fn persist_serialized<E: Encode>(&mut self, extension: &str, data: E) -> Result<(), Self::Err> {
        Ok(())
    }
}

// This test is meant to be run in miri:
//   cargo +nightly miri test -p genesis-core
//
// The 32X code and the Genesis+32X debugger code make moderate use of unsafe to avoid having to
// put a lifetime on the SH-2 bus structs, which would be excessively annoying to deal with due to
// how the SH-2 opcode lookup table is implemented.
// This test is not completely exhaustive but should hit the major code paths that use unsafe.
//
// Warning, this is very slow in miri (takes 2-3 minutes on my machine)
#[test]
fn check_for_memory_model_violations() {
    const CPUS: [WhichCpu; 2] = [WhichCpu::Master, WhichCpu::Slave];

    let mut emulator = GenesisEmulator::create(
        GenesisHardware::SegaCd32X,
        None,
        Some(vec![0xFF; 128 * 1024]),
        None,
        GenesisEmulatorConfig::default(),
        &mut NullOutput,
    )
    .expect("Failed to create emulator");

    let (state_sender, _state_receiver) = jgenesis_common::sync::new_shared_var();
    let (mut debugger, debugger_handle) = GenesisDebugger::new(state_sender);

    let sega_32x = emulator.bus.sega_32x.as_mut().unwrap();

    // No debugger
    for which in CPUS {
        sega_32x.simulate_bus_interactions::<false>(
            &mut Dummy32XDebugger,
            which,
            &[0x02000000, 0x00004020],
            &[(0x06000000, 0), (0x00004020, 0)],
        );
    }

    // With debugger, no breakpoints
    let mut debugger_with_cpus = debugger.with_cpus(&mut emulator.m68k, &mut emulator.z80);
    let mut s32x_debugger = debugger_with_cpus
        .for_32x(emulator.bus.sega_cd.as_mut(), genesis_components!(emulator.bus));
    for which in CPUS {
        sega_32x.simulate_bus_interactions::<true>(
            &mut *s32x_debugger,
            which,
            &[0x02000000, 0x00004020],
            &[(0x06000000, 0), (0x00004020, 0)],
        );
    }

    for which in CPUS {
        debugger_handle
            .command_sender
            .send(GenesisDebugCommand::UpdateSh2Breakpoints(
                which,
                Sh2Breakpoints {
                    memory: vec![
                        Sh2Breakpoint {
                            start_address: 0x02000000,
                            end_address: 0x03000000,
                            read: true,
                            write: false,
                            execute: false,
                        },
                        Sh2Breakpoint {
                            start_address: 0x06000000,
                            end_address: 0x07000000,
                            read: false,
                            write: true,
                            execute: false,
                        },
                    ],
                    interrupt: vec![],
                },
            ))
            .unwrap();
    }

    debugger.process_commands(&mut emulator.as_debug_view());

    let sega_32x = emulator.bus.sega_32x.as_mut().unwrap();
    let mut debugger_with_cpus = debugger.with_cpus(&mut emulator.m68k, &mut emulator.z80);
    let mut s32x_debugger = debugger_with_cpus
        .for_32x(emulator.bus.sega_cd.as_mut(), genesis_components!(emulator.bus));

    // With debugger, with breakpoints, no memory edits
    for which in CPUS {
        // Expected to trigger 2 breakpoints
        for _ in 0..2 {
            debugger_handle.send_command(GenesisDebugCommand::BreakResume).unwrap();
        }

        sega_32x.simulate_bus_interactions::<true>(
            &mut *s32x_debugger,
            which,
            &[0x02000000, 0x00004020],
            &[(0x06000000, 0), (0x00004020, 0)],
        );
    }

    // With debugger, with breakpoints, with memory edits
    for which in CPUS {
        for memory_area in GenesisMemoryArea::ALL {
            debugger_handle
                .command_sender
                .send(GenesisDebugCommand::EditMemory(memory_area, 0, 0))
                .unwrap();
        }

        for memory_area in SegaCdMemoryArea::ALL {
            debugger_handle
                .command_sender
                .send(GenesisDebugCommand::EditSegaCdMemory(memory_area, 0, 0))
                .unwrap();
        }

        for memory_area in S32XMemoryArea::ALL {
            debugger_handle
                .command_sender
                .send(GenesisDebugCommand::Edit32XMemory(memory_area, 0, 0))
                .unwrap();
        }

        // Expected to trigger 2 breakpoints
        for _ in 0..2 {
            debugger_handle.send_command(GenesisDebugCommand::BreakResume).unwrap();
        }

        sega_32x.simulate_bus_interactions::<true>(
            &mut *s32x_debugger,
            which,
            &[0x02000000, 0x00004020],
            &[(0x06000000, 0), (0x00004020, 0)],
        );
    }
}
