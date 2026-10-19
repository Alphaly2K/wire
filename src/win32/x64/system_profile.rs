//! Win64 virtual hardware and checked output layout, independent of host ISA.
use crate::{
    win32::catalog::SystemProfileApi,
    x64::memory::{Memory, PAGE, WRITE},
};

pub(crate) fn call(
    mem: &mut Memory,
    id: SystemProfileApi,
    args: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    match id {
        SystemProfileApi::ProcessorTopology => {
            if let Some(data) = processor_information(args[0] as u32) {
                mem.check(args[2], 4, WRITE)?;
                let capacity = mem.read_int(args[2], 4)? as usize;
                if capacity < data.len() {
                    mem.write_int(args[2], 4, data.len() as u64)?;
                    mem.write_int(last_error, 4, 122)?;
                    Ok(0)
                } else {
                    mem.check(args[1], data.len(), WRITE)?;
                    mem.write(args[1], &data)?;
                    mem.write_int(args[2], 4, data.len() as u64)?;
                    Ok(1)
                }
            } else {
                mem.write_int(last_error, 4, 87)?;
                Ok(0)
            }
        }
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

fn processor_information(relation: u32) -> Option<Vec<u8>> {
    if !matches!(relation, 0 | 1 | 2 | 3 | 4 | 6 | 0xffff) {
        return None;
    }
    let mut output = Vec::new();
    // The interpreter exposes one virtual CPU, one NUMA node and one group.
    // No hardware cache is advertised (also consistent with the CPUID profile).
    for (kind, size) in [(0u32, 48usize), (1, 48), (3, 48), (4, 80), (6, 48)] {
        if kind != relation && !(relation == 0xffff && kind != 6) {
            continue;
        }
        let mut record = vec![0; size];
        record[..4].copy_from_slice(&kind.to_le_bytes());
        record[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        if kind == 4 {
            record[8..10].copy_from_slice(&1u16.to_le_bytes());
            record[10..12].copy_from_slice(&1u16.to_le_bytes());
            record[32] = 1;
            record[33] = 1;
            record[72..80].copy_from_slice(&1u64.to_le_bytes());
        } else {
            if kind != 1 {
                record[30..32].copy_from_slice(&1u16.to_le_bytes());
            }
            record[32..40].copy_from_slice(&1u64.to_le_bytes());
        }
        output.extend(record);
    }
    Some(output)
}
