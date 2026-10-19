//! WinHTTP discovery without an Internet Explorer profile or HTTP transport.
//! Unavailable proxy discovery/session creation returns real API failure.
use crate::{
    win32::catalog::WinHttpApi,
    x64::memory::{Memory, WRITE},
};

pub(crate) fn call(
    mem: &mut Memory,
    id: WinHttpApi,
    a: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    let error = match id {
        WinHttpApi::UserProxy => {
            if a[0] == 0 {
                87
            } else {
                mem.check(a[0], 32, WRITE)?;
                // No IE proxy settings exist in the isolated guest profile.
                // Failure does not publish automatic detection/PAC/proxy data.
                2 // ERROR_FILE_NOT_FOUND
            }
        }
        WinHttpApi::DefaultProxy => {
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
        WinHttpApi::Open => 50, // No native HTTP transport, no fake HINTERNET.
        WinHttpApi::ProxyForUrl | WinHttpApi::Close => 6, // No valid session handles exist.
    };
    mem.write_int(last_error, 4, error)?;
    Ok(0)
}
