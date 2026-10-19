//! Checked directory query copyout; no filesystem or process mutation.
use crate::{
    runtime::environment::GuestEnvironment, win32::catalog::SystemPathApi, x64::memory::Memory,
};

pub(crate) fn call(
    mem: &mut Memory,
    id: SystemPathApi,
    args: &[u64],
    profile: &GuestEnvironment,
) -> Result<u64, String> {
    if matches!(id, SystemPathApi::TempA | SystemPathApi::Temp2A) {
        unreachable!("ANSI temp directory queries have no Win64 binding");
    }
    let path = if id.temporary() {
        profile.temporary_directory()
    } else if id.system() {
        profile.system_directory()
    } else {
        profile.windows_directory()
    };
    let wide = id.wide();
    let bytes: Vec<u8> = if wide {
        path.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        path.bytes().chain([0]).collect()
    };
    let required = bytes.len() / if wide { 2 } else { 1 };
    let (pointer, capacity) = if id.temporary() {
        (args[1], args[0])
    } else {
        (args[0], args[1])
    };
    if capacity < required as u64 {
        Ok(required as u64)
    } else {
        mem.write(pointer, &bytes)?;
        Ok(required as u64 - 1)
    }
}
