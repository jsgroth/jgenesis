use crate::core::InstructionExecutor;
use crate::traits::BusInterface;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {}
