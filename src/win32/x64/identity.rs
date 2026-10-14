//! Isolated identity adapter; borrows only memory, immutable defaults and the error slot.
use crate::x64::memory::{Memory, WRITE};
pub(crate) fn call(
    mem: &mut Memory,
    id: crate::win32::catalog::IdentityApi,
    a: &[u64],
    environment: &crate::runtime::environment::GuestEnvironment,
    error_slot: u64,
) -> Result<u64, String> {
    let extended = id.extended();
    let text = if id.user() {
        environment.user_name()
    } else if extended {
        match a[0] {
            0 | 4 => environment.computer_name(), // NetBIOS (logical and physical are the same virtual machine).
            1 | 3 | 5 | 7 => environment.dns_hostname(), // DNS hostname / FQDN of the unjoined profile.
            2 | 6 => environment.dns_domain(),           // No DNS domain membership.
            _ => {
                mem.write_int(error_slot, 4, 87)?;
                return Ok(0);
            }
        }
    } else {
        environment.computer_name()
    };
    let out = a[extended as usize];
    let size = a[1 + extended as usize];
    let capacity = mem.read_int(size, 4)?;
    let required = text.len() as u64 + 1;
    mem.check(size, 4, WRITE)?;
    if capacity < required {
        mem.write_int(size, 4, required)?;
        mem.write_int(
            error_slot,
            4,
            if id.user() {
                122
            } else if extended {
                234
            } else {
                111
            },
        )?;
        return Ok(0);
    }
    let bytes: Vec<u8> = if id.wide() {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        text.bytes().chain([0]).collect()
    };
    mem.check(out, bytes.len(), WRITE)?;
    mem.write(out, &bytes)?;
    mem.write_int(size, 4, if id.user() { required } else { required - 1 })?;
    Ok(1)
}
