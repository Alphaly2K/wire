//! Win64 virtual hardware and checked output layout, independent of host ISA.
use crate::{
    win32::catalog::SystemProfileApi,
    x64::memory::{Memory, PAGE},
};

pub(crate) fn call(
    mem: &mut Memory,
    id: SystemProfileApi,
    args: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    match id {
        SystemProfileApi::SystemInfo => {
            let mut info = [0; 48];
            info[..2].copy_from_slice(&9u16.to_le_bytes()); // PROCESSOR_ARCHITECTURE_AMD64
            info[4..8].copy_from_slice(&(PAGE as u32).to_le_bytes());
            info[8..16].copy_from_slice(&0x10000u64.to_le_bytes());
            info[16..24].copy_from_slice(&((1u64 << 47) - 65537).to_le_bytes());
            info[24..32].copy_from_slice(&1u64.to_le_bytes()); // One virtual guest CPU.
            info[32..36].copy_from_slice(&1u32.to_le_bytes());
            info[36..40].copy_from_slice(&8664u32.to_le_bytes()); // PROCESSOR_AMD_X8664
            info[40..44].copy_from_slice(&65536u32.to_le_bytes());
            info[44..46].copy_from_slice(&6u16.to_le_bytes());
            mem.write(args[0], &info)?;
            Ok(0)
        }
        SystemProfileApi::MemoryStatusEx => {
            if mem.read_int(args[0], 4)? != 64 {
                mem.write_int(last_error, 4, 87)?;
                Ok(0)
            } else {
                let status = mem.memory_status();
                mem.write(args[0], &status)?;
                Ok(1)
            }
        }
        SystemProfileApi::MemoryStatus => unreachable!("GlobalMemoryStatus has no Win64 binding"),
    }
}
