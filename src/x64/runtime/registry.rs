//! Isolated, process-lifetime guest registry. No host registry or invented installations.
//! HKCU is writable; system roots are read-only. Persistence and WOW32 views are absent.
use super::{Memory, Runtime64, WRITE};
use std::collections::BTreeMap;
type Key = (u32, String);
#[derive(Default)]
struct Node {
    values: BTreeMap<String, (u32, Vec<u8>)>,
}
pub(super) struct Registry {
    nodes: BTreeMap<Key, Node>,
    handles: BTreeMap<u64, (Key, u32)>,
    next: u64,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            nodes: [0, 1, 2, 3, 5]
                .into_iter()
                .map(|root| ((root, String::new()), Node::default()))
                .collect(),
            handles: BTreeMap::new(),
            next: super::TEB + 0x500000,
        }
    }
}
fn root(handle: u64) -> Option<u32> {
    let value = handle as u32;
    (matches!(value, 0x80000000..=0x80000003 | 0x80000005)
        && (handle == value as u64 || handle == 0xffffffff00000000 | value as u64))
        .then_some(value - 0x80000000)
}
impl Registry {
    fn resolve(&self, handle: u64) -> Result<(Key, u32), u64> {
        if let Some(root) = root(handle) {
            Ok((
                (root, String::new()),
                if root == 1 { 0xf003f } else { 0x20019 },
            ))
        } else {
            self.handles.get(&handle).cloned().ok_or(6)
        }
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "RegCreateKeyA" | "RegCreateKeyW" | "RegOpenKeyA" | "RegOpenKeyW" => 3,
        "RegCreateKeyExA" | "RegCreateKeyExW" => 9,
        "RegOpenKeyExA" | "RegOpenKeyExW" => 5,
        "RegQueryValueExA" | "RegQueryValueExW" | "RegSetValueExA" | "RegSetValueExW" => 6,
        "RegDeleteValueA" | "RegDeleteValueW" => 2,
        "RegCloseKey" => 1,
        _ => return None,
    })
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "RegCreateKeyExA" | "RegCreateKeyExW" => &[2, 4, 5],
        "RegOpenKeyExA" | "RegOpenKeyExW" => &[2, 3],
        "RegSetValueExA" | "RegSetValueExW" => &[2, 3, 5],
        _ => &[],
    }
}
fn string_type(kind: u32) -> bool {
    matches!(kind, 1 | 2 | 7)
}
impl Runtime64 {
    pub(super) fn registry_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        // Registry APIs return LSTATUS directly and preserve thread LastError.
        let (base, access) = match self.registry.resolve(args[0]) {
            Ok(key) => key,
            Err(status) => return Ok(status),
        };
        let wide = name.ends_with('W');
        match name {
            "RegCloseKey" => {
                if root(args[0]).is_none() {
                    self.registry.handles.remove(&args[0]);
                }
                Ok(0)
            }
            name if name.starts_with("RegCreateKey") || name.starts_with("RegOpenKey") => {
                let create = name.starts_with("RegCreate");
                let extended = name.contains("Ex");
                let output = args[if extended {
                    if create {
                        7
                    } else {
                        4
                    }
                } else {
                    2
                }];
                let disposition = if extended && create { args[8] } else { 0 };
                if output == 0 {
                    return Ok(87);
                }
                let rights = if extended {
                    args[if create { 5 } else { 3 }] as u32
                } else {
                    if create {
                        0xf003f
                    } else {
                        access
                    }
                };
                if rights & !0x1f033f != 0 || rights & 0x300 == 0x300 {
                    return Ok(87);
                }
                if rights & 0x200 != 0 {
                    return Ok(50);
                }
                if extended {
                    if create && (args[2] != 0 || args[4] > 1) {
                        return Ok(87);
                    }
                    if create && (args[3] != 0 || args[6] != 0) {
                        return Ok(50);
                    }
                    if !create && args[2] != 0 {
                        return Ok(87);
                    }
                }
                let child = if args[1] == 0 {
                    String::new()
                } else {
                    Self::path_name(mem, args[1], wide)?.to_lowercase()
                };
                if child.starts_with('\\')
                    || child.ends_with('\\')
                    || child.contains("\\\\")
                    || child.len() > 4096
                {
                    return Ok(87);
                }
                let path = match (base.1.is_empty(), child.is_empty()) {
                    (_, true) => base.1.clone(),
                    (true, _) => child.clone(),
                    _ => format!("{}\\{child}", base.1),
                };
                let key = (base.0, path.clone());
                let exists = self.registry.nodes.contains_key(&key);
                if !create && !exists {
                    return Ok(2);
                }
                if create && !exists && (base.0 != 1 || access & 4 == 0) {
                    return Ok(5);
                }
                if base.0 != 1 && rights & (2 | 4 | 0x10000 | 0x40000 | 0x80000) != 0 {
                    return Ok(5);
                }
                if self.registry.handles.len() >= 4096
                    || self.registry.next >= super::TEB + 0x510000
                {
                    return Ok(8);
                }
                mem.check(output, 8, WRITE)?;
                if disposition != 0 {
                    mem.check(disposition, 4, WRITE)?;
                }
                // Commit key and handle only after every guest output is checked.
                if create && !exists {
                    let mut parent = String::new();
                    for component in path.split('\\') {
                        if !parent.is_empty() {
                            parent.push('\\');
                        }
                        parent.push_str(component);
                        self.registry
                            .nodes
                            .entry((base.0, parent.clone()))
                            .or_default();
                    }
                }
                let handle = self.registry.next;
                mem.write_int(output, 8, handle)?;
                if disposition != 0 {
                    mem.write_int(disposition, 4, if exists { 2 } else { 1 })?;
                }
                self.registry.handles.insert(handle, (key, rights & !0x100));
                self.registry.next += 16;
                Ok(0)
            }
            _ => {
                let value = if args[1] == 0 {
                    String::new()
                } else {
                    Self::path_name(mem, args[1], wide)?.to_lowercase()
                };
                if value.len() > 16384 {
                    return Ok(87);
                }
                if name.starts_with("RegSet") {
                    if access & 2 == 0 {
                        return Ok(5);
                    }
                    if args[2] != 0 || args[5] > 1024 * 1024 {
                        return Ok(87);
                    }
                    let mut data = vec![0; args[5] as usize];
                    mem.read(args[4], &mut data)?;
                    let kind = args[3] as u32;
                    if string_type(kind) {
                        if wide {
                            if !data.len().is_multiple_of(2) {
                                return Ok(87);
                            }
                        } else {
                            data = crate::codepage::decode(&data, 932)
                                .map_err(|e| format!("registry string: {e}"))?
                                .encode_utf16()
                                .flat_map(u16::to_le_bytes)
                                .collect();
                        }
                    }
                    self.registry
                        .nodes
                        .get_mut(&base)
                        .unwrap()
                        .values
                        .insert(value, (kind, data));
                    return Ok(0);
                }
                if name.starts_with("RegDelete") {
                    if access & 2 == 0 {
                        return Ok(5);
                    }
                    return Ok(
                        if self
                            .registry
                            .nodes
                            .get_mut(&base)
                            .unwrap()
                            .values
                            .remove(&value)
                            .is_some()
                        {
                            0
                        } else {
                            2
                        },
                    );
                }
                if access & 1 == 0 {
                    return Ok(5);
                }
                if args[2] != 0 || (args[4] != 0 && args[5] == 0) {
                    return Ok(87);
                }
                let Some((kind, stored)) = self.registry.nodes[&base].values.get(&value) else {
                    return Ok(2);
                };
                let bytes = if string_type(*kind) && !wide {
                    let units = stored
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect::<Vec<_>>();
                    let string =
                        String::from_utf16(&units).map_err(|_| "invalid UTF-16 registry data")?;
                    crate::codepage::encode(&string, 932)
                        .map_err(|e| format!("registry conversion: {e}"))?
                        .0
                } else {
                    stored.clone()
                };
                let capacity = if args[5] == 0 {
                    0
                } else {
                    mem.read_int(args[5], 4)? as usize
                };
                if args[3] != 0 {
                    mem.check(args[3], 4, WRITE)?;
                }
                if args[5] != 0 {
                    mem.check(args[5], 4, WRITE)?;
                }
                if args[4] != 0 && capacity >= bytes.len() {
                    mem.check(args[4], bytes.len(), WRITE)?;
                }
                if args[3] != 0 {
                    mem.write_int(args[3], 4, *kind as u64)?;
                }
                if args[5] != 0 {
                    mem.write_int(args[5], 4, bytes.len() as u64)?;
                }
                if args[4] != 0 {
                    if capacity < bytes.len() {
                        return Ok(234);
                    }
                    mem.write(args[4], &bytes)?;
                }
                Ok(0)
            }
        }
    }
}
