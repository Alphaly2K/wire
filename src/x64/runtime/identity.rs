//! Identity of the isolated guest profile; this does not query host accounts/DNS.
use super::{Memory, Runtime64, WRITE};
pub(super) fn user(name: &str) -> bool {
    matches!(name, "GetUserNameA" | "GetUserNameW")
}
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "GetComputerNameA" | "GetComputerNameW" | "GetUserNameA" | "GetUserNameW" => 2,
        "GetComputerNameExA" | "GetComputerNameExW" => 3,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    arity(name).is_some()
        && if user(name) {
            dll.eq_ignore_ascii_case("advapi32.dll")
        } else {
            dll.eq_ignore_ascii_case("kernel32.dll") || dll.eq_ignore_ascii_case("kernelbase.dll")
        }
}
impl Runtime64 {
    pub(super) fn identity_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let extended = name.contains("Ex");
        let text = if user(name) {
            "wire"
        } else if extended {
            match a[0] {
                0 | 4 => "WIRE", // NetBIOS (logical and physical are the same virtual machine).
                1 | 3 | 5 | 7 => "wire", // DNS hostname / FQDN of the unjoined profile.
                2 | 6 => "",     // No DNS domain membership.
                _ => {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
            }
        } else {
            "WIRE"
        };
        let out = a[extended as usize];
        let size = a[1 + extended as usize];
        let capacity = mem.read_int(size, 4)?;
        let required = text.len() as u64 + 1;
        mem.check(size, 4, WRITE)?;
        if capacity < required {
            mem.write_int(size, 4, required)?;
            self.set_error(
                mem,
                if user(name) {
                    122
                } else if extended {
                    234
                } else {
                    111
                },
            )?;
            return Ok(0);
        }
        let bytes: Vec<u8> = if name.ends_with('W') {
            text.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            text.bytes().chain([0]).collect()
        };
        mem.check(out, bytes.len(), WRITE)?;
        mem.write(out, &bytes)?;
        mem.write_int(size, 4, if user(name) { required } else { required - 1 })?;
        Ok(1)
    }
}
