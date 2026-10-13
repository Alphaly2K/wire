//! Persistent, typed shader keyword views. Mono stores an unmapped identity;
//! every engine use rechecks the rooted shader and the original view producer.
//! Field offsets and entry targets come from the loaded metadata/registration.
use super::{bindings, callbacks, engine_objects, managed_calls, Object};
use std::{cell::RefCell, collections::BTreeMap, ffi::CStr, ptr, sync::OnceLock};

// 0x6feb is reserved for async-operation retained buffers. Keep identities in
// their own 4GiB arena, never overlapping any mapped callback data.
const BASE: u64 = 0x6fe900000000;
const LIMIT: usize = 65536;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) width: usize,
    pub(super) alignment: usize,
    pointer: usize,
    reference: Option<(usize, usize)>,
    index: Option<usize>,
}
#[derive(Clone, Copy)]
struct Producer {
    method: usize,
    owner: usize,
    target: u64,
}
struct Binding {
    space: usize,
    local: usize,
    space_layout: Layout,
    local_layout: Layout,
    producers: Vec<Producer>,
    barrier: unsafe extern "C" fn(*mut Object, Object),
}
static BINDING: OnceLock<Binding> = OnceLock::new();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Origin {
    owner: u64,
    class: usize,
    pointer: u64,
    getter: u64,
}
#[derive(Clone, Copy)]
struct View {
    origin: Origin,
    pointer: u64,
}
#[derive(Default)]
struct Registry {
    entries: BTreeMap<u64, View>,
    closed: bool,
}
impl Registry {
    fn intern(&mut self, origin: Origin, pointer: u64) -> Result<u64, String> {
        if self.closed || origin.owner == 0 || origin.pointer == 0 || origin.getter == 0 {
            return Err("keyword view has no live typed producer".into());
        }
        if pointer == 0 {
            return Ok(0);
        }
        if let Some((&token, _)) = self
            .entries
            .iter()
            .find(|(_, view)| view.origin == origin && view.pointer == pointer)
        {
            return Ok(token);
        }
        if self.entries.len() >= LIMIT {
            return Err("keyword view identity capacity exceeded".into());
        }
        let token = BASE + self.entries.len() as u64 * 16;
        self.entries.insert(token, View { origin, pointer });
        Ok(token)
    }
    fn lookup(&self, token: u64) -> Result<View, String> {
        if self.closed {
            return Err("keyword view used after retirement".into());
        }
        self.entries
            .get(&token)
            .copied()
            .ok_or_else(|| "keyword view identity is forged or belongs to another protocol".into())
    }
    fn existing(&self, origin: Origin, pointer: u64) -> Result<u64, String> {
        if self.closed {
            return Err("keyword output after view retirement".into());
        }
        self.entries
            .iter()
            .find_map(|(&token, view)| {
                (view.origin == origin && view.pointer == pointer).then_some(token)
            })
            .ok_or_else(|| "nested keyword output does not belong to its input shader space".into())
    }
}
thread_local! { static VIEWS: RefCell<Registry> = RefCell::new(Registry::default()); }
pub(super) fn installed() -> bool {
    BINDING.get().is_some()
}
pub(super) fn layout(class: Object) -> Option<Layout> {
    let b = BINDING.get()?;
    if class as usize == b.space {
        Some(b.space_layout)
    } else if class as usize == b.local {
        Some(b.local_layout)
    } else {
        None
    }
}
pub(super) fn space(class: Object) -> bool {
    BINDING.get().is_some_and(|b| b.space == class as usize)
}
pub(super) fn producer(method: Object, class: Object) -> bool {
    BINDING.get().is_some_and(|b| {
        b.space == class as usize && b.producers.iter().any(|p| p.method == method as usize)
    })
}
pub(super) fn install(
    library: Object,
    core: Object,
    images: &[Object],
    registry: &bindings::Registry,
) -> Result<(), String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing keyword view metadata API: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
    );
    let method = sym!(
        "mono_class_get_method_from_name",
        unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
    );
    let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
    let size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let fields = sym!(
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
    let name = sym!(
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const libc::c_char
    );
    let parent = sym!(
        "mono_field_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let offset = sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
    let ty = sym!(
        "mono_field_get_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let kind = sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32);
    let byref = sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32);
    let class_of = sym!(
        "mono_class_from_mono_type",
        unsafe extern "C" fn(Object) -> Object
    );
    let corlib = unsafe { corlib() };
    let system = |name: &CStr| unsafe { find(corlib, c"System".as_ptr(), name.as_ptr()) };
    let space = unsafe {
        find(
            core,
            c"UnityEngine.Rendering".as_ptr(),
            c"LocalKeywordSpace".as_ptr(),
        )
    };
    let local = unsafe {
        find(
            core,
            c"UnityEngine.Rendering".as_ptr(),
            c"LocalKeyword".as_ptr(),
        )
    };
    let intptr = system(c"IntPtr");
    let string = system(c"String");
    let uint32 = system(c"UInt32");
    let void = system(c"Void");
    if [space, local, intptr, string, uint32, void]
        .iter()
        .any(|p| p.is_null())
    {
        return Err("shader keyword actual metadata classes absent".into());
    }
    let inspect = |class: Object,
                   specs: &[(&CStr, u32, Object)]|
     -> Result<(usize, usize, Vec<usize>), String> {
        let mut alignment = 0;
        let width = unsafe { size(class, &mut alignment) };
        if !(1..=256).contains(&width) || alignment != 8 || width % 8 != 0 {
            return Err("keyword value layout is not bounded aligned storage".into());
        }
        let mut found = vec![None; specs.len()];
        let mut covered = vec![false; width as usize];
        let mut iterator = ptr::null_mut();
        loop {
            let field = unsafe { fields(class, &mut iterator) };
            if field.is_null() {
                break;
            }
            let attrs = unsafe { flags(field) };
            if attrs & (0x10 | 0x40) != 0 {
                continue;
            }
            let text = unsafe { name(field) };
            let field_type = unsafe { ty(field) };
            if text.is_null()
                || field_type.is_null()
                || unsafe { parent(field) } != class
                || attrs & 0x20 == 0
            {
                return Err("keyword value must have declared immutable fields".into());
            }
            let index = specs
                .iter()
                .position(|(label, _, _)| unsafe { CStr::from_ptr(text) } == *label)
                .ok_or("unrecognized shader keyword instance field")?;
            let (_, expected_kind, expected_class) = specs[index];
            let field_offset = unsafe { offset(field) };
            let field_width = if expected_kind == 9 { 4 } else { 8 };
            if found[index].is_some()
                || unsafe { kind(field_type) } != expected_kind
                || unsafe { byref(field_type) } != 0
                || unsafe { class_of(field_type) } != expected_class
                || field_offset < 16
            {
                return Err("shader keyword actual field type differs from protocol".into());
            }
            let at = (field_offset - 16) as usize;
            if !at.is_multiple_of(field_width)
                || at + field_width > width as usize
                || covered[at..at + field_width].iter().any(|b| *b)
            {
                return Err("shader keyword fields overlap or exceed value layout".into());
            }
            covered[at..at + field_width].fill(true);
            found[index] = Some(at);
        }
        Ok((
            width as usize,
            alignment as usize,
            found
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or("shader keyword required field absent")?,
        ))
    };
    let (width, alignment, offsets) = inspect(space, &[(c"m_KeywordSpace", 0x18, intptr)])?;
    if width != 8 || offsets != [0] {
        return Err("keyword space must be a single immutable IntPtr".into());
    }
    let space_layout = Layout {
        width,
        alignment,
        pointer: offsets[0],
        reference: None,
        index: None,
    };
    let (width, alignment, offsets) = inspect(
        local,
        &[
            (c"m_SpaceInfo", 0x11, space),
            (c"m_Name", 0xe, string),
            (c"m_Index", 9, uint32),
        ],
    )?;
    let local_layout = Layout {
        width,
        alignment,
        pointer: offsets[0],
        reference: Some((offsets[1], string as usize)),
        index: Some(offsets[2]),
    };
    let api = managed_calls::Api::load(library)?;
    let mut producers = Vec::new();
    for (namespace, label) in [
        (c"UnityEngine", c"Shader"),
        (c"UnityEngine", c"ComputeShader"),
        (c"UnityEngine.Rendering", c"RayTracingShader"),
    ] {
        let owner = unsafe { find(core, namespace.as_ptr(), label.as_ptr()) };
        if owner.is_null() {
            continue;
        }
        let method = unsafe { method(owner, c"get_keywordSpace_Injected".as_ptr(), 2) };
        if method.is_null() {
            continue;
        }
        api.fixed_typed_icall(
            method,
            owner,
            &[(1, void, false), (0x18, intptr, false), (0x11, space, true)],
            images,
        )?;
        let binding = format!(
            "{}.{}::get_keywordSpace_Injected",
            namespace.to_str().unwrap(),
            label.to_str().unwrap()
        );
        let Some(target) = registry.registered_target(&binding)? else {
            continue;
        };
        producers.push(Producer {
            method: method as usize,
            owner: owner as usize,
            target,
        });
    }
    if producers.is_empty() {
        return Err("shader keyword space has no registered typed producers".into());
    }
    BINDING
        .set(Binding {
            space: space as usize,
            local: local as usize,
            space_layout,
            local_layout,
            producers,
            barrier: sym!(
                "mono_gc_wbarrier_generic_store",
                unsafe extern "C" fn(*mut Object, Object)
            ),
        })
        .map_err(|_| "keyword view bridge already installed".into())
}
pub(super) fn origin(method: Object, pointer: u64) -> Result<Origin, String> {
    let b = BINDING.get().ok_or("keyword metadata absent")?;
    let producer = b
        .producers
        .iter()
        .find(|p| p.method == method as usize)
        .ok_or("keyword value output lacks a typed producer")?;
    engine_objects::engine_pointer(pointer as usize)?;
    let owner = callbacks::with_metadata_service(|route, memory| {
        route.sync_guest(memory)?;
        let roots = route
            .objects
            .as_ref()
            .ok_or("keyword producer has no managed roots")?;
        roots
            .roots
            .engine_owner(roots.api, pointer, producer.owner as Object)
    })?;
    Ok(Origin {
        owner,
        class: producer.owner,
        pointer,
        getter: producer.target,
    })
}
fn resolve(token: u64) -> Result<u64, String> {
    if token == 0 {
        return Ok(0);
    }
    let view = VIEWS.with(|views| views.borrow().lookup(token))?;
    let pointer = callbacks::with_metadata_service(|route, memory| {
        route.sync_guest(memory)?;
        let roots = route
            .objects
            .as_ref()
            .ok_or("keyword view has no managed owner roots")?;
        roots
            .roots
            .engine_cached_pointer(roots.api, view.origin.owner, view.origin.class as Object)
    })?;
    if pointer != view.origin.pointer || pointer == 0 {
        return Err("keyword view owner was destroyed or rebound".into());
    }
    engine_objects::engine_pointer(pointer as usize)?;
    let (_, outputs) = callbacks::guest_value_span_arguments(
        view.origin.getter,
        &[pointer, 0],
        &[],
        &[(1, &[0; 8])],
        &[],
        &[],
        callbacks::Work::Asset,
    )?;
    if outputs.len() != 1 || outputs[0].as_slice() != view.pointer.to_le_bytes() {
        return Err("shader keyword view changed since its identity was issued".into());
    }
    engine_objects::engine_pointer(view.pointer as usize)
}
impl Layout {
    fn check_storage(self, address: usize) -> Result<(), String> {
        if address == 0
            || !address.is_multiple_of(self.alignment)
            || address.checked_add(self.width).is_none()
        {
            return Err("keyword native value storage is unaligned or absent".into());
        }
        Ok(())
    }
    /// Native padding is never sent to guest, and m_Name remains rooted through
    /// the existing typed reference service for the complete nested callback.
    pub(super) unsafe fn encode(self, address: usize) -> Result<Vec<u8>, String> {
        self.check_storage(address)?;
        let pointer = unsafe { ((address + self.pointer) as *const u64).read() };
        let mut bytes = vec![0; self.width];
        bytes[self.pointer..self.pointer + 8].copy_from_slice(&resolve(pointer)?.to_le_bytes());
        if let Some((at, class)) = self.reference {
            let native = unsafe { ((address + at) as *const Object).read() };
            let token = callbacks::with_metadata_service(|route, memory| {
                let native = managed_calls::Api::load(route.library)?.compatible(
                    native,
                    managed_calls::Shape::Reference {
                        class: class as Object,
                        string: true,
                    },
                )?;
                route.root_reference(native, memory)
            })?;
            bytes[at..at + 8].copy_from_slice(&token.to_le_bytes());
        }
        if let Some(at) = self.index {
            let index = unsafe { ((address + at) as *const u32).read() };
            bytes[at..at + 4].copy_from_slice(&index.to_le_bytes());
        }
        Ok(bytes)
    }
    pub(super) unsafe fn origin(self, address: usize) -> Result<Option<Origin>, String> {
        self.check_storage(address)?;
        let token = unsafe { ((address + self.pointer) as *const u64).read() };
        if token == 0 {
            return Ok(None);
        }
        VIEWS.with(|views| views.borrow().lookup(token).map(|view| Some(view.origin)))
    }
    pub(super) fn publish(
        self,
        bytes: &[u8],
        origin: Option<Origin>,
    ) -> Result<NativeValue, String> {
        if bytes.len() != self.width {
            return Err("keyword producer requires exact typed output".into());
        }
        let pointer = u64::from_le_bytes(bytes[self.pointer..self.pointer + 8].try_into().unwrap());
        let token = if pointer == 0 {
            0
        } else {
            engine_objects::engine_pointer(pointer as usize)?;
            let origin = origin.ok_or("keyword output has no rooted shader provenance")?;
            VIEWS.with(|views| {
                if self.reference.is_some() {
                    views.borrow().existing(origin, pointer)
                } else {
                    views.borrow_mut().intern(origin, pointer)
                }
            })?
        };
        let mut result = vec![0; self.width];
        result[self.pointer..self.pointer + 8].copy_from_slice(&token.to_le_bytes());
        let reference = if let Some((at, class)) = self.reference {
            let token = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
            if token == 0 {
                Some((at, ptr::null_mut()))
            } else {
                let native = callbacks::with_metadata_service(|route, _| {
                    let roots = route
                        .objects
                        .as_ref()
                        .ok_or("keyword output has no typed reference roots")?;
                    roots.roots.compatible(roots.api, token, class as Object)
                })?;
                Some((at, native))
            }
        } else {
            None
        };
        if let Some(at) = self.index {
            result[at..at + 4].copy_from_slice(&bytes[at..at + 4]);
        }
        Ok(NativeValue {
            bytes: result,
            reference,
        })
    }
    pub(super) unsafe fn commit(self, address: usize, value: &NativeValue) -> Result<(), String> {
        self.check_storage(address)?;
        if value.bytes.len() != self.width
            || value.reference.map(|r| r.0) != self.reference.map(|r| r.0)
        {
            return Err("keyword native output requires exact typed reference slots".into());
        }
        // Never overwrite m_Name with a token or an unbarriered native pointer.
        for (index, &byte) in value.bytes.iter().enumerate() {
            if self
                .reference
                .is_some_and(|(at, _)| (at..at + 8).contains(&index))
            {
                continue;
            }
            unsafe { ((address + index) as *mut u8).write(byte) };
        }
        if let Some((at, native)) = value.reference {
            let b = BINDING
                .get()
                .ok_or("keyword reference output barrier absent")?;
            unsafe { (b.barrier)((address + at) as *mut Object, native) };
        }
        Ok(())
    }
}
pub(super) struct NativeValue {
    bytes: Vec<u8>,
    reference: Option<(usize, Object)>,
}
pub(super) fn retire_after_vm_cleanup() {
    VIEWS.with(|views| views.borrow_mut().closed = true);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn origin(owner: u64) -> Origin {
        Origin {
            owner,
            class: 2,
            pointer: 0x1000,
            getter: 0x2000,
        }
    }
    #[test]
    fn persistent_views_preserve_equality_but_never_alias_reused_owner_addresses() {
        let mut views = Registry::default();
        let first = views.intern(origin(1), 0x3000).unwrap();
        assert_eq!(views.intern(origin(1), 0x3000).unwrap(), first);
        assert_eq!(views.existing(origin(1), 0x3000).unwrap(), first);
        let second = views.intern(origin(2), 0x3000).unwrap();
        assert_ne!(first, second);
        assert_eq!(views.lookup(first).unwrap().origin.owner, 1);
        assert!(views.existing(origin(1), 0x4000).is_err());
        assert!(views.lookup(first + 1).is_err());
        assert!(views.lookup(0x6fec00000000).is_err());
        assert_eq!(views.intern(origin(1), 0).unwrap(), 0);
        views.closed = true;
        assert!(views.lookup(first).is_err());
        assert!(views.intern(origin(1), 0x3000).is_err());
        assert!(views.existing(origin(1), 0x3000).is_err());
    }
    #[test]
    fn invalid_provenance_and_wrong_width_fail_before_changing_native_output() {
        let mut views = Registry::default();
        assert!(views.intern(origin(0), 0x3000).is_err());
        let mut wrong = origin(1);
        wrong.getter = 0;
        assert!(views.intern(wrong, 0x3000).is_err());
        let layout = Layout {
            width: 8,
            alignment: 8,
            pointer: 0,
            reference: None,
            index: None,
        };
        let mut destination = [99u64];
        let value = NativeValue {
            bytes: vec![0; 7],
            reference: None,
        };
        assert!(unsafe { layout.commit(destination.as_mut_ptr() as usize, &value) }.is_err());
        assert_eq!(destination, [99]);
        assert!(unsafe { layout.commit(destination.as_mut_ptr() as usize + 1, &value) }.is_err());
        assert!(layout.publish(&[0; 7], None).is_err());
        let value = layout.publish(&[0; 8], None).unwrap();
        unsafe { layout.commit(destination.as_mut_ptr() as usize, &value) }.unwrap();
        assert_eq!(destination, [0]);
    }
}
