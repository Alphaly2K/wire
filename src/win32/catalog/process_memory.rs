use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessMemoryApi {
    Read,
    Write,
}

const fn declaration(id: ProcessMemoryApi, name: &'static str) -> Contract {
    Contract::new(
        ApiId::ProcessMemory(id),
        name,
        &[Parameter::Pointer; 5],
        ReturnConvention::BoolLastError,
        "process handle, remote address, caller buffer, SIZE_T length and optional SIZE_T output",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    )
}

pub static PROCESS_MEMORY_CONTRACTS: [Contract; 2] = [
    declaration(ProcessMemoryApi::Read, "ReadProcessMemory"),
    declaration(ProcessMemoryApi::Write, "WriteProcessMemory"),
];
