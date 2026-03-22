//! CKB-VM execution module for DOB decoder binaries.
//!
//! Decoder binaries are pure computational RISC-V programs that receive
//! `argv = [dna_hex, pattern_json]` and produce output via a single debug
//! syscall (number 2177). No CKB syscall mocking is needed.

use ckb_vm::cost_model::constant_cycles;
use ckb_vm::machine::asm::{AsmCoreMachine, AsmMachine};
use ckb_vm::machine::VERSION2;
use ckb_vm::registers::{A0, A7};
use ckb_vm::{
    Bytes, DefaultMachineBuilder, DefaultMachineRunner, Memory, Register, SupportMachine, Syscalls,
    ISA_A, ISA_B, ISA_IMC, ISA_MOP,
};

use std::sync::{Arc, Mutex};

/// Debug syscall number used by DOB decoders to emit output.
const DEBUG_SYSCALL_NUMBER: i32 = 2177;

/// Syscall handler that captures output from the decoder binary.
///
/// When the guest program issues ecall with A7 = 2177, this handler reads a
/// null-terminated C string from VM memory starting at the address in A0 and
/// appends it to the shared output buffer.
struct DebugSyscall {
    output: Arc<Mutex<Vec<String>>>,
}

impl<Mac: SupportMachine> Syscalls<Mac> for DebugSyscall {
    fn initialize(&mut self, _machine: &mut Mac) -> Result<(), ckb_vm::Error> {
        Ok(())
    }

    fn ecall(&mut self, machine: &mut Mac) -> Result<bool, ckb_vm::Error> {
        let code = &machine.registers()[A7];
        if code.to_i32() != DEBUG_SYSCALL_NUMBER {
            return Ok(false);
        }

        let mut addr = machine.registers()[A0].to_u64();
        let mut buffer = Vec::new();

        loop {
            let byte = machine
                .memory_mut()
                .load8(&Mac::REG::from_u64(addr))?
                .to_u8();
            if byte == 0 {
                break;
            }
            buffer.push(byte);
            addr += 1;
        }

        let text = String::from_utf8(buffer)
            .map_err(|_| ckb_vm::Error::Unexpected("non-UTF-8 debug output".to_string()))?;

        self.output.lock().unwrap().push(text);

        Ok(true)
    }
}

/// Execute a RISC-V binary in CKB-VM with the given arguments.
///
/// Returns `(exit_code, debug_output_lines)` where `debug_output_lines`
/// contains all strings emitted via syscall 2177.
pub fn execute_riscv_binary(binary: &[u8], args: &[&str]) -> anyhow::Result<(i8, Vec<String>)> {
    let code: Bytes = binary.to_vec().into();
    let args_bytes: Vec<Bytes> = args
        .iter()
        .map(|a| Bytes::from(a.as_bytes().to_vec()))
        .collect();

    let debug_output = Arc::new(Mutex::new(Vec::new()));
    let syscall_handler = Box::new(DebugSyscall {
        output: Arc::clone(&debug_output),
    });

    let asm_core = <Box<AsmCoreMachine> as SupportMachine>::new(
        ISA_IMC | ISA_B | ISA_MOP | ISA_A,
        VERSION2,
        u64::MAX,
    );
    let default_machine = DefaultMachineBuilder::new(asm_core)
        .instruction_cycle_func(Box::new(constant_cycles))
        .syscall(syscall_handler)
        .build();
    let mut machine = AsmMachine::new(default_machine);

    machine
        .load_program(&code, args_bytes.iter().map(|a| Ok(a.clone())))
        .map_err(|e| anyhow::anyhow!("failed to load program: {e}"))?;

    let exit_code = machine
        .run()
        .map_err(|e| anyhow::anyhow!("VM execution failed: {e}"))?;

    let output = debug_output.lock().unwrap().clone();
    Ok((exit_code, output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_binary_returns_error() {
        let result = execute_riscv_binary(&[], &["arg1"]);
        assert!(result.is_err(), "empty binary should fail as invalid ELF");
    }
}
