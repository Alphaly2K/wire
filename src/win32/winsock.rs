//! Winsock startup, events and address conversion. No network socket backend.
//! Public ABI/behavior checked against fixed Wine 10.0 headers/protocol tests;
//! address conversion is independently implemented with Rust's IP value types.
use crate::win32::catalog::WinsockApi;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

pub(crate) trait Backend {
    type Error;
    fn width(&self) -> usize;
    fn check_write(&self, address: u64, size: usize) -> Result<(), Self::Error>;
    fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn string(&self, address: u64, wide: bool) -> Result<Vec<u8>, Self::Error>;
    fn create_event(&mut self) -> Result<(u64, bool), u32>;
    fn close_event(&mut self, handle: u64) -> bool;
    fn change_event(&mut self, handle: u64, signaled: bool) -> Result<u64, u32>;
    fn read_int(&self, address: u64, size: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(address, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn write_int(&mut self, address: u64, size: usize, value: u64) -> Result<(), Self::Error> {
        self.write(address, &value.to_le_bytes()[..size])
    }
}
#[derive(Default)]
pub(crate) struct Winsock {
    references: u32,
    pub(crate) inet_buffers: BTreeMap<u32, u64>,
    pub(crate) results: BTreeMap<u64, bool>,
}

pub(crate) fn ipv4(input: &[u8]) -> Option<[u8; 4]> {
    if !input.is_empty() && input.iter().all(u8::is_ascii_whitespace) {
        return Some([0; 4]);
    }
    let input = std::str::from_utf8(input)
        .ok()?
        .trim_end_matches(|c: char| c.is_ascii_whitespace());
    let parts: Vec<_> = input.split('.').collect();
    if !(1..=4).contains(&parts.len()) {
        return None;
    }
    let mut address = 0u32;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || part.starts_with(['+', '-']) {
            return None;
        }
        let (radix, digits) = if part.starts_with("0x") || part.starts_with("0X") {
            (16, &part[2..])
        } else if part.starts_with('0') && part.len() > 1 {
            (8, &part[1..])
        } else {
            (10, *part)
        };
        let value = u32::from_str_radix(digits, radix).ok()?;
        let shift = if i + 1 == parts.len() { 0 } else { 24 - i * 8 };
        let bits = if i + 1 == parts.len() {
            (5 - parts.len()) * 8
        } else {
            8
        };
        if bits != 32 && value >= 1u32 << bits {
            return None;
        }
        address |= value << shift;
    }
    Some(address.to_be_bytes())
}

impl Winsock {
    pub(crate) fn initialized(&self) -> bool {
        self.references != 0
    }

    pub(crate) fn call<B: Backend>(
        &mut self,
        mem: &mut B,
        id: WinsockApi,
        args: &[u64],
        last_error: u64,
    ) -> Result<u64, B::Error> {
        use WinsockApi::*;
        match id {
            Address => {
                if args[0] == 0 {
                    return Ok(u32::MAX as u64);
                }
                let text = mem.string(args[0], false)?;
                Ok(ipv4(&text[..text.len() - 1]).map_or(u32::MAX, u32::from_le_bytes) as u64)
            }
            FdIsSet => {
                if args[1] == 0 {
                    return Ok(0);
                }
                let count = mem.read_int(args[1], 4)?;
                if count > 64 {
                    mem.write_int(last_error, 4, 10022)?;
                    return Ok(0);
                }
                let width = mem.width();
                let mut bytes = vec![0; count as usize * width];
                if count != 0 {
                    mem.read(args[1] + width as u64, &mut bytes)?;
                }
                Ok(u64::from(bytes.chunks_exact(width).any(|part| {
                    let mut value = [0; 8];
                    value[..width].copy_from_slice(part);
                    u64::from_le_bytes(value) == args[0]
                })))
            }
            IsBlocking => Ok(0),
            UnhookBlocking if self.initialized() => Ok(0),
            SetBlockingHook if self.initialized() && args[0] == 0 => Ok(0),
            Select => {
                let error = if !self.initialized() {
                    10093
                } else {
                    let mut any = false;
                    for &pointer in &args[1..4] {
                        if pointer != 0 {
                            any |= mem.read_int(pointer, 4)? != 0;
                        }
                    }
                    if any {
                        10038
                    } else {
                        10022
                    }
                };
                mem.write_int(last_error, 4, error)?;
                Ok(u32::MAX as u64)
            }
            Accept | Bind | Close | Connect | PeerName | SocketName | SocketOpt | Ioctl
            | Listen | Receive | ReceiveFrom | Send | SendTo | SetSocketOpt | Shutdown
            | AsyncSelect | EventSelect | NetworkEvents | IoctlEx | SendDisconnect
            | ReceiveDisconnect | ReceiveEx | ReceiveFromEx | SendEx | SendToEx => {
                mem.write_int(
                    last_error,
                    4,
                    if self.initialized() { 10038 } else { 10093 },
                )?;
                Ok(if id == Accept && mem.width() == 8 {
                    u64::MAX
                } else {
                    u32::MAX as u64
                })
            }
            AsyncHostByAddress | AsyncHostByName | AsyncProtoByNumber | AsyncProtoByName
            | AsyncServiceByPort | AsyncServiceByName | SetBlockingHook => {
                mem.write_int(
                    last_error,
                    4,
                    if self.initialized() { 10045 } else { 10093 },
                )?;
                Ok(0)
            }
            ProtoByName | ProtoByNumber | ServiceByName | ServiceByPort => {
                mem.write_int(
                    last_error,
                    4,
                    if self.initialized() { 11004 } else { 10093 },
                )?;
                Ok(0)
            }
            CancelAsync | CancelBlocking | UnhookBlocking => {
                mem.write_int(
                    last_error,
                    4,
                    if self.initialized() { 10022 } else { 10093 },
                )?;
                Ok(u32::MAX as u64)
            }
            AddressText | HostByAddress | HostByName | HostName | HostNameW | AddrInfoA
            | AddrInfoW | FreeAddrInfoA | FreeAddrInfoW | AddrInfoExW | FreeAddrInfoExW
            | CancelAddrInfo | NameInfoA | NameInfoW | AddrInfoResult => {
                unreachable!("owned name results use the runtime adapter")
            }
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
                Ok(if mem.width() == 4 {
                    u32::MAX as u64
                } else {
                    u64::MAX
                })
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
                let input = mem.string(args[1], wide)?;
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
                match mem.create_event() {
                    Ok((handle, _)) => Ok(handle),
                    Err(_) => {
                        mem.write_int(last_error, 4, 10055)?;
                        Ok(0)
                    }
                }
            }
            CloseEvent => {
                if mem.close_event(args[0]) {
                    Ok(1)
                } else {
                    mem.write_int(last_error, 4, 6)?;
                    Ok(0)
                }
            }
            SetEvent | ResetEvent => match mem.change_event(args[0], id == SetEvent) {
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
                    mem.check_write(args[1], if mem.width() == 4 { 400 } else { 408 })?;
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
                    if mem.width() == 8 {
                        mem.write(args[1], &header)?;
                        mem.write(args[1] + 16, &strings)?;
                    } else {
                        mem.write(args[1], &header[..4])?;
                        mem.write(args[1] + 4, &strings)?;
                        mem.write(args[1] + 390, &header[4..])?;
                    }
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
