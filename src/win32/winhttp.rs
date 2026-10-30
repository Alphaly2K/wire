//! WinHTTP discovery for the isolated direct-proxy profile, without a transport.
use super::catalog::WinHttpApi;

pub(crate) trait Memory {
    type Error;
    fn check_write(&self, address: u64, length: usize) -> Result<(), Self::Error>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn write_error(&mut self, address: u64, code: u64) -> Result<(), Self::Error>;
}

pub(crate) fn call<M: Memory>(
    mem: &mut M,
    id: WinHttpApi,
    a: &[u64],
    last_error: u64,
    width: usize,
) -> Result<u64, M::Error> {
    let error = match id {
        WinHttpApi::UserProxy => {
            if a[0] == 0 {
                87
            } else {
                mem.check_write(a[0], if width == 4 { 16 } else { 32 })?;
                // No IE proxy settings exist in the isolated guest profile.
                // Failure does not publish automatic detection/PAC/proxy data.
                2 // ERROR_FILE_NOT_FOUND
            }
        }
        WinHttpApi::DefaultProxy => {
            if a[0] == 0 {
                87
            } else {
                let size = if width == 4 { 12 } else { 24 };
                mem.check_write(a[0], size)?;
                // The virtual profile has explicit direct system settings.
                let mut info = [0; 24];
                info[..4].copy_from_slice(&1u32.to_le_bytes());
                mem.write(a[0], &info[..size])?;
                return Ok(1);
            }
        }
        WinHttpApi::Open => 50, // No native HTTP transport, no fake HINTERNET.
        WinHttpApi::ProxyForUrl | WinHttpApi::Close => 6, // No valid session handles exist.
    };
    mem.write_error(last_error, error)?;
    Ok(0)
}
