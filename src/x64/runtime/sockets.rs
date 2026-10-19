//! Winsock startup, events and address conversion. No network socket backend.
//! Public ABI/behavior checked against fixed Wine 10.0 headers/protocol tests;
//! address conversion is independently implemented with Rust's IP value types.
use super::{events::Events, Memory, WRITE};
use crate::win32::{catalog::WinsockApi, x64::text::string_bytes};
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Default)]
pub(super) struct Winsock {
    references: u32,
}

impl Winsock {
    pub(super) fn initialized(&self) -> bool {
        self.references != 0
    }

    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: WinsockApi,
        args: &[u64],
        last_error: u64,
        events: &mut Events,
    ) -> Result<u64, String> {
        use WinsockApi::*;
        match id {
            Socket | SocketA | SocketW => {
                // This isolated Winsock profile has no installed network
                // transport provider. Events and address values remain usable,
                // but no address family can create a network SOCKET.
                let error = if self.references == 0 {
                    10093 // WSANOTINITIALISED
                } else if id != Socket && args[3] != 0 {
                    10045 // WSAEOPNOTSUPP: custom protocol/duplicate socket info.
                } else {
                    10047 // WSAEAFNOSUPPORT: no transport provider for any family.
                };
                mem.write_int(last_error, 4, error)?;
                Ok(u64::MAX) // INVALID_SOCKET is pointer-width on Win64.
            }
            Htonl | Ntohl => Ok((args[0] as u32).swap_bytes() as u64),
            Htons | Ntohs => Ok((args[0] as u16).swap_bytes() as u64),
            PtonA | PtonW => {
                if args[1] == 0 || args[2] == 0 {
                    mem.write_int(last_error, 4, 10014)?; // WSAEFAULT precedes family validation.
                    return Ok(u32::MAX as u64);
                }
                if !matches!(args[0], 2 | 23) {
                    mem.write_int(last_error, 4, 10047)?; // Windows AF_INET6 is 23, not host AF_INET6.
                    return Ok(u32::MAX as u64);
                }
                let wide = id == PtonW;
                let input = string_bytes(mem, args[1], u32::MAX as u64, if wide { 2 } else { 1 })?;
                let text = if wide {
                    let units: Vec<_> = input[..input.len() - 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect();
                    String::from_utf16_lossy(&units)
                } else {
                    String::from_utf8_lossy(&input[..input.len() - 1]).into_owned()
                };
                let address = if args[0] == 2 {
                    text.parse::<Ipv4Addr>().ok().map(|ip| ip.octets().to_vec())
                } else {
                    text.parse::<Ipv6Addr>().ok().map(|ip| ip.octets().to_vec())
                };
                if let Some(bytes) = address {
                    // Stage all source bytes before the checked, atomic store.
                    // Failed syntax leaves the destination undefined by the API;
                    // this implementation preserves it instead of partial parsing.
                    mem.write(args[2], &bytes)?;
                    Ok(1)
                } else {
                    if wide {
                        mem.write_int(last_error, 4, 10022)?; // ANSI syntax failure preserves LastError.
                    }
                    Ok(0)
                }
            }
            NtopA | NtopW => {
                let wide = id == NtopW;
                let invalid = if wide { 87 } else { 0xc000000d }; // Win10 ANSI returns NTSTATUS.
                if !wide && args[2] == 0 {
                    mem.write_int(last_error, 4, invalid)?;
                    return Ok(0);
                }
                let count = match args[0] {
                    2 => 4,
                    23 => 16,
                    _ => {
                        mem.write_int(last_error, 4, 10047)?;
                        return Ok(0);
                    }
                };
                let mut address = [0; 16];
                mem.read(args[1], &mut address[..count])?;
                let text = if count == 4 {
                    Ipv4Addr::from(<[u8; 4]>::try_from(&address[..4]).unwrap()).to_string()
                } else if address[..12] == [0; 12]
                    && address[12..] != [0, 0, 0, 0]
                    && address[12..] != [0, 0, 0, 1]
                {
                    // Windows also prints IPv4-compatible addresses in dotted form.
                    format!(
                        "::{}",
                        Ipv4Addr::from(<[u8; 4]>::try_from(&address[12..]).unwrap())
                    )
                } else {
                    Ipv6Addr::from(address).to_string()
                };
                if args[2] == 0 || args[3] < text.len() as u64 + 1 {
                    mem.write_int(last_error, 4, invalid)?;
                    return Ok(0);
                }
                let bytes: Vec<_> = if wide {
                    text.encode_utf16()
                        .chain([0])
                        .flat_map(u16::to_le_bytes)
                        .collect()
                } else {
                    text.bytes().chain([0]).collect()
                };
                mem.write(args[2], &bytes)?;
                Ok(args[2])
            }
            CreateEvent => {
                if self.references == 0 {
                    mem.write_int(last_error, 4, 10093)?;
                    return Ok(0);
                }
                match events.create(true, false, None, 0x1f0003) {
                    Ok((handle, _)) => Ok(handle),
                    Err(_) => {
                        mem.write_int(last_error, 4, 10055)?;
                        Ok(0)
                    }
                }
            }
            CloseEvent => {
                if events.close(args[0]) {
                    Ok(1)
                } else {
                    mem.write_int(last_error, 4, 6)?;
                    Ok(0)
                }
            }
            SetEvent | ResetEvent => match events.change(args[0], id == SetEvent) {
                Ok(v) => Ok(v),
                Err(e) => {
                    mem.write_int(last_error, 4, e as u64)?;
                    Ok(0)
                }
            },
            Startup => {
                let requested = args[0] as u16;
                let major = requested as u8;
                let minor = (requested >> 8) as u8;
                if args[1] != 0 {
                    // Win64 reorders WSADATA fields: vendor pointer at 8,
                    // description at 16, status at 273, sizeof=408 (alignment=8).
                    // The unused vendor pointer and tail padding are untouched.
                    mem.check(args[1], 408, WRITE)?;
                }
                let next = self.references.checked_add(1);
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
                    self.references = next.unwrap();
                    Ok(0)
                }
            }
            Cleanup => {
                if self.references == 0 {
                    mem.write_int(last_error, 4, 10093)?; // WSANOTINITIALISED
                    Ok(u32::MAX as u64) // SOCKET_ERROR, signed 32-bit return.
                } else {
                    self.references -= 1;
                    Ok(0)
                }
            }
            GetLastError => mem.read_int(last_error, 4),
            SetLastError => {
                mem.write_int(last_error, 4, args[0] as u32 as u64)?;
                Ok(0)
            }
            WaitEvents => unreachable!("Winsock waits are process-controlled"),
        }
    }
}
