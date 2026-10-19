//! Local WMI namespace COM objects and the emulated Win32_OperatingSystem.
//! Other providers fail explicitly; no host hardware inventory is exposed.
//! COM ABI from Hangover Wine wbemcli.idl (LGPL-2.1-or-later), Hans Leidekker
//! for CodeWeavers, 2009. See third_party/hangover/README.md.
use crate::{
    api::{ApiState, ApiTable, Args},
    heap::GuestHeap,
    mem::GuestMem,
};
use std::collections::HashMap;

mod calls;
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
        heap: &mut GuestHeap,
        process_heap: u32,
    ) -> Result<u32, u32> {
        let table = if let Some(&p) = self.tables.get(kind) {
            p
        } else {
            let entries = methods(kind).unwrap();
            let p = heap
                .alloc(mem, process_heap, entries.len() as u32 * 4, false)
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
        let p = heap.alloc(mem, process_heap, 4, false).map_err(|_| NOMEM)?;
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
    Some(state.wmi.call(
        calls::Request { dll, name, args: a },
        mem,
        api,
        &mut state.heap,
        state.process_heap,
        &mut state.bstrs,
    ))
}
