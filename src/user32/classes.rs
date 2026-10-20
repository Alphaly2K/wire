//! PE32 checked class records and persistent names; public legacy table stays in User32.
use super::*;
use crate::win32::catalog::ClassApi;

#[derive(Clone)]
pub struct WindowClass {
    pub name: String,
    pub fields: [u32; 11], // style through hIconSm, excluding cbSize
    pub(super) wide: bool,
    pub(super) name_ptrs: [u32; 2],
    menu_ptrs: [u32; 2],
}

pub(crate) fn persistent_strings(
    mem: &mut GuestMem,
    scratch: &mut crate::win32::x86::scratch::Scratch32,
    last_error: &mut u32,
    text: &str,
) -> Result<[u32; 2], String> {
    let ansi = crate::codepage::encode(text, 932)
        .map_err(|e| format!("class name encoding: {e}"))?
        .0;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let a = scratch.alloc(mem, ansi.len() as u32 + 1, 1, last_error);
    let w = scratch.alloc(mem, (wide.len() as u32 + 1) * 2, 2, last_error);
    mem.write_bytes(a, &ansi).map_err(|e| e.to_string())?;
    mem.write_u8(a + ansi.len() as u32, 0)
        .map_err(|e| e.to_string())?;
    for (i, value) in wide.into_iter().chain([0]).enumerate() {
        mem.write_u16(w + i as u32 * 2, value)
            .map_err(|e| e.to_string())?;
    }
    Ok([a, w])
}

pub fn find_class(
    mem: &GuestMem,
    state: &ApiState,
    id: u32,
    instance: u32,
    wide: bool,
) -> Result<Option<u16>, String> {
    state.user32.find_class(mem, id, instance, wide)
}
impl User32 {
    pub(super) fn find_class(
        &self,
        mem: &GuestMem,
        id: u32,
        instance: u32,
        wide: bool,
    ) -> Result<Option<u16>, String> {
        let name = if id > 0xffff {
            Some(
                crate::file_api::read_name(mem, id, wide)
                    .map_err(|e| format!("class name: {e}"))?
                    .to_lowercase(),
            )
        } else {
            None
        };
        Ok(self.classes.iter().find_map(|(&atom, class)| {
            let matches_name = name
                .as_ref()
                .map_or(atom as u32 == id, |n| class.name.to_lowercase() == *n);
            (matches_name && (class.fields[4] == instance || class.fields[0] & 0x4000 != 0))
                .then_some(atom)
        }))
    }
}

impl User32 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_system_class(
        &mut self,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        scratch: &mut crate::win32::x86::scratch::Scratch32,
        last_error: &mut u32,
        module: u32,
        name: &str,
        wide: bool,
    ) -> Result<(), String> {
        if self
            .classes
            .values()
            .any(|c| c.name.eq_ignore_ascii_case(name))
        {
            return Ok(());
        }
        if self.next_atom > 0xffff {
            return Err("system class atom table exhausted".into());
        }
        let name_ptrs = persistent_strings(mem, scratch, last_error, name)?;
        let mut fields = [0; 11];
        fields[0] = 0x4000; // CS_GLOBALCLASS
        fields[1] = api.register(
            "user32.dll",
            Some(if wide {
                "DefWindowProcW"
            } else {
                "DefWindowProcA"
            }),
            None,
        );
        fields[3] = 16;
        fields[4] = module;
        fields[9] = name_ptrs[usize::from(wide)];
        let atom = self.next_atom as u16;
        self.next_atom += 1;
        self.classes.insert(
            atom,
            WindowClass {
                name: name.into(),
                fields,
                wide,
                name_ptrs,
                menu_ptrs: [0; 2],
            },
        );
        Ok(())
    }
}

