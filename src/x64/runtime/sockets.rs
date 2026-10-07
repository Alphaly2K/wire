//! Winsock startup state and AMD64 output layout; sockets remain explicit boundaries.
//! ABI/negotiation checked against Wine 10.0 winsock2.h and ws2_32/socket.c.
use super::{Memory, Runtime64, WRITE};

pub(super) fn ordinal(name: &str) -> Option<&'static str> {
    match name.strip_prefix('#')?.parse::<u16>().ok()? {
        111 => Some("WSAGetLastError"),
        112 => Some("WSASetLastError"),
        115 => Some("WSAStartup"),
        116 => Some("WSACleanup"),
        _ => None,
    }
}
pub(super) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if dll.eq_ignore_ascii_case("ws2_32.dll") {
        ordinal(name).unwrap_or(name)
    } else {
        name
    }
}
pub(super) fn is_call(name: &str) -> bool {
    matches!(
        name,
        "WSAStartup" | "WSACleanup" | "WSAGetLastError" | "WSASetLastError"
    )
}

impl Runtime64 {
    pub(super) fn winsock_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        match name {
            "WSAStartup" => {
                let requested = args[0] as u16;
                let major = requested as u8;
                let minor = (requested >> 8) as u8;
                if args[1] != 0 {
                    // Win64 reorders WSADATA fields: vendor pointer at 8,
                    // description at 16, status at 273, sizeof=408 (alignment=8).
                    // The unused vendor pointer and tail padding are untouched.
                    mem.check(args[1], 408, WRITE)?;
                }
                let next = self.winsock_refs.checked_add(1);
                if major != 0 && args[1] != 0 && next.is_none() {
                    return Ok(10067); // WSAEPROCLIM, no reference/output mutation.
                }
                if args[1] != 0 {
                    let negotiated = match major {
                        1 => u16::from_le_bytes([1, minor.min(1)]),
                        2 => u16::from_le_bytes([2, minor.min(2)]),
                        _ => 0x0202,
                    };
                    let mut header = [0; 8];
                    header[..2].copy_from_slice(&negotiated.to_le_bytes());
                    header[2..4].copy_from_slice(&0x0202u16.to_le_bytes());
                    if major == 1 {
                        header[4..6].copy_from_slice(&32767u16.to_le_bytes());
                        header[6..8].copy_from_slice(&65467u16.to_le_bytes());
                    }
                    let mut strings = [0; 386];
                    let description = b"Wire Winsock services";
                    strings[..description.len()].copy_from_slice(description);
                    strings[257..264].copy_from_slice(b"Running");
                    mem.write(args[1], &header)?;
                    mem.write(args[1] + 16, &strings)?;
                }
                // WSAStartup returns errors directly and preserves last error.
                if major == 0 {
                    Ok(10092) // WSAVERNOTSUPPORTED
                } else if args[1] == 0 {
                    Ok(10014) // WSAEFAULT
                } else {
                    self.winsock_refs = next.unwrap();
                    Ok(0)
                }
            }
            "WSACleanup" => {
                if self.winsock_refs == 0 {
                    self.set_error(mem, 10093)?; // WSANOTINITIALISED
                    Ok(u32::MAX as u64) // SOCKET_ERROR, signed 32-bit return.
                } else {
                    self.winsock_refs -= 1;
                    Ok(0)
                }
            }
            "WSAGetLastError" => mem.read_int(self.teb() + 0x68, 4),
            "WSASetLastError" => {
                self.set_error(mem, args[0] as u32)?;
                Ok(0)
            }
            _ => unreachable!(),
        }
    }
}
