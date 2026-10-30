//! Registry calls use shared logical state and publish checked outputs atomically.
use super::{
    call::Call,
    catalog::RegistryApi as R,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    machine::Mode,
    memory::{Memory, WRITE},
    process::Loaded,
};
#[path = "../../../../src/win32/registry_state.rs"]
mod state;
pub(crate) type Registry = state::Registry;

pub(crate) fn new(mode: Mode) -> Registry {
    let start = if mode == Mode::X86 {
        super::tokens::START_X86 + 0x10000
    } else {
        0x7ffd04000000
    };
    Registry::new(
        start,
        start + 0x10000,
        Some(state::Budget {
            nodes: 4096,
            values: 4096,
            bytes: 16 * 1024 * 1024,
        }),
    )
}
fn status(code: u32) -> Failure {
    Failure::Windows(code)
}
fn optional_name(memory: &Memory, pointer: u64, wide: bool) -> Result<String, Failure> {
    if pointer == 0 {
        Ok(String::new())
    } else {
        name(memory, pointer, wide)
    }
}
fn publish(memory: &mut Memory, outputs: &[(u64, Vec<u8>)]) -> Result<(), Failure> {
    for (address, bytes) in outputs {
        memory.check(*address, bytes.len(), WRITE)?;
    }
    for (address, bytes) in outputs {
        memory
            .write(*address, bytes)
            .expect("preflighted registry output");
    }
    Ok(())
}
fn dword(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}
fn rights(root: u32, mut mask: u32) -> Result<u32, Failure> {
    let generic = mask >> 28;
    mask &= 0x0fffffff;
    if generic & (8 | 2) != 0 {
        mask |= 0x20019;
    }
    if generic & 4 != 0 {
        mask |= 0x20006;
    }
    if generic & 1 != 0 {
        mask |= 0xf003f;
    }
    if mask & 0x02000000 != 0 {
        mask = (mask & !0x02000000) | if root == 1 { 0xf003f } else { 0x20019 };
    }
    if mask & !0x1f033f != 0 || mask & 0x300 == 0x300 {
        return Err(status(87));
    }
    if mask & 0x200 != 0 {
        return Err(status(50));
    }
    Ok(mask & !0x100)
}
fn open(
    id: R,
    a: &[u64; 16],
    memory: &mut Memory,
    registry: &mut Registry,
) -> Result<u64, Failure> {
    let create = matches!(id, R::CreateA | R::CreateW | R::CreateExA | R::CreateExW);
    let extended = matches!(id, R::CreateExA | R::CreateExW | R::OpenExA | R::OpenExW);
    let output = a[if extended {
        if create {
            7
        } else {
            4
        }
    } else {
        2
    }];
    let disposition = if create && extended { a[8] } else { 0 };
    if output == 0 {
        return Err(status(if create && extended { 1010 } else { 87 }));
    }
    let result = (|| {
        let (base, access) = registry.resolve(a[0]).map_err(status)?;
        let mut child = optional_name(memory, a[1], id.wide())?;
        if !create && child.is_empty() && (!extended || state::root(a[0]).is_some()) {
            return Ok((a[0], None));
        }
        if !create && base.0 == 0 {
            child = child.strip_prefix('\\').unwrap_or(&child).to_owned();
        }
        if extended {
            if a[2] as u32 != 0 {
                return Err(status(if !create && a[2] as u32 == 8 { 50 } else { 87 }));
            }
            if create {
                if a[4] as u32 > 1 {
                    return Err(status(if a[4] as u32 & !15 == 0 { 50 } else { 87 }));
                }
                if a[3] != 0 || a[6] != 0 {
                    return Err(status(50));
                }
            }
        }
        let mask = if extended {
            a[if create { 5 } else { 3 }] as u32
        } else {
            0x02000000
        };
        let access_mask = rights(base.0, mask)?;
        let job = registry
            .prepare_open(&base, access, &child, create, access_mask)
            .map_err(status)?;
        Ok((job.handle, Some(job)))
    })();
    let (handle, job) = match result {
        Ok(answer) => answer,
        Err(Failure::Windows(code)) => {
            if !create {
                publish(memory, &[(output, vec![0; memory.mode() as usize / 8])])?;
            }
            return Ok(code.into());
        }
        Err(error) => return Err(error),
    };
    let mut outputs = vec![(
        output,
        handle.to_le_bytes()[..memory.mode() as usize / 8].to_vec(),
    )];
    if disposition != 0 {
        outputs.push((
            disposition,
            dword(if job.as_ref().is_some_and(|job| job.created) {
                1
            } else {
                2
            }),
        ));
    }
    publish(memory, &outputs)?;
    if let Some(job) = job {
        registry.commit_open(job);
    }
    Ok(0)
}
fn is_string(kind: u32) -> bool {
    matches!(kind, 1 | 2 | 7)
}
fn set(
    a: &[u64; 16],
    wide: bool,
    memory: &Memory,
    registry: &mut Registry,
    key: state::Key,
    access: u32,
) -> Result<u64, Failure> {
    if access & 2 == 0 {
        return Err(status(5));
    }
    if a[2] as u32 != 0 || a[5] as u32 > 1024 * 1024 {
        return Err(status(87));
    }
    let value = optional_name(memory, a[1], wide)?;
    if value.len() > 16384 || value.to_lowercase().len() > 16384 {
        return Err(status(87));
    }
    let count = a[5] as u32 as usize;
    if a[4] == 0 && count != 0 {
        return Err(status(998));
    }
    let mut data = vec![0; count];
    memory.read(a[4], &mut data)?;
    let kind = a[3] as u32;
    if is_string(kind) {
        let width = if wide { 2 } else { 1 };
        if count >= width
            && count.is_multiple_of(width)
            && data[count - width..].iter().any(|b| *b != 0)
        {
            let after = a[4].checked_add(count as u64).ok_or(status(87))?;
            if memory.read_int(after, width)? == 0 {
                data.resize(count + width, 0);
            }
        }
        if !wide {
            data = crate::codepage::decode_lossy(&data, 932)
                .map_err(status)?
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
        }
    }
    if data.len() > 1024 * 1024 {
        return Err(status(8));
    }
    registry
        .check_value(&key, &value, data.len())
        .map_err(status)?;
    registry.set_value(&key, value, kind, data);
    Ok(0)
}
fn encoded_name(name: &str, wide: bool) -> Result<Vec<u8>, Failure> {
    if wide {
        Ok(name.encode_utf16().flat_map(u16::to_le_bytes).collect())
    } else {
        Ok(crate::codepage::encode(name, 932).map_err(status)?.0)
    }
}
fn value_bytes(value: &state::Value, wide: bool) -> Result<Vec<u8>, Failure> {
    if !wide && is_string(value.kind) {
        let units: Vec<_> = value
            .data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        encoded_name(&String::from_utf16_lossy(&units), false)
    } else {
        Ok(value.data.clone())
    }
}
fn query(
    id: R,
    a: &[u64; 16],
    memory: &mut Memory,
    registry: &Registry,
    mut key: state::Key,
    access: u32,
) -> Result<u64, Failure> {
    let legacy = matches!(id, R::QueryA | R::QueryW);
    let wide = id.wide();
    let (type_out, data_out, length_out) = if legacy {
        (0, a[2], a[3])
    } else {
        (a[3], a[4], a[5])
    };
    if data_out != 0 && length_out == 0 || !legacy && a[2] != 0 {
        return Err(status(87));
    }
    if access & 1 == 0 {
        return Err(status(5));
    }
    let name = optional_name(memory, a[1], wide)?;
    if name.len() > 16384 {
        return Err(status(87));
    }
    if legacy && !name.is_empty() {
        key = registry.key(&key, &name).map_err(status)?;
        if !registry.nodes.contains_key(&key) {
            return Err(status(2));
        }
    }
    let normalized = name.to_lowercase();
    let value = registry.nodes[&key]
        .values
        .get(if legacy { "" } else { &normalized });
    let empty = legacy && value.is_none();
    let (kind, mut bytes) = match value {
        Some(value) => (value.kind, value_bytes(value, wide)?),
        None if legacy => (1, vec![0; if wide { 2 } else { 1 }]),
        None => {
            if data_out == 0 && length_out != 0 {
                publish(memory, &[(length_out, dword(0))])?;
            }
            return Err(status(2));
        }
    };
    let capacity = if data_out == 0 {
        0
    } else {
        memory.read_int(length_out, 4)? as usize
    };
    let required = bytes.len();
    let enough = capacity >= required || empty;
    let mut outputs = Vec::new();
    if type_out != 0 {
        outputs.push((type_out, dword(kind)));
    }
    if length_out != 0 {
        outputs.push((length_out, dword(required as u32)));
    }
    if data_out != 0 && enough {
        let width = if wide { 2 } else { 1 };
        if is_string(kind)
            && required >= width
            && required.is_multiple_of(width)
            && capacity >= required + width
            && bytes[required - width..].iter().any(|b| *b != 0)
        {
            bytes.resize(required + width, 0);
        }
        if !bytes.is_empty() {
            outputs.push((data_out, bytes));
        }
    }
    publish(memory, &outputs)?;
    Ok(if data_out != 0 && !enough { 234 } else { 0 })
}
fn enum_key(
    id: R,
    a: &[u64; 16],
    memory: &mut Memory,
    registry: &Registry,
    key: &state::Key,
    access: u32,
) -> Result<u64, Failure> {
    if access & 8 == 0 {
        return Err(status(5));
    }
    let extended = matches!(id, R::EnumKeyExA | R::EnumKeyExW);
    if a[2] == 0 || extended && (a[3] == 0 || a[4] != 0 || a[5] != 0 && a[6] == 0) {
        return Err(status(87));
    }
    if extended && a[7] != 0 {
        return Err(status(50));
    }
    let node = registry
        .children(key)
        .nth(a[1] as u32 as usize)
        .ok_or(status(259))?;
    let wide = id.wide();
    let width = if wide { 2 } else { 1 };
    let mut bytes = encoded_name(&node.name, wide)?;
    let required = bytes.len() / width;
    let capacity = if extended {
        memory.read_int(a[3], 4)?
    } else {
        a[3] as u32 as u64
    };
    let class_enough = !extended || a[5] == 0 || memory.read_int(a[6], 4)? != 0;
    let enough = capacity > required as u64 && class_enough;
    let mut outputs = Vec::new();
    if extended {
        outputs.push((a[3], dword(required as u32)));
        if a[6] != 0 {
            outputs.push((a[6], dword(0)));
        }
    }
    if enough {
        bytes.resize(bytes.len() + width, 0);
        outputs.push((a[2], bytes));
        if extended && a[5] != 0 {
            outputs.push((a[5], vec![0; width]));
        }
    }
    publish(memory, &outputs)?;
    Ok(if enough { 0 } else { 234 })
}
fn enum_value(
    a: &[u64; 16],
    wide: bool,
    memory: &mut Memory,
    registry: &Registry,
    key: &state::Key,
    access: u32,
) -> Result<u64, Failure> {
    if access & 1 == 0 {
        return Err(status(5));
    }
    if a[2] == 0 || a[3] == 0 || a[4] != 0 || a[6] != 0 && a[7] == 0 {
        return Err(status(87));
    }
    let value = registry.nodes[key]
        .values
        .values()
        .nth(a[1] as u32 as usize)
        .ok_or(status(259))?;
    let width = if wide { 2 } else { 1 };
    let mut name = encoded_name(&value.name, wide)?;
    let name_length = name.len() / width;
    let name_enough = memory.read_int(a[3], 4)? > name_length as u64;
    let bytes = value_bytes(value, wide)?;
    let data_enough = a[6] == 0 || memory.read_int(a[7], 4)? >= bytes.len() as u64;
    let mut outputs = vec![(a[3], dword(name_length as u32))];
    if a[5] != 0 {
        outputs.push((a[5], dword(value.kind)));
    }
    if a[7] != 0 {
        outputs.push((a[7], dword(bytes.len() as u32)));
    }
    if name_enough {
        name.resize(name.len() + width, 0);
        outputs.push((a[2], name));
        if a[6] != 0 && data_enough && !bytes.is_empty() {
            outputs.push((a[6], bytes));
        }
    }
    publish(memory, &outputs)?;
    Ok(if name_enough && data_enough { 0 } else { 234 })
}
fn query_info(
    a: &[u64; 16],
    wide: bool,
    memory: &mut Memory,
    registry: &Registry,
    key: &state::Key,
    access: u32,
) -> Result<u64, Failure> {
    if access & 1 == 0 {
        return Err(status(5));
    }
    if a[3] != 0 || a[1] != 0 && a[2] == 0 {
        return Err(status(87));
    }
    if a[10] != 0 || a[11] != 0 {
        return Err(status(50));
    }
    let width = if wide { 2 } else { 1 };
    let class_enough = a[1] == 0 || memory.read_int(a[2], 4)? != 0;
    let mut subkeys = 0;
    let mut max_key = 0;
    for node in registry.children(key) {
        subkeys += 1;
        max_key = max_key.max(encoded_name(&node.name, wide)?.len() / width);
    }
    let values = &registry.nodes[key].values;
    let mut max_name = 0;
    let mut max_data = 0;
    for value in values.values() {
        max_name = max_name.max(encoded_name(&value.name, wide)?.len() / width);
        max_data = max_data.max(value_bytes(value, wide)?.len());
    }
    let mut outputs = Vec::new();
    for (pointer, value) in [
        (a[2], 0),
        (a[4], subkeys),
        (a[5], max_key),
        (a[6], 0),
        (a[7], values.len()),
        (a[8], max_name),
        (a[9], max_data),
    ] {
        if pointer != 0 {
            outputs.push((pointer, dword(value as u32)));
        }
    }
    if a[1] != 0 && class_enough {
        outputs.push((a[1], vec![0; width]));
    }
    publish(memory, &outputs)?;
    Ok(if class_enough { 0 } else { 234 })
}
fn delete_key(
    id: R,
    a: &[u64; 16],
    memory: &Memory,
    registry: &mut Registry,
    base: &state::Key,
    access: u32,
) -> Result<u64, Failure> {
    let tree = matches!(
        id,
        R::DeleteTreeA | R::DeleteTreeW | R::ShellDeleteKeyA | R::ShellDeleteKeyW
    );
    if matches!(id, R::DeleteKeyExA | R::DeleteKeyExW) {
        let view = a[2] as u32;
        if a[3] as u32 != 0 || view & !0x300 != 0 || view == 0x300 {
            return Err(status(87));
        }
        if view == 0x200 {
            return Err(status(50));
        }
    }
    let child = optional_name(memory, a[1], id.wide())?;
    if !tree && (a[1] == 0 || child.is_empty()) {
        return Err(status(87));
    }
    if base.0 != 1 || tree && access & 0x10009 != 0x10009 {
        return Err(status(5));
    }
    let key = registry.key(base, &child).map_err(status)?;
    if tree && access & 2 == 0 {
        let prefix = if key.1.is_empty() {
            String::new()
        } else {
            format!("{}\\", key.1)
        };
        if registry.nodes.iter().any(|(other, node)| {
            other.0 == key.0
                && (other == &key || other.1.starts_with(&prefix))
                && !node.values.is_empty()
        }) {
            return Err(status(5));
        }
    }
    registry
        .delete_key(&key, tree, tree && child.is_empty())
        .map_err(status)?;
    Ok(0)
}
fn invoke(
    id: R,
    a: &[u64; 16],
    memory: &mut Memory,
    registry: &mut Registry,
) -> Result<u64, Failure> {
    if matches!(
        id,
        R::CreateA
            | R::CreateW
            | R::CreateExA
            | R::CreateExW
            | R::OpenA
            | R::OpenW
            | R::OpenExA
            | R::OpenExW
    ) {
        return open(id, a, memory, registry);
    }
    if id == R::Close {
        registry.close(a[0]).map_err(status)?;
        return Ok(0);
    }
    let (key, access) = registry.resolve(a[0]).map_err(status)?;
    match id {
        R::SetExA | R::SetExW => set(a, id.wide(), memory, registry, key, access),
        R::DeleteA | R::DeleteW => {
            if access & 2 == 0 {
                return Err(status(5));
            }
            let value = optional_name(memory, a[1], id.wide())?;
            if value.len() > 16384 {
                return Err(status(87));
            }
            Ok(
                if registry
                    .nodes
                    .get_mut(&key)
                    .unwrap()
                    .values
                    .remove(&value.to_lowercase())
                    .is_some()
                {
                    0
                } else {
                    2
                },
            )
        }
        R::QueryA | R::QueryW | R::QueryExA | R::QueryExW => {
            query(id, a, memory, registry, key, access)
        }
        R::EnumKeyA | R::EnumKeyW | R::EnumKeyExA | R::EnumKeyExW => {
            enum_key(id, a, memory, registry, &key, access)
        }
        R::EnumValueA | R::EnumValueW => enum_value(a, id.wide(), memory, registry, &key, access),
        R::QueryInfoA | R::QueryInfoW => query_info(a, id.wide(), memory, registry, &key, access),
        R::DeleteKeyA
        | R::DeleteKeyW
        | R::DeleteKeyExA
        | R::DeleteKeyExW
        | R::DeleteTreeA
        | R::DeleteTreeW
        | R::ShellDeleteKeyA
        | R::ShellDeleteKeyW => delete_key(id, a, memory, registry, &key, access),
        R::Flush => Err(status(if access & 1 == 0 { 5 } else { 50 })),
        _ => unreachable!("registry open returned above"),
    }
}
pub(super) fn dispatch(
    id: R,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let registry = &mut image
        .ok_or(ServiceError::Unavailable(
            "registry services require a loaded image",
        ))?
        .registry;
    match invoke(id, &call.arguments, memory, registry) {
        Ok(value) => Ok(value),
        Err(Failure::Windows(code)) => Ok(code.into()),
        Err(Failure::Fault(fault)) => Err(fault.into()),
    }
}
