//! Call frames capture scalar/aggregate arguments and returns before outputs
//! can alias the guest stack. Completion never reloads the saved return.
use super::catalog::{Contract, Parameter, ReturnConvention};
use crate::{
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, READ},
};

#[derive(Clone)]
pub struct Call {
    pub arguments: [u64; 16],
    mode: Mode,
    returns: ReturnConvention,
    target: u64,
    sp: u64,
}

fn add(mode: Mode, address: u64, offset: u64, size: usize) -> Result<u64, Fault> {
    if mode == Mode::X86 {
        return Ok(mode.address(address.wrapping_add(offset)));
    }
    address.checked_add(offset).ok_or(Fault {
        address,
        length: size,
        access: READ,
        kind: FaultKind::Address,
    })
}

pub(crate) fn peek_slot(state: &State, memory: &Memory, slot: u64) -> Option<u64> {
    let mode = memory.mode();
    if mode == Mode::X64 && slot < 4 {
        return Some(state.regs[[1, 2, 8, 9][slot as usize]]);
    }
    let address = if mode == Mode::X86 {
        mode.address(state.regs[4].wrapping_add(4 + slot * 4))
    } else {
        state.regs[4].checked_add(40 + (slot - 4) * 8)?
    };
    let width = mode as usize / 8;
    let mut bytes = [0; 8];
    memory.observe(address, &mut bytes[..width]).ok()?;
    Some(u64::from_le_bytes(bytes))
}

impl Call {
    pub(crate) fn capture_stdcall32(
        state: &State,
        memory: &Memory,
        stack_bytes: u32,
    ) -> Result<Self, Fault> {
        let mut arguments = [0; 16];
        let sp = Mode::X86.address(state.regs[4]);
        for (n, value) in arguments[..stack_bytes as usize / 4].iter_mut().enumerate() {
            *value = memory.read_int(add(Mode::X86, sp, 4 + n as u64 * 4, 4)?, 4)?;
        }
        Ok(Self {
            arguments,
            mode: Mode::X86,
            returns: ReturnConvention::Uint32,
            target: memory.read_int(sp, 4)?,
            sp: add(Mode::X86, sp, u64::from(stack_bytes) + 4, 4)?,
        })
    }
    pub(crate) fn capture_cdecl32(
        state: &State,
        memory: &Memory,
        slots: u32,
    ) -> Result<Self, Fault> {
        let mut call = Self::capture_stdcall32(state, memory, slots * 4)?;
        call.sp = Mode::X86.address(state.regs[4].wrapping_add(4));
        Ok(call)
    }
    pub(crate) fn capture_nonreturn32(
        state: &State,
        memory: &Memory,
        slots: u32,
    ) -> Result<Self, Fault> {
        let mut arguments = [0; 16];
        let sp = Mode::X86.address(state.regs[4]);
        for (n, value) in arguments[..slots as usize].iter_mut().enumerate() {
            *value = memory.read_int(add(Mode::X86, sp, 4 + n as u64 * 4, 4)?, 4)?;
        }
        Ok(Self {
            arguments,
            mode: Mode::X86,
            returns: ReturnConvention::Uint32,
            target: 0,
            sp,
        })
    }
    pub(crate) fn capture_graphics(
        state: &State,
        memory: &Memory,
        arity: usize,
        dwords: &[usize],
        floats: &[usize],
    ) -> Result<Self, Fault> {
        let mut arguments = [0; 16];
        let sp = state.regs[4];
        for (n, value) in arguments[..arity].iter_mut().enumerate() {
            *value = if n < 4 {
                state.regs[[1, 2, 8, 9][n]]
            } else {
                memory.read_int(add(Mode::X64, sp, 40 + (n as u64 - 4) * 8, 8)?, 8)?
            };
        }
        for &n in dwords {
            arguments[n] = u64::from(arguments[n] as u32);
        }
        for &n in floats {
            arguments[n] = u32::from_le_bytes(state.xmm[n][..4].try_into().unwrap()).into();
        }
        Ok(Self {
            arguments,
            mode: Mode::X64,
            returns: ReturnConvention::Uint64,
            target: memory.read_int(sp, 8)?,
            sp: add(Mode::X64, sp, 8, 8)?,
        })
    }
    pub(crate) fn arguments(
        mode: Mode,
        state: &State,
        memory: &Memory,
        contract: &Contract,
    ) -> Result<[u64; 16], Fault> {
        let mut arguments = [0; 16];
        let sp = mode.address(state.regs[4]);
        let mut offset = 4;
        let mut slot = 0;
        for (index, parameter) in contract.parameters.iter().enumerate() {
            let carrier = match mode {
                Mode::X64 if index < 4 => state.regs[[1, 2, 8, 9][index]],
                Mode::X64 => memory.read_int(add(mode, sp, 40 + (index as u64 - 4) * 8, 8)?, 8)?,
                Mode::X86 if *parameter == Parameter::Variant => {
                    let address = add(mode, sp, offset, 16)?;
                    offset += 16;
                    address
                }
                Mode::X86 => {
                    let size = if matches!(parameter, Parameter::Int64 | Parameter::Uint64) {
                        8
                    } else {
                        4
                    };
                    let value = memory.read_int(add(mode, sp, offset, size)?, size)?;
                    offset += size as u64;
                    value
                }
            };
            if *parameter == Parameter::Variant {
                let size = if mode == Mode::X86 { 16 } else { 24 };
                let mut bytes = [0; 24];
                memory.read(carrier, &mut bytes[..size])?;
                for (n, chunk) in bytes.as_chunks::<8>().0.iter().enumerate() {
                    arguments[slot + n] = u64::from_le_bytes(*chunk);
                }
            } else {
                arguments[slot] = if matches!(parameter, Parameter::Int32 | Parameter::Uint32) {
                    carrier as u32 as u64
                } else {
                    carrier
                };
            }
            slot += parameter.slots();
        }
        Ok(arguments)
    }
    pub fn capture(
        mode: Mode,
        state: &State,
        memory: &Memory,
        contract: &Contract,
    ) -> Result<Self, Fault> {
        let arguments = Self::arguments(mode, state, memory, contract)?;
        let sp = mode.address(state.regs[4]);
        let size = mode as usize / 8;
        let target = memory.read_int(sp, size)?;
        let popped = size as u64
            + if mode == Mode::X86 {
                u64::from(contract.stdcall_bytes())
            } else {
                0
            };
        let sp = add(mode, sp, popped, size)?;
        Ok(Self {
            arguments,
            mode,
            returns: contract.returns,
            target,
            sp,
        })
    }

