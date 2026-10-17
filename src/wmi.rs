//! Local WMI namespace COM objects and the emulated Win32_OperatingSystem.
//! Other providers fail explicitly; no host hardware inventory is exposed.
//! COM ABI from Hangover Wine wbemcli.idl (LGPL-2.1-or-later), Hans Leidekker
//! for CodeWeavers, 2009. See third_party/hangover/README.md.
use crate::{
    api::{ApiState, ApiTable, Args},
    mem::GuestMem,
};
use std::collections::HashMap;
const POINTER: u32 = 0x80004003;
const NOINTERFACE: u32 = 0x80004002;
const NOTIMPL: u32 = 0x80004001;
const NOMEM: u32 = 0x8007000e;
const LOCATOR: &[(&str, u32)] = &[
    ("QueryInterface", 12),
    ("AddRef", 4),
    ("Release", 4),
    ("ConnectServer", 36),
];
const SERVICES: &[(&str, u32)] = &[
    ("QueryInterface", 12),
    ("AddRef", 4),
    ("Release", 4),
    ("OpenNamespace", 24),
    ("CancelAsyncCall", 8),
    ("QueryObjectSink", 12),
    ("GetObject", 24),
    ("GetObjectAsync", 20),
    ("PutClass", 20),
    ("PutClassAsync", 20),
    ("DeleteClass", 20),
    ("DeleteClassAsync", 20),
    ("CreateClassEnum", 20),
    ("CreateClassEnumAsync", 20),
    ("PutInstance", 20),
    ("PutInstanceAsync", 20),
    ("DeleteInstance", 20),
    ("DeleteInstanceAsync", 20),
    ("CreateInstanceEnum", 20),
    ("CreateInstanceEnumAsync", 20),
    ("ExecQuery", 24),
    ("ExecQueryAsync", 24),
    ("ExecNotificationQuery", 24),
    ("ExecNotificationQueryAsync", 24),
    ("ExecMethod", 32),
    ("ExecMethodAsync", 28),
];
const ENUM: &[(&str, u32)] = &[
    ("QueryInterface", 12),
    ("AddRef", 4),
    ("Release", 4),
    ("Reset", 4),
    ("Next", 20),
    ("NextAsync", 12),
    ("Clone", 8),
    ("Skip", 12),
];
const CLASS: &[(&str, u32)] = &[
    ("QueryInterface", 12),
    ("AddRef", 4),
    ("Release", 4),
    ("GetQualifierSet", 8),
    ("Get", 24),
    ("Put", 20),
    ("Delete", 8),
    ("GetNames", 20),
    ("BeginEnumeration", 8),
    ("Next", 24),
    ("EndEnumeration", 4),
    ("GetPropertyQualifierSet", 12),
    ("Clone", 8),
    ("GetObjectText", 12),
    ("SpawnDerivedClass", 12),
    ("SpawnInstance", 12),
    ("CompareTo", 12),
    ("GetPropertyOrigin", 12),
    ("InheritsFrom", 8),
    ("GetMethod", 20),
    ("PutMethod", 20),
    ("DeleteMethod", 8),
    ("BeginMethodEnumeration", 8),
    ("NextMethod", 20),
    ("EndMethodEnumeration", 4),
    ("GetMethodQualifierSet", 12),
    ("GetMethodOrigin", 12),
];
fn methods(kind: &str) -> Option<&'static [(&'static str, u32)]> {
    match kind {
        "IWbemLocator" => Some(LOCATOR),
        "IWbemServices" => Some(SERVICES),
        "IEnumWbemClassObject" => Some(ENUM),
        "IWbemClassObject" => Some(CLASS),
        _ => None,
    }
}
pub(crate) fn stack_bytes(name: &str) -> Option<u32> {
    let (kind, method) = name.split_once('.')?;
    methods(kind)?
        .iter()
        .find_map(|&(n, b)| (n == method).then_some(b))
}
fn guid(mem: &GuestMem, p: u32, fields: [u32; 4]) -> bool {
    let bytes = fields
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut b = [0; 16];
    mem.read_bytes(p, &mut b).is_ok() && b == bytes.as_slice()
}
fn iid(mem: &GuestMem, p: u32, kind: &str) -> bool {
    guid(mem, p, [0, 0, 0xc0, 0x46000000])
        || match kind {
            "IWbemLocator" => guid(mem, p, [0xdc12a687, 0x11cf737f, 0xaa004d88, 0x242e4b00]),
            "IWbemServices" => guid(mem, p, [0x9556dc99, 0x11cf828c, 0xaa007ea3, 0xc7403200]),
            "IEnumWbemClassObject" => {
                guid(mem, p, [0x027947e1, 0x11ced731, 0x000057a3, 0x01000000])
            }
            "IWbemClassObject" => guid(mem, p, [0xdc12a681, 0x11cf737f, 0xaa004d88, 0x242e4b00]),
            _ => false,
        }
}
struct Object {
    kind: &'static str,
    refs: u32,
    cursor: u32,
}
#[derive(Default)]
pub(crate) struct Wmi {
    objects: HashMap<u32, Object>,
    tables: HashMap<&'static str, u32>,
}
impl Wmi {
    fn create(
        &mut self,
        kind: &'static str,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        let table = if let Some(&p) = self.tables.get(kind) {
            p
        } else {
            let entries = methods(kind).unwrap();
            let p = state
                .heap
                .alloc(mem, state.process_heap, entries.len() as u32 * 4, false)
                .map_err(|_| NOMEM)?;
            if p == 0 {
                return Err(NOMEM);
            }
            for (i, (name, _)) in entries.iter().enumerate() {
                let stub = api.register("wire.wmi", Some(&format!("{kind}.{name}")), None);
                mem.write_u32(p + i as u32 * 4, stub).map_err(|_| POINTER)?;
            }
            self.tables.insert(kind, p);
            p
        };
        let p = state
            .heap
            .alloc(mem, state.process_heap, 4, false)
            .map_err(|_| NOMEM)?;
        if p == 0 {
            return Err(NOMEM);
        }
        mem.write_u32(p, table).map_err(|_| POINTER)?;
        self.objects.insert(
            p,
            Object {
                kind,
                refs: 1,
                cursor: 0,
            },
        );
        Ok(p)
    }
}
pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &Args,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
) -> Option<(u32, String, bool)> {
    if dll != "wire.wmi" && !(dll == "ole32.dll" && name == "CoCreateInstance") {
        return None;
    }
    let mut wmi = std::mem::take(&mut state.wmi);
    let mut note = String::new();
    let mut supported = true;
    let result = (|| -> Result<u32, u32> {
        if dll == "ole32.dll" {
            mem.write_u32(a[4], 0).map_err(|_| POINTER)?;
            if a[1] != 0 {
                return Err(0x80040110);
            }
            if !guid(mem, a[0], [0x4590f811, 0x11d01d3a, 0xaa001f89, 0x242e4b00]) {
                supported = false;
                return Err(0x80040154);
            }
            if a[2] & 3 == 0 {
                return Err(0x80040154);
            }
            if !iid(mem, a[3], "IWbemLocator") {
                return Err(NOINTERFACE);
            }
            let p = wmi.create("IWbemLocator", mem, api, state)?;
            mem.write_u32(a[4], p).map_err(|_| POINTER)?;
            return Ok(0);
        }
        let (kind, method) = name.split_once('.').ok_or(NOTIMPL)?;
        let object = wmi
            .objects
            .get_mut(&a[0])
            .filter(|o| o.kind == kind)
            .ok_or(POINTER)?;
        match method {
            "QueryInterface" => {
                mem.write_u32(a[2], 0).map_err(|_| POINTER)?;
                if !iid(mem, a[1], kind) {
                    return Err(NOINTERFACE);
                }
                object.refs = object.refs.checked_add(1).ok_or(NOMEM)?;
                mem.write_u32(a[2], a[0]).map_err(|_| POINTER)?;
                Ok(0)
            }
            "AddRef" => {
                object.refs = object.refs.checked_add(1).ok_or(NOMEM)?;
                Ok(object.refs)
            }
            "Release" => {
                object.refs -= 1;
                let refs = object.refs;
                if refs == 0 {
                    wmi.objects.remove(&a[0]);
                    state.heap.free(state.process_heap, a[0]);
                }
                Ok(refs)
            }
            "ConnectServer" => {
                mem.write_u32(a[8], 0).map_err(|_| POINTER)?;
                let ns = mem
                    .read_wstr(a[1], 1024)
                    .map_err(|_| POINTER)?
                    .to_ascii_lowercase();
                let ns = ns.strip_prefix("\\\\.\\").unwrap_or(&ns);
                if !matches!(ns, "root" | "root\\cimv2") {
                    return Err(0x8004100e);
                }
                if a[2] != 0 || a[3] != 0 || a[6] != 0 || a[7] != 0 {
                    return Err(0x8004100c);
                }
                let p = wmi.create("IWbemServices", mem, api, state)?;
                mem.write_u32(a[8], p).map_err(|_| POINTER)?;
                Ok(0)
            }
            "ExecQuery" | "ExecNotificationQuery" => {
                mem.write_u32(a[5], 0).map_err(|_| POINTER)?;
                let lang = mem.read_wstr(a[1], 16).map_err(|_| POINTER)?;
                let query = mem.read_wstr(a[2], 4096).map_err(|_| POINTER)?;
                note = format!("WMI {lang}: {query}");
                if method == "ExecQuery"
                    && lang.eq_ignore_ascii_case("WQL")
                    && query
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .eq_ignore_ascii_case("SELECT * FROM Win32_OperatingSystem")
                {
                    if a[4] != 0 || a[3] & !0x30 != 0 {
                        return Err(0x80041008);
                    }
                    let p = wmi.create("IEnumWbemClassObject", mem, api, state)?;
                    mem.write_u32(a[5], p).map_err(|_| POINTER)?;
                    return Ok(0);
                }
                note.push_str("; provider unavailable");
                supported = false;
                Err(if lang.eq_ignore_ascii_case("WQL") {
                    0x80041011
                } else {
                    0x80041014
                })
            }
            "Reset" if kind == "IEnumWbemClassObject" => {
                object.cursor = 0;
                Ok(0)
            }
            "Skip" if kind == "IEnumWbemClassObject" => {
                let remaining = 1 - object.cursor;
                object.cursor = (object.cursor.saturating_add(a[2])).min(1);
                Ok(if a[2] <= remaining { 0 } else { 1 })
            }
            "Clone" if kind == "IEnumWbemClassObject" => {
                mem.write_u32(a[1], 0).map_err(|_| POINTER)?;
                let cursor = object.cursor;
                let p = wmi.create("IEnumWbemClassObject", mem, api, state)?;
                wmi.objects.get_mut(&p).unwrap().cursor = cursor;
                mem.write_u32(a[1], p).map_err(|_| POINTER)?;
                Ok(0)
            }
            "Next" if kind == "IEnumWbemClassObject" => {
                mem.write_u32(a[4], 0).map_err(|_| POINTER)?;
                if a[2] == 0 {
                    return Ok(0);
                }
                if a[2] > 4096 {
                    return Err(0x80041008);
                }
                mem.ptr(a[3], a[2] * 4, true).map_err(|_| POINTER)?;
                if object.cursor == 1 {
                    return Ok(1);
                }
                let p = wmi.create("IWbemClassObject", mem, api, state)?;
                mem.write_u32(a[3], p).map_err(|_| POINTER)?;
                mem.write_u32(a[4], 1).map_err(|_| POINTER)?;
                wmi.objects.get_mut(&a[0]).unwrap().cursor = 1;
                Ok(if a[2] == 1 { 0 } else { 1 })
            }
            "Get" if kind == "IWbemClassObject" => {
                mem.ptr(a[3], 16, true).map_err(|_| POINTER)?;
                if a[4] != 0 {
                    mem.ptr(a[4], 4, true).map_err(|_| POINTER)?;
                }
                if a[5] != 0 {
                    mem.ptr(a[5], 4, true).map_err(|_| POINTER)?;
                }
                let key = mem
                    .read_wstr(a[1], 256)
                    .map_err(|_| POINTER)?
                    .to_ascii_lowercase();
                let text = match key.as_str() {
                    "__class" => "Win32_OperatingSystem",
                    "caption" => "Microsoft Windows 7",
                    "csdversion" => "",
                    "osarchitecture" => "32-bit",
                    "version" => "6.1.7601",
                    "buildnumber" => "7601",
                    _ => return Err(0x80041002),
                };
                if a[2] != 0 {
                    return Err(0x80041008);
                }
                let p = state.bstrs.allocate(
                    mem,
                    &mut state.heap,
                    state.process_heap,
                    &text.encode_utf16().collect::<Vec<_>>(),
                )?;
                let mut variant = [0; 16];
                variant[0] = 8;
                variant[8..12].copy_from_slice(&p.to_le_bytes());
                mem.write_bytes(a[3], &variant).map_err(|_| POINTER)?;
                if a[4] != 0 {
                    mem.write_u32(a[4], 8).map_err(|_| POINTER)?;
                }
                if a[5] != 0 {
                    mem.write_u32(a[5], 0).map_err(|_| POINTER)?;
                }
                Ok(0)
            }
            _ => {
                supported = false;
                Err(NOTIMPL)
            }
        }
    })();
    state.wmi = wmi;
    Some((
        result.unwrap_or_else(|e| e),
        if note.is_empty() {
            "local WMI COM contract".into()
        } else {
            note
        },
        supported,
    ))
}
