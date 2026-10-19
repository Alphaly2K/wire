//! PE32 WMI COM calls with explicit object and allocation resources.
use super::{guid, iid, Wmi, NOINTERFACE, NOMEM, NOTIMPL, POINTER};
use crate::{
    api::{ApiTable, Args},
    heap::GuestHeap,
    mem::GuestMem,
    win32::com::bstr::Bstrings32,
};

pub(super) struct Request<'a> {
    pub dll: &'a str,
    pub name: &'a str,
    pub args: &'a Args,
}

impl Wmi {
    pub(super) fn call(
        &mut self,
        request: Request<'_>,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        heap: &mut GuestHeap,
        process_heap: u32,
        bstrs: &mut Bstrings32,
    ) -> (u32, String, bool) {
        let Request { dll, name, args: a } = request;
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
                let p = self.create("IWbemLocator", mem, api, heap, process_heap)?;
                mem.write_u32(a[4], p).map_err(|_| POINTER)?;
                return Ok(0);
            }
            let (kind, method) = name.split_once('.').ok_or(NOTIMPL)?;
            let object = self
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
                        self.objects.remove(&a[0]);
                        heap.free(process_heap, a[0]);
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
                    let p = self.create("IWbemServices", mem, api, heap, process_heap)?;
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
                        let p =
                            self.create("IEnumWbemClassObject", mem, api, heap, process_heap)?;
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
                    let p = self.create("IEnumWbemClassObject", mem, api, heap, process_heap)?;
                    self.objects.get_mut(&p).unwrap().cursor = cursor;
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
                    let p = self.create("IWbemClassObject", mem, api, heap, process_heap)?;
                    mem.write_u32(a[3], p).map_err(|_| POINTER)?;
                    mem.write_u32(a[4], 1).map_err(|_| POINTER)?;
                    self.objects.get_mut(&a[0]).unwrap().cursor = 1;
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
                    let p = bstrs.allocate(
                        mem,
                        heap,
                        process_heap,
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
        (
            result.unwrap_or_else(|e| e),
            if note.is_empty() {
                "local WMI COM contract".into()
            } else {
                note
            },
            supported,
        )
    }
}