    pub fn complete(self, state: &mut State, value: u64) {
        state.cancel_repeat();
        state.regs[0] = if self.mode == Mode::X86
            || matches!(
                self.returns,
                ReturnConvention::CharacterCount
                    | ReturnConvention::Hresult
                    | ReturnConvention::BoolLastError
                    | ReturnConvention::Uint32
                    | ReturnConvention::Ntstatus
            ) {
            value as u32 as u64
        } else {
            value
        };
        if self.mode == Mode::X86 && self.returns == ReturnConvention::Uint64 {
            state.regs[2] = value >> 32;
        }
        state.regs[4] = self.sp;
        state.ip = self.target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PAGE, WRITE};
    use crate::win32::catalog::{resolve, Architecture, Resolution};

    fn contract(mode: Mode, dll: &str, name: &str) -> &'static Contract {
        let architecture = if mode == Mode::X86 {
            Architecture::X86
        } else {
            Architecture::X64
        };
        let Resolution::Bound(contract) = resolve(architecture, dll, name) else {
            panic!("missing test ABI");
        };
        contract
    }

    #[test]
    fn win64_register_stack_arguments_and_saved_return_are_independent() {
        let mut memory = Memory::new(Mode::X64);
        memory.map(0x10000, 2 * PAGE, READ | WRITE).unwrap();
        let mut state = State::new(Mode::X64, 0x1234, 0x10fa0);
        let sp = state.regs[4];
        memory.write_int(sp, 8, 0x87654321).unwrap();
        for (index, reg) in [1, 2, 8, 9].into_iter().enumerate() {
            state.regs[reg] = 0xffff_ffff_0000_1000 + index as u64;
        }
        for index in 4..12 {
            memory
                .write_int(sp + 40 + (index - 4) * 8, 8, 0xffff_ffff_0000_1000 + index)
                .unwrap();
        }
        let before = state.clone();
        let abi = contract(Mode::X64, "user32.dll", "CreateWindowExW");
        memory.protect(0x11000, PAGE, 0).unwrap();
        assert!(Call::capture(Mode::X64, &state, &memory, abi).is_err());
        assert_eq!(state, before);
        memory.protect(0x11000, PAGE, READ | WRITE).unwrap();
        let call = Call::capture(Mode::X64, &state, &memory, abi).unwrap();
        assert_eq!(
            &call.arguments[..12],
            &[
                0x1000,
                0xffff_ffff_0000_1001,
                0xffff_ffff_0000_1002,
                0x1003,
                0x1004,
                0x1005,
                0x1006,
                0x1007,
                0xffff_ffff_0000_1008,
                0xffff_ffff_0000_1009,
                0xffff_ffff_0000_100a,
                0xffff_ffff_0000_100b
            ]
        );
        memory.write_int(sp, 8, 0).unwrap();
        call.complete(&mut state, 0xfedc_ba98_7654_3210);
        let mut expected = before;
        expected.regs[0] = 0xfedc_ba98_7654_3210;
        expected.regs[4] += 8;
        expected.ip = 0x87654321;
        assert_eq!(state, expected);
    }

    #[test]
    fn pe32_wide_arguments_and_cdecl_have_distinct_stack_completion() {
        let mut memory = Memory::new(Mode::X86);
        memory.map(0x10000, PAGE, READ | WRITE).unwrap();
        let mut state = State::new(Mode::X86, 0x1234, 0x10000);
        for (offset, size, value) in [
            (0, 4, 0x87654321),
            (4, 4, 0x10203040),
            (8, 8, 0xfedc_ba98_7654_3210),
            (16, 4, 0x50607080),
            (20, 4, 9),
        ] {
            memory.write_int(0x10000 + offset, size, value).unwrap();
        }
        let call = Call::capture(
            Mode::X86,
            &state,
            &memory,
            contract(Mode::X86, "kernel32.dll", "SetFilePointerEx"),
        )
        .unwrap();
        assert_eq!(
            &call.arguments[..4],
            &[0x10203040, 0xfedc_ba98_7654_3210, 0x50607080, 9]
        );
        call.complete(&mut state, 0x1234_5678_0000_0001);
        assert_eq!(
            (state.ip, state.regs[0], state.regs[4]),
            (0x87654321, 1, 0x10018)
        );
        state.regs[4] = 0x10000;
        let call = Call::capture(
            Mode::X86,
            &state,
            &memory,
            contract(Mode::X86, "user32.dll", "wsprintfA"),
        )
        .unwrap();
        assert_eq!(&call.arguments[..2], &[0x10203040, 0x76543210]);
        call.complete(&mut state, 20);
        assert_eq!(state.regs[4], 0x10004);
    }

    #[test]
    fn pe32_return_wraps_stack_and_splits_uint64() {
        let mut memory = Memory::new(Mode::X86);
        memory.map(0xfffff000, PAGE, READ | WRITE).unwrap();
        memory.write_int(0xfffffffc, 4, 0x12345678).unwrap();
        let mut state = State::new(Mode::X86, 0x10000, 0xfffffffc);
        let call = Call::capture(
            Mode::X86,
            &state,
            &memory,
            contract(Mode::X86, "kernel32.dll", "GetTickCount64"),
        )
        .unwrap();
        call.complete(&mut state, 0x1234_5678_9abc_def0);
        assert_eq!(
            (state.ip, state.regs[0], state.regs[2], state.regs[4]),
            (0x12345678, 0x9abcdef0, 0x12345678, 0)
        );
    }

    #[test]
    fn by_value_variants_preserve_following_carriers_and_snapshot_bytes() {
        use crate::win32::catalog::{AccessibilityApi, ApiId, ArchitecturePlan, ExecutionPolicy};
        static ABI: Contract = Contract::new(
            ApiId::Accessibility(AccessibilityApi::PropertyChanged),
            "aggregate-abi-fixture",
            &[
                Parameter::Variant,
                Parameter::Uint32,
                Parameter::Pointer,
                Parameter::Variant,
                Parameter::Uint32,
                Parameter::Pointer,
            ],
            ReturnConvention::Hresult,
            "aggregate ABI regression",
            [ArchitecturePlan {
                dlls: &[],
                execution: ExecutionPolicy::Absent,
            }; 2],
        );
        assert_eq!(ABI.stdcall_bytes(), 48);
        assert_eq!(ABI.win64_dwords(), &[3, 8]);
        for mode in [Mode::X86, Mode::X64] {
            let base = if mode == Mode::X64 {
                0x1_0001_0000
            } else {
                0x10000
            };
            let sp = base + 0x100;
            let width = mode as usize / 8;
            let mut memory = Memory::new(mode);
            memory.map(base, 2 * PAGE, READ | WRITE).unwrap();
            let mut state = State::new(mode, 0x1234, sp);
            memory.write_int(sp, width, 0x76543210).unwrap();
            let variants = [base + 0x800, base + 0x900];
            let words = [
                0x12345678_9abcdef0,
                0xfedcba98_76543210,
                0x99887766_55443322,
            ];
            let mut offset = 4;
            for (index, parameter) in ABI.parameters.iter().enumerate() {
                let value = match index {
                    0 => variants[0],
                    3 => variants[1],
                    2 | 5 => base + index as u64,
                    _ => 0xffff_ffff_1234_0000 + index as u64,
                };
                if mode == Mode::X64 {
                    if index < 4 {
                        state.regs[[1, 2, 8, 9][index]] = value;
                    } else {
                        memory
                            .write_int(sp + 40 + (index as u64 - 4) * 8, 8, value)
                            .unwrap();
                    }
                }
                if *parameter == Parameter::Variant {
                    let pointer = if mode == Mode::X86 {
                        sp + offset
                    } else {
                        value
                    };
                    let count = if mode == Mode::X86 { 2 } else { 3 };
                    for (n, &word) in words[..count].iter().enumerate() {
                        memory.write_int(pointer + n as u64 * 8, 8, word).unwrap();
                    }
                    offset += 16;
                } else {
                    if mode == Mode::X86 {
                        memory.write_int(sp + offset, 4, value).unwrap();
                    }
                    offset += 4;
                }
            }
            let before = state.clone();
            let call = Call::capture(mode, &state, &memory, &ABI).unwrap();
            let third = if mode == Mode::X86 { 0 } else { words[2] };
            assert_eq!(
                &call.arguments[..10],
                &[
                    words[0],
                    words[1],
                    third,
                    0x1234_0001,
                    base + 2,
                    words[0],
                    words[1],
                    third,
                    0x1234_0004,
                    base + 5,
                ]
            );
            memory.write(sp, &[0; 64]).unwrap();
            for pointer in variants {
                memory.write(pointer, &[0; 24]).unwrap();
            }
            assert_eq!(call.arguments[0], words[0]);
            call.complete(&mut state, 0xffff_ffff_8000_4001);
            let mut expected = before;
            expected.ip = 0x76543210;
            expected.regs[0] = 0x80004001;
            expected.regs[4] += if mode == Mode::X86 { 52 } else { 8 };
            assert_eq!(state, expected);
        }
    }

    #[test]
    fn by_value_variant_and_return_faults_do_not_publish_a_call() {
        for mode in [Mode::X86, Mode::X64] {
            let mut memory = Memory::new(mode);
            memory.map(0x10000, 3 * PAGE, READ | WRITE).unwrap();
            let sp = if mode == Mode::X86 { 0x10ffc } else { 0x10ff8 };
            let mut state = State::new(mode, 0x1234, sp);
            memory.write_int(sp, mode as usize / 8, 0x76543210).unwrap();
            let abi = contract(
                mode,
                "uiautomationcore.dll",
                "UiaRaiseAutomationPropertyChangedEvent",
            );
            assert_eq!(abi.stdcall_bytes(), 40);
            if mode == Mode::X86 {
                memory.write(sp + 4, &[0; 40]).unwrap();
            } else {
                state.regs[1] = 1;
                state.regs[2] = 30000;
                state.regs[8] = 0x11ff0;
                state.regs[9] = 0x12040;
            }
            let before = state.clone();
            let page = if mode == Mode::X86 { 0x11000 } else { 0x12000 };
            memory.protect(page, PAGE, 0).unwrap();
            let fault = Call::capture(mode, &state, &memory, abi).err().unwrap();
            assert_eq!(fault.access, READ);
            assert_eq!(state, before);
            memory.protect(page, PAGE, READ | WRITE).unwrap();
            assert!(Call::capture(mode, &state, &memory, abi).is_ok());
            memory.protect(0x10000, PAGE, 0).unwrap();
            let fault = Call::capture(mode, &state, &memory, abi).err().unwrap();
            assert_eq!(fault.address, sp);
            assert_eq!(state, before);
            memory.protect(0x10000, PAGE, READ | WRITE).unwrap();
            let call = Call::capture(mode, &state, &memory, abi).unwrap();
            call.complete(&mut state, 0x80004001);
            assert_eq!(state.ip, 0x76543210);
            assert_eq!(state.regs[4], sp + if mode == Mode::X86 { 44 } else { 8 });
        }
    }
}
