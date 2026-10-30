//! Local WMI namespace COM objects and the emulated Win32_OperatingSystem.
//! Other providers fail explicitly; no host hardware inventory is exposed.
//! COM ABI from Hangover Wine wbemcli.idl (LGPL-2.1-or-later), Hans Leidekker
//! for CodeWeavers, 2009. See third_party/hangover/README.md.
use std::collections::HashMap;

pub(crate) trait Backend {
    type Error: From<u32>;
    fn check(&self, p: u32, length: usize, write: bool) -> Result<(), Self::Error>;
    fn read(&self, p: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, p: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn allocate(&mut self, length: u32) -> Result<u32, Self::Error>;
    fn free(&mut self, p: u32) -> Result<(), Self::Error>;
    fn check_methods(&self, kind: &'static str) -> Result<(), Self::Error>;
    fn register(&mut self, method: Method) -> Result<u32, Self::Error>;
    fn bstr(&mut self, text: &[u16]) -> Result<u32, Self::Error>;
    fn put(&mut self, p: u32, value: u32) -> Result<(), Self::Error> {
        self.write(p, &value.to_le_bytes())
    }
    fn text(&self, p: u32, limit: u32) -> Result<String, Self::Error> {
        let mut units = Vec::new();
        for n in 0..limit {
            let at = p
                .checked_add(n * 2)
                .ok_or_else(|| Self::Error::from(POINTER))?;
            let mut bytes = [0; 2];
            self.read(at, &mut bytes)?;
            let unit = u16::from_le_bytes(bytes);
            if unit == 0 {
                return Ok(String::from_utf16_lossy(&units));
            }
            units.push(unit);
        }
        Err(POINTER.into())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Method {
    pub kind: &'static str,
    pub name: &'static str,
    pub stack: u32,
}
impl Method {
    pub(crate) fn lookup(name: &str) -> Option<Self> {
        let stack = stack_bytes(name)?;
        let (kind, name) = name.split_once('.')?;
        [
            "IWbemLocator",
            "IWbemServices",
            "IEnumWbemClassObject",
            "IWbemClassObject",
        ]
        .into_iter()
        .find(|&k| k == kind)
        .and_then(|kind| {
            methods(kind)?.iter().find_map(|&(n, _)| {
                (n == name).then_some(Self {
                    kind,
                    name: n,
                    stack,
                })
            })
        })
    }
}
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
pub(crate) fn methods(kind: &str) -> Option<&'static [(&'static str, u32)]> {
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
fn guid<B: Backend>(mem: &B, p: u32, fields: [u32; 4]) -> Result<bool, B::Error> {
    let bytes = fields
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut b = [0; 16];
    mem.read(p, &mut b)?;
    Ok(b == bytes.as_slice())
}
fn iid<B: Backend>(mem: &B, p: u32, kind: &str) -> Result<bool, B::Error> {
    if guid(mem, p, [0, 0, 0xc0, 0x46000000])? {
        return Ok(true);
    }
    let fields = match kind {
        "IWbemLocator" => [0xdc12a687, 0x11cf737f, 0xaa004d88, 0x242e4b00],
        "IWbemServices" => [0x9556dc99, 0x11cf828c, 0xaa007ea3, 0xc7403200],
        "IEnumWbemClassObject" => [0x027947e1, 0x11ced731, 0x000057a3, 0x01000000],
        "IWbemClassObject" => [0xdc12a681, 0x11cf737f, 0xaa004d88, 0x242e4b00],
        _ => return Ok(false),
    };
    guid(mem, p, fields)
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
    fn create<B: Backend>(&mut self, kind: &'static str, mem: &mut B) -> Result<u32, B::Error> {
        if self.objects.len() >= 4096 {
            return Err(NOMEM.into());
        }
        let table = if let Some(&p) = self.tables.get(kind) {
            p
        } else {
            let entries = methods(kind).unwrap();
            mem.check_methods(kind)?;
            let mut bytes = Vec::with_capacity(entries.len() * 4);
            for &(name, stack) in entries {
                bytes.extend_from_slice(&mem.register(Method { kind, name, stack })?.to_le_bytes());
            }
            let p = mem.allocate(bytes.len() as u32)?;
            if let Err(error) = mem.write(p, &bytes) {
                mem.free(p)?;
                return Err(error);
            }
            self.tables.insert(kind, p);
            p
        };
        let p = mem.allocate(4)?;
        if let Err(error) = mem.put(p, table) {
            mem.free(p)?;
            return Err(error);
        }
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
    pub(crate) fn activate<B: Backend>(&mut self, a: &[u32], mem: &mut B) -> Result<u32, B::Error> {
        mem.check(a[4], 4, true)?;
        let result = if a[1] != 0 {
            Err(0x80040110)
        } else if !guid(mem, a[0], [0x4590f811, 0x11d01d3a, 0xaa001f89, 0x242e4b00])?
            || a[2] & 3 == 0
        {
            Err(0x80040154)
        } else if !iid(mem, a[3], "IWbemLocator")? {
            Err(NOINTERFACE)
        } else {
            Ok(())
        };
        if let Err(code) = result {
            mem.put(a[4], 0)?;
            return Ok(code);
        }
        let p = self.create("IWbemLocator", mem)?;
        mem.put(a[4], p)?;
        Ok(0)
    }
    pub(crate) fn call<B: Backend>(
        &mut self,
        method: Method,
        a: &[u32],
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        let Method { kind, name, .. } = method;
        let object = self
            .objects
            .get_mut(&a[0])
            .filter(|o| o.kind == kind)
            .ok_or_else(|| B::Error::from(POINTER))?;
        match name {
            "QueryInterface" => {
                mem.check(a[2], 4, true)?;
                if !iid(mem, a[1], kind)? {
                    mem.put(a[2], 0)?;
                    return Ok(NOINTERFACE);
                }
                let refs = object
                    .refs
                    .checked_add(1)
                    .ok_or_else(|| B::Error::from(NOMEM))?;
                mem.put(a[2], a[0])?;
                object.refs = refs;
                Ok(0)
            }
            "AddRef" => {
                object.refs = object
                    .refs
                    .checked_add(1)
                    .ok_or_else(|| B::Error::from(NOMEM))?;
                Ok(object.refs)
            }
            "Release" => {
                let refs = object.refs - 1;
                if refs == 0 {
                    mem.free(a[0])?;
                    self.objects.remove(&a[0]);
                } else {
                    object.refs = refs;
                }
                Ok(refs)
            }
            "ConnectServer" => {
                mem.check(a[8], 4, true)?;
                let ns = mem.text(a[1], 1024)?.to_ascii_lowercase();
                let ns = ns.strip_prefix("\\\\.\\").unwrap_or(&ns);
                let code = if !matches!(ns, "root" | "root\\cimv2") {
                    0x8004100e
                } else if a[2] != 0 || a[3] != 0 || a[6] != 0 || a[7] != 0 {
                    0x8004100c
                } else {
                    0
                };
                let p = if code == 0 {
                    self.create("IWbemServices", mem)?
                } else {
                    0
                };
                mem.put(a[8], p)?;
                Ok(code)
            }
            "ExecQuery" | "ExecNotificationQuery" => {
                mem.check(a[5], 4, true)?;
                let lang = mem.text(a[1], 16)?;
                let query = mem.text(a[2], 4096)?;
                let code = if name == "ExecQuery"
                    && lang.eq_ignore_ascii_case("WQL")
                    && query
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .eq_ignore_ascii_case("SELECT * FROM Win32_OperatingSystem")
                {
                    if a[4] != 0 || a[3] & !0x30 != 0 {
                        0x80041008
                    } else {
                        0
                    }
                } else if lang.eq_ignore_ascii_case("WQL") {
                    0x80041011
                } else {
                    0x80041014
                };
                let p = if code == 0 {
                    self.create("IEnumWbemClassObject", mem)?
                } else {
                    0
                };
                mem.put(a[5], p)?;
                Ok(code)
            }
            "Reset" if kind == "IEnumWbemClassObject" => {
                object.cursor = 0;
                Ok(0)
            }
            "Skip" if kind == "IEnumWbemClassObject" => {
                let remaining = 1 - object.cursor;
                object.cursor = object.cursor.saturating_add(a[2]).min(1);
                Ok(if a[2] <= remaining { 0 } else { 1 })
            }
            "Clone" if kind == "IEnumWbemClassObject" => {
                mem.check(a[1], 4, true)?;
                let cursor = object.cursor;
                let p = self.create("IEnumWbemClassObject", mem)?;
                mem.put(a[1], p)?;
                self.objects.get_mut(&p).unwrap().cursor = cursor;
                Ok(0)
            }
            "Next" if kind == "IEnumWbemClassObject" => {
                mem.check(a[4], 4, true)?;
                if a[2] == 0 {
                    mem.put(a[4], 0)?;
                    return Ok(0);
                }
                if a[2] > 4096 {
                    mem.put(a[4], 0)?;
                    return Ok(0x80041008);
                }
                mem.check(a[3], a[2] as usize * 4, true)?;
                if object.cursor == 1 {
                    mem.put(a[4], 0)?;
                    return Ok(1);
                }
                let p = self.create("IWbemClassObject", mem)?;
                mem.put(a[3], p)?;
                mem.put(a[4], 1)?;
                self.objects.get_mut(&a[0]).unwrap().cursor = 1;
                Ok(if a[2] == 1 { 0 } else { 1 })
            }
            "Get" if kind == "IWbemClassObject" => {
                mem.check(a[3], 16, true)?;
                if a[4] != 0 {
                    mem.check(a[4], 4, true)?;
                }
                if a[5] != 0 {
                    mem.check(a[5], 4, true)?;
                }
                let key = mem.text(a[1], 256)?.to_ascii_lowercase();
                let text = match key.as_str() {
                    "__class" => "Win32_OperatingSystem",
                    "caption" => "Microsoft Windows 7",
                    "csdversion" => "",
                    "osarchitecture" => "32-bit",
                    "version" => "6.1.7601",
                    "buildnumber" => "7601",
                    _ => return Ok(0x80041002),
                };
                if a[2] != 0 {
                    return Ok(0x80041008);
                }
                let p = mem.bstr(&text.encode_utf16().collect::<Vec<_>>())?;
                let mut variant = [0; 16];
                variant[0] = 8;
                variant[8..12].copy_from_slice(&p.to_le_bytes());
                mem.write(a[3], &variant)?;
                if a[4] != 0 {
                    mem.put(a[4], 8)?;
                }
                if a[5] != 0 {
                    mem.put(a[5], 0)?;
                }
                Ok(0)
            }
            _ => Ok(NOTIMPL),
        }
    }
}