impl User32 {
    #[allow(clippy::too_many_arguments)]
    fn initialize_controls(
        &mut self,
        extended: bool,
        record: u32,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        modules: &mut crate::modules::Modules,
        scratch: &mut crate::win32::x86::scratch::Scratch32,
        last_error: &mut u32,
    ) -> Result<(u32, String), String> {
        if extended && mem.read_u32(record).map_err(|e| e.to_string())? != 8 {
            *last_error = 87;
            return Ok((0, String::new()));
        }
        let module = modules.provider_handle_for("comctl32.dll");
        for class in [
            "Button",
            "Edit",
            "Static",
            "ListBox",
            "ComboBox",
            "ScrollBar",
            "SysListView32",
            "SysTreeView32",
            "SysTabControl32",
            "msctls_progress32",
        ] {
            self.register_system_class(mem, api, scratch, last_error, module, class, true)?;
        }
        Ok((1, "registered controls with basic window/text state".into()))
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn class_call(
        &mut self,
        id: ClassApi,
        a: &crate::api::Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        modules: &mut crate::modules::Modules,
        scratch: &mut crate::win32::x86::scratch::Scratch32,
        last_error: &mut u32,
    ) -> Result<(u32, String), String> {
        if matches!(
            id,
            ClassApi::InitializeControls | ClassApi::InitializeControlsEx
        ) {
            return self.initialize_controls(
                id == ClassApi::InitializeControlsEx,
                a[0],
                mem,
                api,
                modules,
                scratch,
                last_error,
            );
        }
        if id.field() {
            return self.field_call(id, a, last_error);
        }
        if matches!(id, ClassApi::NameA | ClassApi::NameW) {
            let Some(window) = self.windows.get(&a[0]) else {
                *last_error = 1400;
                return Ok((0, "USER32 error 1400".into()));
            };
            let text: Vec<u16> = self.classes[&window.class].name.encode_utf16().collect();
            let value = crate::user32_common::copy_text(mem, a[1], a[2], &text, id.wide())?;
            return Ok((value, "class name".into()));
        }
        let wide = id.wide();
        let extended = id.extended();
        if id.query() {
            self.ensure_builtin(mem, api, modules, scratch, last_error, a[1], wide)?;
        }
        let fail = |last_error: &mut u32, code| {
            *last_error = code;
            Ok((0, format!("window class error {code}")))
        };
        if id.register() {
            if extended && mem.read_u32(a[0]).map_err(|e| e.to_string())? != 48 {
                return fail(last_error, 87);
            }
            let mut fields = [0; 11];
            for (i, value) in fields
                .iter_mut()
                .take(if extended { 11 } else { 10 })
                .enumerate()
            {
                *value = mem
                    .read_u32(a[0] + (i as u32 + extended as u32) * 4)
                    .map_err(|e| e.to_string())?;
            }
            if fields[1] == 0 || fields[9] <= 0xffff || fields[2] > 4096 || fields[3] > 4096 {
                return fail(last_error, 87);
            }
            let text = crate::file_api::read_name(mem, fields[9], wide)
                .map_err(|e| format!("class name: {e}"))?;
            if text.is_empty() || text.encode_utf16().count() > 255 {
                return fail(last_error, 87);
            }
            if self.find_class(mem, fields[9], fields[4], wide)?.is_some() {
                return fail(last_error, 1410);
            }
            if self.next_atom > 0xffff {
                return fail(last_error, 8);
            }
            let name_ptrs = persistent_strings(mem, scratch, last_error, &text)?;
            let menu_ptrs = if fields[8] <= 0xffff {
                [fields[8]; 2]
            } else {
                let menu = crate::file_api::read_name(mem, fields[8], wide)
                    .map_err(|e| format!("class menu: {e}"))?;
                persistent_strings(mem, scratch, last_error, &menu)?
            };
            let atom = self.next_atom as u16;
            self.next_atom += 1;
            self.classes.insert(
                atom,
                WindowClass {
                    name: text.clone(),
                    fields,
                    wide,
                    name_ptrs,
                    menu_ptrs,
                },
            );
            return Ok((atom as u32, format!("registered {text}")));
        }
        let unregister = id.unregister();
        let (instance, id) = if unregister {
            (a[1], a[0])
        } else {
            (a[0], a[1])
        };
        let Some(atom) = self.find_class(mem, id, instance, wide)? else {
            return fail(last_error, 1411);
        };
        if unregister {
            if self.windows.values().any(|w| w.class == atom) {
                return fail(last_error, 1412);
            }
            self.classes.remove(&atom);
            return Ok((1, "class unregistered".into()));
        }
        let class = self.classes.get(&atom).unwrap();
        let mut fields = class.fields;
        fields[8] = class.menu_ptrs[wide as usize];
        fields[9] = class.name_ptrs[wide as usize];
        let mut bytes = Vec::new();
        if extended {
            bytes.extend(48u32.to_le_bytes());
        }
        for value in fields.into_iter().take(if extended { 11 } else { 10 }) {
            bytes.extend(value.to_le_bytes());
        }
        mem.write_bytes(a[2], &bytes).map_err(|e| e.to_string())?;
        Ok((1, format!("class {} atom={atom:#x}", class.name)))
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ensure_builtin(
        &mut self,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        modules: &mut crate::modules::Modules,
        scratch: &mut crate::win32::x86::scratch::Scratch32,
        last_error: &mut u32,
        pointer: u32,
        wide: bool,
    ) -> Result<(), String> {
        if pointer > 0xffff {
            if let Ok(class) = crate::file_api::read_name(mem, pointer, wide) {
                if let Some(builtin) = [
                    "Button",
                    "Edit",
                    "Static",
                    "ListBox",
                    "ComboBox",
                    "ScrollBar",
                ]
                .into_iter()
                .find(|name| name.eq_ignore_ascii_case(&class))
                {
                    let module = modules.provider_handle_for("user32.dll");
                    self.register_system_class(
                        mem, api, scratch, last_error, module, builtin, true,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn field_call(
        &mut self,
        id: ClassApi,
        a: &crate::api::Args,
        last_error: &mut u32,
    ) -> Result<(u32, String), String> {
        let Some(window) = self.windows.get(&a[0]) else {
            *last_error = 1400;
            return Ok((0, "invalid class HWND".into()));
        };
        if a[1] as i32 == -32 && !id.set() {
            return Ok((window.class as u32, "class atom".into()));
        }
        let index = match a[1] as i32 {
            -26 => 0,
            -24 => 1,
            -20 => 2,
            -18 => 3,
            -16 => 4,
            -14 => 5,
            -12 => 6,
            -10 => 7,
            -8 => 8,
            -34 => 10,
            _ => {
                *last_error = 1413;
                return Ok((0, "unsupported class index".into()));
            }
        };
        let class = self.classes.get_mut(&window.class).unwrap();
        let old = class.fields[index];
        if id.set() {
            if matches!(index, 2 | 3) && a[2] > 4096 {
                *last_error = 87;
                return Ok((0, "class extra storage too large".into()));
            }
            if index == 8 {
                *last_error = 120;
                return Ok((0, "class menu-name change unavailable".into()));
            }
            class.fields[index] = a[2];
        }
        Ok((old, "class field state".into()))
    }
}
