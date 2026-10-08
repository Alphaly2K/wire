//! WinHTTP discovery without an Internet Explorer profile or HTTP transport.
//! Unavailable proxy discovery/session creation returns real API failure.
use super::*;
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "WinHttpGetIEProxyConfigForCurrentUser"
        | "WinHttpGetDefaultProxyConfiguration"
        | "WinHttpCloseHandle" => 1,
        "WinHttpGetProxyForUrl" => 4,
        "WinHttpOpen" => 5,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    dll.eq_ignore_ascii_case("winhttp.dll") && arity(name).is_some()
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    if name == "WinHttpOpen" {
        &[1, 4]
    } else {
        &[]
    }
}
impl Runtime64 {
    pub(super) fn winhttp_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let error = match name {
            "WinHttpGetIEProxyConfigForCurrentUser" => {
                if a[0] == 0 {
                    87
                } else {
                    mem.check(a[0], 32, WRITE)?;
                    // No IE proxy settings exist in the isolated guest profile.
                    // Failure does not publish automatic detection/PAC/proxy data.
                    2 // ERROR_FILE_NOT_FOUND
                }
            }
            "WinHttpGetDefaultProxyConfiguration" => {
                if a[0] == 0 {
                    87
                } else {
                    mem.check(a[0], 24, WRITE)?;
                    // The virtual profile has explicit direct system settings.
                    let mut info = [0; 24];
                    info[..4].copy_from_slice(&1u32.to_le_bytes());
                    mem.write(a[0], &info)?;
                    return Ok(1);
                }
            }
            "WinHttpOpen" => 50, // No native HTTP transport, no fake HINTERNET.
            "WinHttpGetProxyForUrl" | "WinHttpCloseHandle" => 6, // No valid session handles exist.
            _ => return Err(format!("unsupported WinHTTP call {name}")),
        };
        self.set_error(mem, error)?;
        Ok(0)
    }
}
