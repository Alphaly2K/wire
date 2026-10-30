//! Local and numeric name results use checked, runtime-owned guest storage.
use super::{
    catalog::WinsockApi as W,
    context,
    modules::{self, Failure},
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn text(memory: &Memory, pointer: u64, wide: bool) -> Result<String, ServiceError> {
    modules::name(memory, pointer, wide).map_err(|error| match error {
        Failure::Fault(fault) => fault.into(),
        Failure::Windows(_) => ServiceError::Unavailable("invalid Winsock name"),
    })
}
fn string_bytes(value: &str, wide: bool) -> Vec<u8> {
    if wide {
        value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        value.bytes().chain([0]).collect()
    }
}
fn put(bytes: &mut [u8], at: usize, width: usize, value: u64) {
    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}
fn failure(
    memory: &mut Memory,
    state: &State,
    code: u32,
    value: u64,
    outputs: &[u64],
) -> Result<u64, ServiceError> {
    let width = memory.mode() as usize / 8;
    let error = context::error_address(state, memory.mode());
    for &pointer in outputs {
        if pointer != 0 {
            memory.check(pointer, width, WRITE)?;
        }
    }
    memory.check(error, 4, WRITE)?;
    for &pointer in outputs {
        if pointer != 0 {
            memory.write_int(pointer, width, 0)?;
        }
    }
    memory.write_int(error, 4, code.into())?;
    Ok(value)
}
fn thread_buffer(memory: &mut Memory, image: &mut Loaded) -> Option<u64> {
    let id = image.threads.active;
    if let Some(&base) = image.winsock.inet_buffers.get(&id) {
        return Some(base);
    }
    let base = image.data_pages.allocate(memory, 512);
    if base == 0 {
        return None;
    }
    image.winsock.inet_buffers.insert(id, base);
    Some(base)
}
fn ipv4_name(name: &str, local: &str) -> Option<([u8; 4], String)> {
    if name.is_empty() || name.eq_ignore_ascii_case(local) {
        Some(([127, 0, 0, 1], local.to_owned()))
    } else if name.eq_ignore_ascii_case("localhost") {
        Some(([127, 0, 0, 1], "localhost".into()))
    } else {
        super::winsock_core::ipv4(name.as_bytes()).map(|ip| (ip, Ipv4Addr::from(ip).to_string()))
    }
}
fn host_record(
    memory: &mut Memory,
    image: &mut Loaded,
    state: &State,
    address: [u8; 4],
    name: &str,
) -> Result<u64, ServiceError> {
    let Some(base) = thread_buffer(memory, image) else {
        return failure(memory, state, 10055, 0, &[]);
    };
    let width = memory.mode() as usize / 8;
    let mut bytes = [0; 448];
    let output = base + 64;
    put(&mut bytes, 0, width, base + 128);
    put(&mut bytes, width, width, base + 400);
    put(&mut bytes, width * 2, 2, 2);
    put(&mut bytes, width * 2 + 2, 2, 4);
    put(
        &mut bytes,
        if width == 4 { 12 } else { 24 },
        width,
        base + 416,
    );
    let name = string_bytes(name, false);
    bytes[64..64 + name.len()].copy_from_slice(&name);
    put(&mut bytes, 352, width, base + 448);
    bytes[384..388].copy_from_slice(&address);
    memory.write(output, &bytes)?;
    Ok(output)
}

pub(super) fn handles(id: W) -> bool {
    matches!(
        id,
        W::AddressText
            | W::HostByAddress
            | W::HostByName
            | W::HostName
            | W::HostNameW
            | W::AddrInfoA
            | W::AddrInfoW
            | W::FreeAddrInfoA
            | W::FreeAddrInfoW
            | W::AddrInfoExW
            | W::FreeAddrInfoExW
            | W::CancelAddrInfo
            | W::NameInfoA
            | W::NameInfoW
            | W::AddrInfoResult
    )
}
pub(super) fn dispatch(
    id: W,
    a: &[u64; 16],
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let local = image.environment.profile.dns_hostname();
    match id {
        W::AddressText => {
            let bytes = string_bytes(
                &Ipv4Addr::from((a[0] as u32).to_le_bytes()).to_string(),
                false,
            );
            let Some(base) = thread_buffer(memory, image) else {
                return failure(memory, state, 10055, 0, &[]);
            };
            memory.write(base, &bytes)?;
            Ok(base)
        }
        W::HostName | W::HostNameW => {
            if !image.winsock.initialized() {
                return failure(memory, state, 10093, u32::MAX.into(), &[]);
            }
            let wide = id == W::HostNameW;
            let bytes = string_bytes(local, wide);
            if a[0] == 0 || (a[1] as u32 as i32) < (bytes.len() / if wide { 2 } else { 1 }) as i32 {
                return failure(memory, state, 10014, u32::MAX.into(), &[]);
            }
            memory.write(a[0], &bytes)?;
            Ok(0)
        }
        W::HostByName | W::HostByAddress => {
            if !image.winsock.initialized() {
                return failure(memory, state, 10093, 0, &[]);
            }
            let answer = if id == W::HostByName {
                let name = if a[0] == 0 {
                    local.to_owned()
                } else {
                    text(memory, a[0], false)?
                };
                ipv4_name(&name, local)
            } else {
                if a[2] as u32 != 2 {
                    return failure(memory, state, 10047, 0, &[]);
                }
                if a[0] == 0 || a[1] as u32 != 4 {
                    return failure(memory, state, 10014, 0, &[]);
                }
                let mut ip = [0; 4];
                memory.read(a[0], &mut ip)?;
                (ip == [127, 0, 0, 1]).then(|| (ip, "localhost".to_owned()))
            };
            match answer {
                Some((ip, name)) => host_record(memory, image, state, ip, &name),
                None => failure(memory, state, 11001, 0, &[]),
            }
        }
        W::AddrInfoA | W::AddrInfoW | W::AddrInfoExW => addr_info(id, a, state, memory, image),
        W::FreeAddrInfoA | W::FreeAddrInfoW | W::FreeAddrInfoExW => {
            if a[0] == 0 {
                return Ok(0);
            }
            let expected = id == W::FreeAddrInfoExW;
            if image.winsock.results.get(&a[0]) != Some(&expected) {
                return Err(ServiceError::Unavailable("unowned Winsock address result"));
            }
            image.data_pages.free(memory, a[0])?;
            image.winsock.results.remove(&a[0]);
            Ok(0)
        }
        W::CancelAddrInfo | W::AddrInfoResult => failure(memory, state, 6, 6, &[]),
        W::NameInfoA | W::NameInfoW => name_info(id, a, state, memory, image),
        _ => unreachable!("Winsock name adapter admission"),
    }
}

struct Address {
    ip: IpAddr,
    port: u16,
    flags: u32,
    kind: u32,
    protocol: u32,
    name: Option<String>,
}
fn resolve(
    node: Option<&str>,
    service: Option<&str>,
    flags: u32,
    family: u32,
    mut kind: u32,
    mut protocol: u32,
    local: &str,
) -> Result<Address, u32> {
    if flags & !15 != 0 {
        return Err(10022);
    }
    if !matches!(family, 0 | 2 | 23) {
        return Err(10047);
    }
    if !matches!(kind, 0..=2) {
        return Err(10044);
    }
    if !matches!(protocol, 0 | 6 | 17) {
        return Err(10043);
    }
    if kind == 1 && protocol == 17 || kind == 2 && protocol == 6 {
        return Err(10022);
    }
    if kind == 0 {
        kind = match protocol {
            6 => 1,
            17 => 2,
            _ => 0,
        };
    }
    if protocol == 0 {
        protocol = match kind {
            1 => 6,
            2 => 17,
            _ => 0,
        };
    }
    if node.is_none() && service.is_none() {
        return Err(11001);
    }
    let port = match service {
        None | Some("") => 0,
        Some(s) if s.bytes().all(|c| c.is_ascii_digit()) => {
            s.parse::<u16>().map_err(|_| 10109u32)?
        }
        _ => return Err(if flags & 8 != 0 { 11001 } else { 10109 }),
    };
    let (ip, name) = match node {
        None => {
            if flags & 2 != 0 {
                return Err(10022);
            }
            let any = flags & 1 != 0;
            (
                if family == 23 {
                    IpAddr::V6(if any {
                        Ipv6Addr::UNSPECIFIED
                    } else {
                        Ipv6Addr::LOCALHOST
                    })
                } else {
                    IpAddr::V4(if any {
                        Ipv4Addr::UNSPECIFIED
                    } else {
                        Ipv4Addr::LOCALHOST
                    })
                },
                None,
            )
        }
        Some(s) => {
            if let Ok(ip) = s.parse::<IpAddr>() {
                (ip, Some(ip.to_string()))
            } else if flags & 4 == 0
                && (s.is_empty()
                    || s.eq_ignore_ascii_case(local)
                    || s.eq_ignore_ascii_case("localhost"))
            {
                let ip = if family == 23 {
                    IpAddr::V6(Ipv6Addr::LOCALHOST)
                } else {
                    IpAddr::V4(Ipv4Addr::LOCALHOST)
                };
                (
                    ip,
                    Some(if s.eq_ignore_ascii_case("localhost") {
                        "localhost".into()
                    } else {
                        local.into()
                    }),
                )
            } else {
                return Err(11001);
            }
        }
    };
    if family == 2 && !ip.is_ipv4() || family == 23 && !ip.is_ipv6() {
        return Err(10047);
    }
    Ok(Address {
        ip,
        port,
        flags,
        kind,
        protocol,
        name: if flags & 2 != 0 { name } else { None },
    })
}
fn addr_info(
    id: W,
    a: &[u64; 16],
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let ex = id == W::AddrInfoExW;
    let wide = id != W::AddrInfoA;
    let width = memory.mode() as usize / 8;
    let out = a[if ex { 5 } else { 3 }];
    let outputs = [out, if ex { a[9] } else { 0 }];
    let fail = |memory: &mut Memory, code| failure(memory, state, code, code.into(), &outputs);
    if !image.winsock.initialized() {
        return fail(memory, 10093);
    }
    if out == 0 {
        return fail(memory, 10014);
    }
    for &p in &outputs {
        if p != 0 {
            memory.check(p, width, WRITE)?;
        }
    }
    if outputs[1] != 0 && out.abs_diff(outputs[1]) < width as u64 {
        return fail(memory, 10022);
    }
    if ex {
        if !matches!(a[2] as u32, 0 | 12) {
            return fail(memory, 10108);
        }
        if a[3] != 0 || a[6] != 0 || a[7] != 0 || a[8] != 0 {
            return fail(memory, 10045);
        }
    }
    let node = if a[0] == 0 {
        None
    } else {
        Some(text(memory, a[0], wide)?)
    };
    let service = if a[1] == 0 {
        None
    } else {
        Some(text(memory, a[1], wide)?)
    };
    let hints = a[if ex { 4 } else { 2 }];
    let mut header = vec![
        0;
        if ex {
            if width == 4 {
                44
            } else {
                72
            }
        } else if width == 4 {
            32
        } else {
            48
        }
    ];
    if hints != 0 {
        memory.read(hints, &mut header)?;
    }
    if header[16..].iter().any(|b| *b != 0) {
        return fail(memory, 11003);
    }
    let fields: Vec<_> = header[..16]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let address = match resolve(
        node.as_deref(),
        service.as_deref(),
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        image.environment.profile.dns_hostname(),
    ) {
        Ok(address) => address,
        Err(code) => return fail(memory, code),
    };
    if image.winsock.results.len() >= 1024 {
        return fail(memory, 8);
    }
    let base = image.data_pages.allocate(memory, 512);
    if base == 0 {
        return fail(memory, 8);
    }
    let mut bytes = [0; 512];
    let (family, length) = match address.ip {
        IpAddr::V4(ip) => {
            bytes[132..136].copy_from_slice(&ip.octets());
            (2, 16)
        }
        IpAddr::V6(ip) => {
            bytes[136..152].copy_from_slice(&ip.octets());
            (23, 28)
        }
    };
    put(&mut bytes, 0, 4, address.flags.into());
    put(&mut bytes, 4, 4, family);
    put(&mut bytes, 8, 4, address.kind.into());
    put(&mut bytes, 12, 4, address.protocol.into());
    put(&mut bytes, 16, width, length);
    put(&mut bytes, 16 + width * 2, width, base + 128);
    put(&mut bytes, 128, 2, family);
    bytes[130..132].copy_from_slice(&address.port.to_be_bytes());
    if let Some(name) = address.name {
        let name = string_bytes(&name, wide);
        bytes[192..192 + name.len()].copy_from_slice(&name);
        put(&mut bytes, 16 + width, width, base + 192);
    }
    memory
        .write(base, &bytes)
        .expect("fresh Winsock address result");
    image.winsock.results.insert(base, ex);
    memory.write_int(out, width, base)?;
    if outputs[1] != 0 {
        memory.write_int(outputs[1], width, 0)?;
    }
    Ok(0)
}
fn name_info(
    id: W,
    a: &[u64; 16],
    state: &State,
    memory: &mut Memory,
    image: &Loaded,
) -> Result<u64, ServiceError> {
    let fail = |memory: &mut Memory, code| failure(memory, state, code, code.into(), &[]);
    if !image.winsock.initialized() {
        return fail(memory, 10093);
    }
    let flags = a[6] as u32;
    if flags & !31 != 0 || a[2] == 0 && a[4] == 0 {
        return fail(memory, 10022);
    }
    if a[0] == 0 || (a[1] as u32 as i32) < 2 {
        return fail(memory, 10014);
    }
    let family = memory.read_int(a[0], 2)?;
    let length = match family {
        2 => 16,
        23 => 28,
        _ => return fail(memory, 10047),
    };
    if (a[1] as u32 as i32) < length {
        return fail(memory, 10014);
    }
    let mut bytes = vec![0; length as usize];
    memory.read(a[0], &mut bytes)?;
    let ip = if family == 2 {
        IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[4..8]).unwrap()))
    } else {
        IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&bytes[8..24]).unwrap()))
    };
    let mut host = if ip.is_loopback() && flags & 2 == 0 {
        "localhost".to_owned()
    } else if flags & 4 != 0 && flags & 2 == 0 {
        return fail(memory, 11001);
    } else {
        ip.to_string()
    };
    if family == 23 {
        let scope = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        if scope != 0 && host != "localhost" {
            host.push_str(&format!("%{scope}"));
        }
    }
    let service = u16::from_be_bytes(bytes[2..4].try_into().unwrap()).to_string();
    let wide = id == W::NameInfoW;
    let host = string_bytes(&host, wide);
    let service = string_bytes(&service, wide);
    let unit = if wide { 2 } else { 1 };
    for (pointer, capacity, data) in [
        (a[2], a[3] as u32 as usize, &host),
        (a[4], a[5] as u32 as usize, &service),
    ] {
        if pointer != 0 {
            if capacity < data.len() / unit {
                return fail(memory, 10014);
            }
            memory.check(pointer, data.len(), WRITE)?;
        }
    }
    if a[2] != 0 {
        memory.write(a[2], &host)?;
    }
    if a[4] != 0 {
        memory.write(a[4], &service)?;
    }
    Ok(0)
}
