//! PE32 class metadata uses shared records; callbacks remain in the process JIT.
use super::{
    call::Call,
    catalog::ClassApi as C,
    modules::{self, Failure},
    user::{
        self,
        class_metadata::{self, Record},
        classes::Class,
    },
    ServiceError, Services,
};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
    process::Loaded,
};

fn name(memory: &Memory, pointer: u64, wide: bool) -> Result<String, Failure> {
    if wide {
        let mut units = Vec::new();
        for n in 0..32768 {
            let address = memory.mode().address(pointer.wrapping_add(n * 2));
            let unit = memory.read_int(address, 2)? as u16;
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        Ok(String::from_utf16_lossy(&units))
    } else {
        let bytes =
            modules::string(memory, pointer, false, 32768).map_err(|error| match error {
                Failure::Windows(206) => Failure::Windows(123),
                error => error,
            })?;
        crate::codepage::decode(&bytes, 932).map_err(Failure::Windows)
    }
}
pub(crate) fn window_class_name(
    memory: &Memory,
    pointer: u64,
    wide: bool,
) -> Result<String, ServiceError> {
    name(memory, pointer, wide).map_err(|error| match error {
        Failure::Fault(fault) => fault.into(),
        Failure::Windows(_) => ServiceError::Unavailable("invalid PE32 window class name"),
    })
}
fn persistent(
    image: &mut Loaded,
    memory: &mut Memory,
    text: &str,
    menu: Option<&str>,
) -> Result<([u32; 2], [u32; 2]), Failure> {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for text in [Some(text), menu].into_iter().flatten() {
        let (ansi, wide) = class_metadata::strings(text).map_err(Failure::Windows)?;
        let a = bytes.len();
        bytes.extend(ansi);
        bytes.push(0);
        if bytes.len() % 2 != 0 {
            bytes.push(0);
        }
        let w = bytes.len();
        bytes.extend(wide.into_iter().chain(Some(0)).flat_map(u16::to_le_bytes));
        offsets.push([a, w]);
    }
    let pointer = image.data_pages.allocate(memory, bytes.len() as u64);
    if pointer == 0 {
        return Err(Failure::Windows(8));
    }
    memory.write(pointer, &bytes).expect("fresh class strings");
    image
        .data_pages
        .readonly(memory, pointer)
        .expect("fresh class string protection");
    let pointers = |offset: [usize; 2]| offset.map(|n| (pointer + n as u64) as u32);
    Ok((
        pointers(offsets[0]),
        offsets.get(1).copied().map_or([0; 2], pointers),
    ))
}
fn insert(
    image: &mut Loaded,
    memory: &mut Memory,
    fields: [u32; 11],
    text: String,
    wide: bool,
    menu: Option<&str>,
) -> Result<u16, Failure> {
    let class = Class {
        instance: fields[4].into(),
        name: text.clone(),
        procedure: fields[1].into(),
        style: fields[0],
        window_extra: fields[3] as usize,
        wide,
    };
    image
        .user
        .classes
        .check_register(&class)
        .map_err(Failure::Windows)?;
    let (name_ptrs, mut menu_ptrs) = persistent(image, memory, &text, menu)?;
    if menu.is_none() {
        menu_ptrs = [fields[8]; 2];
    }
    let atom = image
        .user
        .classes
        .register(class)
        .expect("preflighted class identity");
    image.user.class_brushes.insert(atom, fields[7].into());
    image.user.class_metadata.insert(
        atom,
        Record {
            name: text,
            fields,
            wide,
            name_ptrs,
            menu_ptrs,
        },
    );
    Ok(atom)
}
fn system(
    image: &mut Loaded,
    memory: &mut Memory,
    services: &mut Services,
    module: u64,
    text: &str,
) -> Result<(), Failure> {
    if image.user.classes.has_ascii_name(text) {
        return Ok(());
    }
    let procedure = services
        .bind("user32.dll", "DefWindowProcW")
        .map_err(|_| Failure::Windows(8))?;
    let mut fields = [0; 11];
    fields[0] = 0x4000;
    fields[1] = procedure as u32;
    fields[3] = 16;
    fields[4] = module as u32;
    let atom = insert(image, memory, fields, text.to_owned(), true, None)?;
    let record = image.user.class_metadata.get_mut(&atom).unwrap();
    record.fields[9] = record.name_ptrs[1];
    Ok(())
}
fn builtin(
    image: &mut Loaded,
    memory: &mut Memory,
    services: &mut Services,
    text: &str,
) -> Result<(), Failure> {
    if let Some(&builtin) = class_metadata::BASIC_CLASSES
        .iter()
        .find(|class| class.eq_ignore_ascii_case(text))
    {
        if !image.user.classes.has_ascii_name(builtin) {
            let module = services
                .register_provider("user32.dll")
                .map_err(|_| Failure::Windows(8))?;
            system(image, memory, services, module, builtin)?;
        }
    }
    Ok(())
}
fn value(
    id: C,
    a: &[u64; 16],
    image: &mut Loaded,
    memory: &mut Memory,
    services: &mut Services,
) -> Result<u64, Failure> {
    if matches!(id, C::InitializeControls | C::InitializeControlsEx) {
        if id == C::InitializeControlsEx && memory.read_int(a[0], 4)? != 8 {
            return Err(Failure::Windows(87));
        }
        let module = services
            .register_provider("comctl32.dll")
            .map_err(|_| Failure::Windows(8))?;
        for name in class_metadata::COMMON_CLASSES {
            system(image, memory, services, module, name)?;
        }
        return Ok(1);
    }
    if id.field() || matches!(id, C::NameA | C::NameW) {
        let atom = image
            .user
            .windows
            .get(&a[0])
            .ok_or(Failure::Windows(1400))?
            .class;
        if id.field() {
            if a[1] as u32 as i32 == -32 && !id.set() {
                return Ok(atom.into());
            }
            let record = image.user.class_metadata.get_mut(&atom).unwrap();
            let result = record
                .field(id, a[1] as u32 as i32, a[2] as u32)
                .map_err(Failure::Windows)?;
            if id.set() {
                let fields = record.fields;
                let class = image.user.classes.get_mut(atom).unwrap();
                class.instance = fields[4].into();
                class.procedure = fields[1].into();
                class.style = fields[0];
                class.window_extra = fields[3] as usize;
                image.user.class_brushes.insert(atom, fields[7].into());
            }
            return Ok(result.into());
        }
        let count = a[2] as u32;
        if a[1] == 0 || count as i32 <= 0 {
            return Ok(0);
        }
        let text: Vec<_> = image.user.class_metadata[&atom]
            .name
            .encode_utf16()
            .collect();
        let (bytes, count) =
            class_metadata::copy_text(&text, count, id.wide()).map_err(Failure::Windows)?;
        memory.write(a[1], &bytes)?;
        return Ok(count.into());
    }
    if id.register() {
        let ex = id.extended();
        if ex && memory.read_int(a[0], 4)? != 48 {
            return Err(Failure::Windows(87));
        }
        let mut fields = [0; 11];
        for (n, field) in fields.iter_mut().take(if ex { 11 } else { 10 }).enumerate() {
            *field = memory.read_int(a[0] + ((n + usize::from(ex)) * 4) as u64, 4)? as u32;
        }
        if fields[1] == 0 || fields[9] <= 65535 || fields[2] > 4096 || fields[3] > 4096 {
            return Err(Failure::Windows(87));
        }
        let text = name(memory, fields[9].into(), id.wide())?;
        if text.is_empty() || text.encode_utf16().count() > 255 {
            return Err(Failure::Windows(87));
        }
        let class = Class {
            instance: fields[4].into(),
            name: text.clone(),
            procedure: fields[1].into(),
            style: fields[0],
            window_extra: fields[3] as usize,
            wide: id.wide(),
        };
        image
            .user
            .classes
            .check_register(&class)
            .map_err(Failure::Windows)?;
        let menu = if fields[8] > 65535 {
            Some(name(memory, fields[8].into(), id.wide())?)
        } else {
            None
        };
        return insert(image, memory, fields, text, id.wide(), menu.as_deref()).map(u64::from);
    }
    let (instance, pointer) = if id.unregister() {
        (a[1], a[0])
    } else {
        (a[0], a[1])
    };
    let text = if pointer > 65535 {
        Some(name(memory, pointer, id.wide())?)
    } else {
        None
    };
    if id.query() {
        if let Some(text) = &text {
            if class_metadata::BASIC_CLASSES
                .iter()
                .any(|class| class.eq_ignore_ascii_case(text))
                && !image.user.classes.has_ascii_name(text)
            {
                memory.check(a[2], if id.extended() { 48 } else { 40 }, WRITE)?;
                builtin(image, memory, services, text)?;
            }
        }
    }
    let key = text.as_deref().map_or(
        user::classes::Key::Id(pointer as u32),
        user::classes::Key::Name,
    );
    let (atom, _) = image
        .user
        .classes
        .find(instance, key, true)
        .ok_or(Failure::Windows(1411))?;
    if id.unregister() {
        if image.user.windows.values().any(|w| w.class == atom) {
            return Err(Failure::Windows(1412));
        }
        image.user.classes.remove(atom);
        image.user.class_metadata.remove(&atom);
        image.user.class_brushes.remove(&atom);
        return Ok(1);
    }
    let fields = image.user.class_metadata[&atom].query(id.wide());
    let mut bytes = Vec::new();
    if id.extended() {
        bytes.extend(48u32.to_le_bytes());
    }
    bytes.extend(
        fields
            .iter()
            .take(if id.extended() { 11 } else { 10 })
            .flat_map(|value| value.to_le_bytes()),
    );
    memory.write(a[2], &bytes)?;
    Ok(1)
}
fn result(
    value: Result<u64, Failure>,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    match value {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => user::error(memory, state, code),
    }
}
pub(super) fn prepare_window(
    image: &mut Loaded,
    call: &Call,
    memory: &mut Memory,
    state: &State,
    services: &mut Services,
    wide: bool,
) -> Result<bool, ServiceError> {
    if memory.mode() != Mode::X86 || call.arguments[1] <= 65535 {
        return Ok(true);
    }
    let prepared = name(memory, call.arguments[1], wide)
        .and_then(|text| builtin(image, memory, services, &text));
    match prepared {
        Ok(()) => Ok(true),
        Err(error) => result(Err(error), state, memory).map(|_| false),
    }
}
pub(super) fn desktop(
    image: &mut Loaded,
    memory: &mut Memory,
    state: &State,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let handle = u64::from(user::query_values::DESKTOP);
    if image.user.windows.contains_key(&handle) {
        return Ok(handle);
    }
    let value = (|| -> Result<u64, Failure> {
        if image.user.windows.len() >= 1024 {
            return Err(Failure::Windows(8));
        }
        let module = services
            .register_provider("user32.dll")
            .map_err(|_| Failure::Windows(8))?;
        let (atom, procedure) = if memory.mode() == Mode::X64 {
            let procedure = services
                .bind("user32.dll", "DefWindowProcW")
                .map_err(|_| Failure::Windows(8))?;
            let atom = image
                .user
                .classes
                .register(Class {
                    instance: module,
                    name: "Desktop".into(),
                    procedure,
                    style: 0,
                    window_extra: 0,
                    wide: true,
                })
                .map_err(Failure::Windows)?;
            (atom, procedure)
        } else {
            system(image, memory, services, module, "Desktop")?;
            let (&atom, class) = image
                .user
                .class_metadata
                .iter()
                .find(|(_, c)| c.name.eq_ignore_ascii_case("Desktop"))
                .expect("registered desktop class");
            (atom, class.fields[1].into())
        };
        let rect = super::display::bounds();
        image.user.windows.insert(
            handle,
            user::Window {
                class: atom,
                procedure,
                instance: module,
                parent: 0,
                menu: 0,
                style: 0x10000000,
                ex_style: 0,
                rect,
                client: rect,
                data: 0,
                extra: Vec::new(),
                tid: 0,
                wide: true,
                title: String::new(),
                ready: true,
            },
        );
        Ok(handle)
    })();
    result(value, state, memory)
}
pub(super) fn dispatch(
    id: C,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    if memory.mode() == Mode::X64 {
        return image.user.class_call64(id, &call.arguments, state, memory);
    }
    let value = value(id, &call.arguments, image, memory, services);
    result(value, state, memory)
}
