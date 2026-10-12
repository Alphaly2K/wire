//! Owned, exact-width initialized instance field cells. Guest reference values
//! are rooted identities; native headers and host pointers never enter a view.
use super::{c_char, instances, value_layout, Memory, Object};
use crate::x64::memory::{shared::HostRegion, READ, WRITE};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
    ptr,
};
#[derive(Clone, Copy)]
struct Api {
    find: unsafe extern "C" fn(Object, *const c_char) -> Object,
    parent: unsafe extern "C" fn(Object) -> Object,
    flags: unsafe extern "C" fn(Object) -> u32,
    offset: unsafe extern "C" fn(Object) -> i32,
    ty: unsafe extern "C" fn(Object) -> Object,
    kind: unsafe extern "C" fn(Object) -> u32,
    byref: unsafe extern "C" fn(Object) -> u32,
    type_class: unsafe extern "C" fn(Object) -> Object,
    instance_size: unsafe extern "C" fn(Object) -> i32,
    is_value: unsafe extern "C" fn(Object) -> i32,
    value_size: unsafe extern "C" fn(Object, *mut u32) -> i32,
    assignable: unsafe extern "C" fn(Object, Object) -> i32,
    get: unsafe extern "C" fn(Object, Object, Object),
    set: unsafe extern "C" fn(Object, Object, Object),
    barrier: unsafe extern "C" fn(Object, Object, Object),
}
impl Api {
    fn load(library: Object) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing instance field API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        Ok(Self {
            find: sym!(
                "mono_class_get_field_from_name",
                unsafe extern "C" fn(Object, *const c_char) -> Object
            ),
            parent: sym!(
                "mono_field_get_parent",
                unsafe extern "C" fn(Object) -> Object
            ),
            flags: sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32),
            offset: sym!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32),
            ty: sym!(
                "mono_field_get_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            kind: sym!("mono_type_get_type", unsafe extern "C" fn(Object) -> u32),
            byref: sym!("mono_type_is_byref", unsafe extern "C" fn(Object) -> u32),
            type_class: sym!(
                "mono_class_from_mono_type",
                unsafe extern "C" fn(Object) -> Object
            ),
            is_value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            value_size: sym!(
                "mono_class_value_size",
                unsafe extern "C" fn(Object, *mut u32) -> i32
            ),
            instance_size: sym!(
                "mono_class_instance_size",
                unsafe extern "C" fn(Object) -> i32
            ),
            assignable: sym!(
                "mono_class_is_assignable_from",
                unsafe extern "C" fn(Object, Object) -> i32
            ),
            get: sym!(
                "mono_field_get_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            set: sym!(
                "mono_field_set_value",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            barrier: sym!(
                "mono_gc_wbarrier_set_field",
                unsafe extern "C" fn(Object, Object, Object)
            ),
        })
    }
}
#[derive(Clone, Copy)]
enum Shape {
    Scalar { width: usize, boolean: bool },
    Reference(Object),
    EnginePointer(u64),
    Inline { width: usize, alignment: usize },
}
impl Shape {
    fn width(self) -> usize {
        match self {
            Self::Scalar { width, .. } => width,
            Self::Reference(_) | Self::EnginePointer(_) => 8,
            Self::Inline { width, .. } => width,
        }
    }
}
struct Cell {
    payload_offset: Option<usize>,
    owner: u64,
    field: Object,
    shape: Shape,
    view: HostRegion,
    view_offset: usize,
    committed: Vec<u8>,
    published: bool,
}
struct InlinePayload {
    owner: u64,
    field: Object,
    layout: value_layout::Layout,
    view: HostRegion,
}
#[derive(Default)]
pub(super) struct Registry {
    api: Option<Api>,
    cells: BTreeMap<u64, Cell>,
    retired: bool,
    writes: usize,
    reference_writes: usize,
    verified: usize,
    refreshed: usize,
    payloads: BTreeMap<u64, (value_layout::Layout, HostRegion)>,
    inline_payloads: BTreeMap<u64, InlinePayload>,
    unboxes: usize,
}
pub(super) struct Spec {
    pub(super) owner: Object,
    pub(super) name: CString,
    pub(super) kind: u32,
    pub(super) type_class: Option<Object>,
    pub(super) guest_pointer: Option<u64>,
    pub(super) inline: Option<(usize, usize)>,
    pub(super) nested: Option<value_layout::Layout>,
}
pub(super) struct Refresh {
    updates: Vec<(u64, Vec<u8>)>,
    pub(super) references: Vec<Object>,
}
impl Registry {
    fn read_native(api: Api, object: Object, cell: &Cell, bytes: &mut [u8]) {
        if let Some(offset) = cell.payload_offset {
            unsafe {
                ptr::copy_nonoverlapping(
                    object.cast::<u8>().add(16 + offset),
                    bytes.as_mut_ptr(),
                    bytes.len(),
                );
            }
        } else {
            unsafe {
                (api.get)(object, cell.field, bytes.as_mut_ptr().cast());
            }
        }
    }
    fn write_native(
        api: Api,
        object: Object,
        cell: &Cell,
        bytes: &mut [u8],
        reference: Option<Object>,
    ) {
        if let Some(offset) = cell.payload_offset {
            let destination = unsafe { object.cast::<u8>().add(16 + offset) };
            if let Some(reference) = reference {
                // The complete boxed object remains the GC owner for nested
                // reference leaves; never use a guest token as the barrier owner.
                unsafe {
                    (api.barrier)(object, destination.cast(), reference);
                }
            } else {
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
                }
            }
        } else {
            let value = reference.unwrap_or_else(|| bytes.as_mut_ptr().cast());
            unsafe {
                (api.set)(object, cell.field, value);
            }
        }
    }
    pub(super) fn install_with(
        &mut self,
        library: Object,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        memory: &mut Memory,
        specs: Vec<Spec>,
        mut reference: impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<(), String> {
        if self.retired || specs.len() > 256 || self.cells.len() + specs.len() > 32768 {
            return Err("initialized field views outside single bounded installation".into());
        }
        let api = Api::load(library)?;
        self.api = Some(api);
        for spec in specs {
            let owner = roots.borrow(root_api, spec.owner)?;
            let class = roots.class(root_api, owner)?;
            let field = unsafe { (api.find)(class, spec.name.as_ptr()) };
            if field.is_null() || unsafe { (api.flags)(field) } & (0x10 | 0x40) != 0 {
                return Err(format!(
                    "initialized instance field absent: {:?}",
                    spec.name
                ));
            }
            let parent = unsafe { (api.parent)(field) };
            let ty = unsafe { (api.ty)(field) };
            if parent.is_null()
                || ty.is_null()
                || unsafe { (api.assignable)(parent, class) } == 0
                || unsafe { (api.kind)(ty) } != spec.kind
                || unsafe { (api.byref)(ty) } != 0
            {
                return Err(format!(
                    "initialized field declaration/type mismatch: {:?}",
                    spec.name
                ));
            }
            if let Some(layout) = spec.nested {
                if spec.inline.is_some()
                    || spec.guest_pointer.is_some()
                    || spec.kind != 0x11
                    || spec.type_class != Some(layout.class)
                    || unsafe { (api.type_class)(ty) } != layout.class
                    || unsafe { (api.is_value)(layout.class) } == 0
                {
                    return Err("nested instance value type mismatch".into());
                }
                let mut alignment = 0;
                if unsafe { (api.value_size)(layout.class, &mut alignment) } != layout.width as i32
                    || alignment as usize != layout.alignment
                {
                    return Err("nested instance value size/alignment mismatch".into());
                }
                self.install_inline(
                    root_api,
                    roots,
                    memory,
                    (owner, field),
                    layout,
                    &mut reference,
                )?;
                continue;
            }
            let shape = if let Some((width, alignment)) = spec.inline {
                let mut actual_alignment = 0;
                if spec.kind != 0x11
                    || spec.type_class.is_none()
                    || spec.guest_pointer.is_some()
                    || !(1..=256).contains(&width)
                    || !matches!(alignment, 1 | 2 | 4 | 8)
                    || !width.is_multiple_of(alignment)
                    || unsafe { (api.type_class)(ty) } != spec.type_class.unwrap()
                    || unsafe { (api.is_value)(spec.type_class.unwrap()) } == 0
                    || unsafe { (api.value_size)(spec.type_class.unwrap(), &mut actual_alignment) }
                        != width as i32
                    || actual_alignment as usize != alignment
                {
                    return Err("initialized inline field type/layout mismatch".into());
                }
                Shape::Inline { width, alignment }
            } else if let Some(expected) = spec.guest_pointer {
                if spec.kind != 0x18
                    || spec.type_class.is_some()
                    || expected == 0
                    || !expected.is_multiple_of(8)
                {
                    return Err("engine field is not a checked guest resource pointer".into());
                }
                memory.check(expected, 1, READ)?;
                Shape::EnginePointer(expected)
            } else {
                match (spec.kind, spec.type_class) {
                    (2 | 4 | 5, None) => Shape::Scalar {
                        width: 1,
                        boolean: spec.kind == 2,
                    },
                    (3 | 6 | 7, None) => Shape::Scalar {
                        width: 2,
                        boolean: false,
                    },
                    (8 | 9 | 0xc, None) => Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                    (0xa | 0xb | 0xd, None) => Shape::Scalar {
                        width: 8,
                        boolean: false,
                    },
                    (0xe | 0x12 | 0x15 | 0x1c | 0x1d, Some(target))
                        if !target.is_null()
                            && unsafe { (api.type_class)(ty) } == target
                            && unsafe { (api.is_value)(target) } == 0 =>
                    {
                        Shape::Reference(target)
                    }
                    _ => return Err("unadmitted initialized field shape".into()),
                }
            };
            let offset = unsafe { (api.offset)(field) };
            let size = unsafe { (api.instance_size)(class) };
            let width = shape.width();
            if offset < 16
                || !(16..=65536).contains(&size)
                || offset as usize + width > size as usize
                || !(offset as usize).is_multiple_of(match shape {
                    Shape::Inline { alignment, .. } => alignment,
                    _ => width,
                })
            {
                return Err("initialized field offset/size/alignment mismatch".into());
            }
            let address = owner + offset as u64;
            if self.cells.iter().any(|(&base, c)| {
                address < base + c.shape.width() as u64 && base < address + width as u64
            }) || self.inline_payloads.iter().any(|(&base, p)| {
                address < base + p.layout.width as u64 && base < address + width as u64
            }) {
                return Err("overlapping initialized field views".into());
            }
            let mut value = vec![0; width];
            unsafe {
                (api.get)(spec.owner, field, value.as_mut_ptr().cast());
            }
            if let Shape::Reference(target) = shape {
                let native = u64::from_le_bytes(value.as_slice().try_into().unwrap()) as Object;
                let token = if native.is_null() {
                    0
                } else {
                    let token = reference(roots, native)?;
                    roots.compatible(root_api, token, target)?;
                    token
                };
                value.copy_from_slice(&token.to_le_bytes());
            }
            if let Shape::EnginePointer(expected) = shape {
                if value != expected.to_le_bytes() {
                    return Err("managed engine pointer differs from owned resource".into());
                }
            }
            let view = HostRegion::owned(width)?;
            view.with_native(|p| unsafe {
                ptr::copy_nonoverlapping(value.as_ptr(), p.as_ptr(), width)
            })?;
            self.cells.insert(
                address,
                Cell {
                    payload_offset: None,
                    owner,
                    field,
                    shape,
                    view,
                    view_offset: 0,
                    committed: value,
                    published: false,
                },
            );
            let cell = self.cells.get_mut(&address).unwrap();
            let permissions = if matches!(shape, Shape::EnginePointer(_)) {
                READ
            } else {
                READ | WRITE
            };
            memory.map_shared_data(address, cell.view.clone(), permissions, permissions)?;
            cell.published = true;
            eprintln!("NATIVE_MONO_INSTANCE_FIELD: owner={owner:#x} field={:?} offset={offset} width={width}", spec.name);
        }
        Ok(())
    }
    fn install_inline(
        &mut self,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        memory: &mut Memory,
        receiver: (u64, Object),
        layout: value_layout::Layout,
        reference: &mut impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<(), String> {
        let (owner, field) = receiver;
        layout.check()?;
        let api = self.api.ok_or("nested value before field API")?;
        let object = roots.native(root_api, owner)?;
        let offset = unsafe { (api.offset)(field) };
        let size = unsafe { (api.instance_size)(roots.class(root_api, owner)?) };
        if !layout.value
            || offset < 16
            || !(16..=65536).contains(&size)
            || offset as usize + layout.width > size as usize
            || !(offset as usize).is_multiple_of(layout.alignment)
            || self.cells.len() + layout.slots.len() > 32768
        {
            return Err("nested instance field offset/size/capacity mismatch".into());
        }
        let address = owner + offset as u64;
        if self.cells.iter().any(|(&base, c)| {
            address < base + c.shape.width() as u64 && base < address + layout.width as u64
        }) || self.inline_payloads.iter().any(|(&base, p)| {
            address < base + p.layout.width as u64 && base < address + layout.width as u64
        }) {
            return Err("nested instance field envelopes overlap".into());
        }
        // Copy declared leaves only. Padding is freshly zeroed; nested
        // references are rooted identities, not raw pointers from Mono's heap.
        let mut bytes = vec![0; layout.width];
        let mut records = Vec::new();
        for slot in &layout.slots {
            let width = slot.shape.width();
            let value = &mut bytes[slot.offset..slot.offset + width];
            unsafe {
                ptr::copy_nonoverlapping(
                    object.cast::<u8>().add(offset as usize + slot.offset),
                    value.as_mut_ptr(),
                    width,
                )
            };
            let shape = match slot.shape {
                value_layout::Shape::Reference(class) => {
                    let native = u64::from_le_bytes(value.try_into().unwrap()) as Object;
                    let token = if native.is_null() {
                        0
                    } else {
                        let token = reference(roots, native)?;
                        roots.compatible(root_api, token, class)?;
                        token
                    };
                    value.copy_from_slice(&token.to_le_bytes());
                    Shape::Reference(class)
                }
                value_layout::Shape::Scalar { width, boolean } => Shape::Scalar { width, boolean },
                value_layout::Shape::Inline { width, alignment } => {
                    Shape::Inline { width, alignment }
                }
            };
            records.push((slot.offset, slot.field, shape, value.to_vec()));
        }
        layout.validate(&bytes, root_api, roots)?;
        let view = HostRegion::owned(layout.width)?;
        view.with_native(|p| unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), p.as_ptr(), bytes.len())
        })?;
        memory.map_shared_data(address, view.clone(), READ | WRITE, READ | WRITE)?;
        for (at, leaf, shape, committed) in records {
            self.cells.insert(
                address + at as u64,
                Cell {
                    payload_offset: Some(offset as usize - 16 + at),
                    owner,
                    field: leaf,
                    shape,
                    view: view.clone(),
                    view_offset: at,
                    committed,
                    published: false,
                },
            );
        }
        self.inline_payloads.insert(
            address,
            InlinePayload {
                owner,
                field,
                layout,
                view,
            },
        );
        Ok(())
    }
    fn value(cell: &Cell) -> Result<Vec<u8>, String> {
        cell.view.with_native(|p| unsafe {
            std::slice::from_raw_parts(p.as_ptr().add(cell.view_offset), cell.shape.width())
                .to_vec()
        })
    }
    fn validate(shape: Shape, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() != shape.width()
            || matches!(shape, Shape::Scalar { boolean: true, .. }) && bytes[0] > 1
        {
            return Err("initialized field width/Boolean encoding mismatch".into());
        }
        if let Shape::EnginePointer(expected) = shape {
            if bytes != expected.to_le_bytes() {
                return Err("guest resource pointer changed".into());
            }
        }
        Ok(())
    }
    pub(super) fn unbox(
        &mut self,
        library: Object,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        memory: &mut Memory,
        owner: u64,
        layout: value_layout::Layout,
    ) -> Result<u64, String> {
        if self.retired || !layout.value || self.payloads.len() >= 32768 {
            return Err("unbox outside live bounded value payload registry".into());
        }
        layout.check()?;
        let object = roots.native(root_api, owner)?;
        if roots.class(root_api, owner)? != layout.class {
            return Err("unbox class differs from rooted value layout".into());
        }
        let unbox: unsafe extern "C" fn(Object) -> Object = unsafe {
            let p = libc::dlsym(library, c"mono_object_unbox".as_ptr());
            if p.is_null() {
                return Err("native unbox API absent".into());
            }
            std::mem::transmute(p)
        };
        if unsafe { unbox(object) } != unsafe { object.cast::<u8>().add(16).cast() } {
            return Err("actual native unbox payload layout mismatch".into());
        }
        self.promote_payload(root_api, roots, memory, owner, layout)?;
        self.unboxes += 1;
        Ok(owner + 16)
    }
    pub(super) fn publish_boxed_payload(
        &mut self,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        memory: &mut Memory,
        owner: u64,
        layout: value_layout::Layout,
    ) -> Result<(), String> {
        if self.retired
            || !layout.value
            || !roots.is_boxed(owner)
            || roots.class(root_api, owner)? != layout.class
            || self.payloads.len() >= 32768
        {
            return Err("payload requires live allocated box and exact bounded class".into());
        }
        self.promote_payload(root_api, roots, memory, owner, layout)
    }
    fn promote_payload(
        &mut self,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        memory: &mut Memory,
        owner: u64,
        layout: value_layout::Layout,
    ) -> Result<(), String> {
        layout.check()?;
        if let Some((existing, _)) = self.payloads.get(&owner) {
            if existing.class != layout.class || existing.width != layout.width {
                return Err("unbox changed canonical value layout".into());
            }
            return Ok(());
        }
        let api = self.api.ok_or("unbox before native field API")?;
        let object = roots.native(root_api, owner)?;
        let mut bytes = vec![0; layout.width];
        let mut records = Vec::new();
        for slot in &layout.slots {
            let width = slot.shape.width();
            let value = &mut bytes[slot.offset..slot.offset + width];
            if slot.nested {
                unsafe {
                    ptr::copy_nonoverlapping(
                        object.cast::<u8>().add(16 + slot.offset),
                        value.as_mut_ptr(),
                        width,
                    );
                }
            } else {
                unsafe { (api.get)(object, slot.field, value.as_mut_ptr().cast()) };
            }
            let shape = match slot.shape {
                value_layout::Shape::Reference(class) => {
                    let native = u64::from_le_bytes(value.try_into().unwrap()) as Object;
                    let token = if native.is_null() {
                        0
                    } else {
                        let token = roots.borrow(root_api, native)?;
                        roots.compatible(root_api, token, class)?;
                        token
                    };
                    value.copy_from_slice(&token.to_le_bytes());
                    Shape::Reference(class)
                }
                value_layout::Shape::Scalar { width, boolean } => Shape::Scalar { width, boolean },
                value_layout::Shape::Inline { width, alignment } => {
                    Shape::Inline { width, alignment }
                }
            };
            let address = owner + 16 + slot.offset as u64;
            if let Some(cell) = self.cells.get(&address) {
                if cell.owner != owner
                    || cell.field != slot.field
                    || cell.shape.width() != width
                    || Self::value(cell)? != cell.committed
                    || cell.committed != value
                {
                    return Err("unbox would discard pending or incompatible field snapshot".into());
                }
            }
            records.push((
                address,
                slot.field,
                shape,
                slot.offset,
                value.to_vec(),
                slot.nested.then_some(slot.offset),
            ));
        }
        // Every existing field must correspond exactly to a validated slot.
        if self
            .cells
            .iter()
            .any(|(&address, cell)| cell.owner == owner && !records.iter().any(|r| r.0 == address))
        {
            return Err("unbox layout omits an already published field".into());
        }
        layout.validate(&bytes, root_api, roots)?;
        if self.cells.len()
            + records
                .iter()
                .filter(|r| !self.cells.contains_key(&r.0))
                .count()
            > 32768
        {
            return Err("unbox field capacity exceeded".into());
        }
        let full = HostRegion::owned(layout.width)?;
        full.with_native(|p| unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), p.as_ptr(), bytes.len())
        })?;
        let previous: Vec<_> = self
            .cells
            .iter()
            .filter(|(_, c)| c.owner == owner && c.published)
            .map(|(&a, c)| (a, c.view.clone()))
            .collect();
        for (address, _) in &previous {
            memory.unmap_shared_data(*address)?;
        }
        if let Err(error) =
            memory.map_shared_data(owner + 16, full.clone(), READ | WRITE, READ | WRITE)
        {
            for (address, view) in previous {
                memory.map_shared_data(address, view, READ | WRITE, READ | WRITE)?;
            }
            return Err(error);
        }
        for (address, field, shape, view_offset, committed, payload_offset) in records {
            self.cells.insert(
                address,
                Cell {
                    payload_offset,
                    owner,
                    field,
                    shape,
                    view: full.clone(),
                    view_offset,
                    committed,
                    published: false,
                },
            );
        }
        for (_, view) in previous {
            view.revoke()?;
        }
        self.payloads.insert(owner, (layout, full));
        Ok(())
    }
    pub(super) fn sync(
        &mut self,
        root_api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Err("instance field synchronization after retirement".into());
        }
        let Some(api) = self.api else {
            return Ok(());
        };
        self.validate_payloads(root_api, roots)?;
        let mut updates = Vec::new();
        for (&address, cell) in &self.cells {
            let bytes = Self::value(cell)?;
            Self::validate(cell.shape, &bytes)?;
            let owner = roots.native(root_api, cell.owner)?;
            let reference = if let Shape::Reference(class) = cell.shape {
                let token = u64::from_le_bytes(bytes.as_slice().try_into().unwrap());
                Some(if token == 0 {
                    ptr::null_mut()
                } else {
                    roots.compatible(root_api, token, class)?
                })
            } else {
                None
            };
            if bytes != cell.committed {
                updates.push((address, owner, bytes, reference));
            }
        }
        // Validate the entire batch before touching any native field. Pinned
        // targets stay alive through the native reference write barrier.
        for (address, owner, mut bytes, reference) in updates {
            let cell = self.cells.get_mut(&address).unwrap();
            Self::write_native(api, owner, cell, &mut bytes, reference);
            self.writes += 1;
            self.reference_writes += usize::from(reference.is_some());
            cell.committed = bytes;
        }
        Ok(())
    }
    fn validate_payloads(
        &self,
        api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        for (layout, view) in self
            .payloads
            .values()
            .map(|(l, v)| (l, v))
            .chain(self.inline_payloads.values().map(|p| (&p.layout, &p.view)))
        {
            let bytes = view.with_native(|p| unsafe {
                std::slice::from_raw_parts(p.as_ptr(), layout.width).to_vec()
            })?;
            layout.validate(&bytes, api, roots)?;
        }
        Ok(())
    }
    pub(super) fn verify(
        &mut self,
        root_api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Err("instance field verification after retirement".into());
        }
        let Some(api) = self.api else {
            return Ok(());
        };
        self.validate_payloads(root_api, roots)?;
        for cell in self.cells.values() {
            let owner = roots.native(root_api, cell.owner)?;
            let mut bytes = vec![0; cell.shape.width()];
            Self::read_native(api, owner, cell, &mut bytes);
            if let Shape::Reference(class) = cell.shape {
                let token = u64::from_le_bytes(cell.committed.as_slice().try_into().unwrap());
                let expected = if token == 0 {
                    ptr::null_mut()
                } else {
                    roots.compatible(root_api, token, class)?
                };
                if u64::from_le_bytes(bytes.as_slice().try_into().unwrap()) != expected as u64 {
                    return Err("initialized native reference differs across GC".into());
                }
            } else if bytes != cell.committed {
                return Err("initialized native scalar differs across GC".into());
            }
            if Self::value(cell)? != cell.committed {
                return Err("initialized guest field not synchronized before GC".into());
            }
            self.verified += 1;
        }
        Ok(())
    }
    /// Native references read only from exact fields of this still-rooted owner.
    pub(super) fn native_references(
        &self,
        root_api: instances::Api,
        roots: &instances::Registry,
        owner: u64,
    ) -> Result<Vec<Object>, String> {
        if self.retired {
            return Err("native field graph after retirement".into());
        }
        let api = self.api.ok_or("native field graph before installation")?;
        let object = roots.native(root_api, owner)?;
        let mut references = Vec::new();
        for cell in self
            .cells
            .values()
            .filter(|c| c.owner == owner && matches!(c.shape, Shape::Reference(_)))
        {
            let mut bytes = [0; 8];
            Self::read_native(api, object, cell, &mut bytes);
            let value = u64::from_le_bytes(bytes) as Object;
            if !value.is_null() {
                references.push(value);
            }
        }
        Ok(references)
    }
    /// After an admitted managed call, rebuild exact snapshots from native
    /// fields. Reject unsynchronized guest writes before replacing any cell.
    #[cfg(test)]
    pub(super) fn refresh(
        &mut self,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        owner: u64,
    ) -> Result<(), String> {
        let plan = self.prepare_refresh(root_api, roots, Some(owner), |roots, native| {
            roots.borrow(root_api, native)
        })?;
        self.commit_refresh(plan)
    }
    /// Validate the entire selected field batch before changing guest snapshots.
    pub(super) fn prepare_refresh(
        &self,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        owner: Option<u64>,
        mut reference: impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<Refresh, String> {
        if self.retired {
            return Err("native field refresh after retirement".into());
        }
        let api = self.api.ok_or("native field refresh before installation")?;
        self.validate_payloads(root_api, roots)?;
        for cell in self
            .cells
            .values()
            .filter(|c| owner.is_none_or(|o| c.owner == o))
        {
            if Self::value(cell)? != cell.committed {
                return Err("field refresh would discard pending guest write".into());
            }
        }
        let mut updates = Vec::new();
        let mut references = Vec::new();
        for (&address, cell) in self
            .cells
            .iter()
            .filter(|(_, c)| owner.is_none_or(|o| c.owner == o))
        {
            let object = roots.native(root_api, cell.owner)?;
            let mut bytes = vec![0; cell.shape.width()];
            Self::read_native(api, object, cell, &mut bytes);
            if let Shape::Reference(class) = cell.shape {
                let native = u64::from_le_bytes(bytes.as_slice().try_into().unwrap()) as Object;
                let token = if native.is_null() {
                    0
                } else {
                    let token = reference(roots, native)?;
                    roots.compatible(root_api, token, class)?;
                    token
                };
                bytes.copy_from_slice(&token.to_le_bytes());
                if bytes != cell.committed && !native.is_null() {
                    references.push(native);
                }
            }
            Self::validate(cell.shape, &bytes)?;
            if bytes != cell.committed {
                updates.push((address, bytes));
            }
        }
        Ok(Refresh {
            updates,
            references,
        })
    }
    pub(super) fn commit_refresh(&mut self, plan: Refresh) -> Result<(), String> {
        for (address, bytes) in plan.updates {
            let cell = self
                .cells
                .get_mut(&address)
                .ok_or("field refresh layout changed")?;
            cell.view.with_native(|p| unsafe {
                ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    p.as_ptr().add(cell.view_offset),
                    bytes.len(),
                );
            })?;
            cell.committed = bytes;
            self.refreshed += 1;
        }
        Ok(())
    }
    pub(super) fn has_owner(&self, owner: u64) -> bool {
        !self.retired && self.cells.values().any(|c| c.owner == owner)
    }
    pub(super) fn has_reference(&self, owner: u64, address: u64) -> bool {
        !self.retired
            && self.cells.get(&address).is_some_and(|c| {
                (owner == 0 || c.owner == owner) && matches!(c.shape, Shape::Reference(_))
            })
    }
    pub(super) fn store_reference(
        &mut self,
        root_api: instances::Api,
        roots: &instances::Registry,
        memory: &mut Memory,
        owner: u64,
        address: u64,
        token: u64,
    ) -> Result<(), String> {
        if !self.has_reference(owner, address) {
            return Err("barrier destination is not an owned reference field".into());
        }
        let Shape::Reference(class) = self.cells[&address].shape else {
            unreachable!()
        };
        if token != 0 {
            roots.compatible(root_api, token, class)?;
        }
        let changed = self.cells[&address].committed != token.to_le_bytes();
        memory.write_int(address, 8, token)?;
        self.sync(root_api, roots)?;
        if !changed {
            let cell = &self.cells[&address];
            let native = roots.native(root_api, cell.owner)?;
            let value = if token == 0 {
                ptr::null_mut()
            } else {
                roots.compatible(root_api, token, class)?
            };
            Self::write_native(
                self.api.unwrap(),
                native,
                cell,
                &mut token.to_le_bytes(),
                Some(value),
            );
            self.writes += 1;
            self.reference_writes += 1;
        }
        Ok(())
    }
    /// The embedding API takes reference values directly, but scalar/inline
    /// values by address. Resolve the shape from this exact owned field cell.
    pub(super) fn store_value(
        &mut self,
        root_api: instances::Api,
        roots: &instances::Registry,
        memory: &mut Memory,
        owner: u64,
        field: Object,
        value: u64,
    ) -> Result<(), String> {
        if self.retired {
            return Err("field write after retirement".into());
        }
        if let Some((&address, payload)) = self
            .inline_payloads
            .iter()
            .find(|(_, p)| p.owner == owner && p.field == field)
        {
            let api = self.api.ok_or("nested field write before installation")?;
            if unsafe { (api.flags)(field) } & (0x10 | 0x20 | 0x40) != 0 {
                return Err("nested field API write requires mutable instance storage".into());
            }
            let mut bytes = vec![0; payload.layout.width];
            if value != 0 {
                if !value.is_multiple_of(payload.layout.alignment as u64)
                    || !memory.owned_noncode_access(value, bytes.len())
                {
                    return Err("nested field API input is not aligned owned data".into());
                }
                memory.read(value, &mut bytes)?;
            }
            payload.layout.validate(&bytes, root_api, roots)?;
            memory.write(address, &bytes)?;
            return self.sync(root_api, roots);
        }
        let (&address, cell) = self
            .cells
            .iter()
            .find(|(_, c)| c.owner == owner && c.field == field)
            .ok_or("unadmitted initialized field write")?;
        if cell.payload_offset.is_some() {
            return Err("embedded value leaf is not a field of the complete boxed receiver".into());
        }
        let api = self.api.ok_or("field write before installation")?;
        if unsafe { (api.flags)(field) } & (0x10 | 0x20 | 0x40) != 0 {
            return Err("field API write requires mutable instance storage".into());
        }
        if matches!(cell.shape, Shape::Reference(_)) {
            return self.store_reference(root_api, roots, memory, owner, address, value);
        }
        if matches!(cell.shape, Shape::EnginePointer(_)) {
            return Err("field API cannot replace owned engine resources".into());
        }
        let width = cell.shape.width();
        let alignment = match cell.shape {
            Shape::Inline { alignment, .. } => alignment,
            _ => width,
        };
        let mut bytes = vec![0; width];
        if value != 0 {
            if !value.is_multiple_of(alignment as u64) || !memory.owned_noncode_access(value, width)
            {
                return Err("field API scalar input is not aligned owned data".into());
            }
            memory.read(value, &mut bytes)?;
        }
        Self::validate(cell.shape, &bytes)?;
        memory.write(address, &bytes)?;
        self.sync(root_api, roots)
    }
    pub(super) fn read(&self, owner: u64, field: Object) -> Result<Vec<u8>, String> {
        if self.retired {
            return Err("field read after retirement".into());
        }
        if let Some(payload) = self
            .inline_payloads
            .values()
            .find(|p| p.owner == owner && p.field == field)
        {
            return payload.view.with_native(|p| unsafe {
                std::slice::from_raw_parts(p.as_ptr(), payload.layout.width).to_vec()
            });
        }
        let cell = self
            .cells
            .values()
            .find(|c| c.owner == owner && c.field == field)
            .ok_or("unadmitted initialized field read")?;
        if cell.payload_offset.is_some() {
            return Err("embedded value leaf needs its actual boxed parent field ABI".into());
        }
        Self::value(cell)
    }
    pub(super) fn retire(
        &mut self,
        memory: &mut Memory,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        if !self.cells.is_empty() && !roots.roots_released() {
            return Err("instance field views retired before strong roots".into());
        }
        for (&address, cell) in &mut self.cells {
            if cell.published {
                memory.unmap_shared_data(address)?;
                cell.published = false;
            }
            if !self.payloads.contains_key(&cell.owner)
                && !self
                    .inline_payloads
                    .values()
                    .any(|p| p.owner == cell.owner && p.view.same_storage(&cell.view))
            {
                cell.view.revoke()?;
            }
        }
        for (&owner, (_, view)) in &self.payloads {
            memory.unmap_shared_data(owner + 16)?;
            view.revoke()?;
        }
        for (&address, payload) in &self.inline_payloads {
            memory.unmap_shared_data(address)?;
            payload.view.revoke()?;
        }
        self.retired = true;
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-instance-field-cells={}\nnative-instance-field-writes={}\nnative-instance-reference-writes={}\nnative-instance-field-gc-verifications={}\nnative-instance-field-views-retired={}\nnative-instance-field-refreshes={}\n", self.cells.len(), self.writes, self.reference_writes, self.verified, self.retired, self.refreshed) + &format!("native-value-unbox-calls={}\nnative-value-payloads={}\n",self.unboxes,self.payloads.len())
    }
}
pub(super) fn is_component(library: Object, render: Object, class: Object) -> Result<bool, String> {
    let api = Api::load(library)?;
    let find: unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object = unsafe {
        let p = libc::dlsym(library, c"mono_class_from_name".as_ptr());
        if p.is_null() {
            return Err("component base lookup absent".into());
        }
        std::mem::transmute(p)
    };
    let base = unsafe {
        find(
            render,
            c"UnityEngine.Rendering".as_ptr(),
            c"VolumeComponent".as_ptr(),
        )
    };
    if base.is_null() || class.is_null() {
        return Err("component base/class absent".into());
    }
    Ok(unsafe { (api.assignable)(base, class) } != 0)
}
/// Enumerate only a locked VolumeComponent's declared parameters and their
/// initialized scalar/nullable fields. Arbitrary heap graph traversal is refused.
pub(super) fn component_specs(
    library: Object,
    images: [Object; 3],
    object: Object,
    class: Object,
) -> Result<Vec<Spec>, String> {
    macro_rules! sym {
        ($name:literal, $ty:ty) => {{
            let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
            if p.is_null() {
                return Err(concat!("missing component field symbol: ", $name).into());
            }
            unsafe { std::mem::transmute::<Object, $ty>(p) }
        }};
    }
    let api = Api::load(library)?;
    let find = sym!(
        "mono_class_from_name",
        unsafe extern "C" fn(Object, *const c_char, *const c_char) -> Object
    );
    let fields = sym!(
        "mono_class_get_fields",
        unsafe extern "C" fn(Object, *mut Object) -> Object
    );
    let parent = sym!(
        "mono_class_get_parent",
        unsafe extern "C" fn(Object) -> Object
    );
    let class_image = sym!(
        "mono_class_get_image",
        unsafe extern "C" fn(Object) -> Object
    );
    let field_name = sym!(
        "mono_field_get_name",
        unsafe extern "C" fn(Object) -> *const c_char
    );
    let object_class = sym!(
        "mono_object_get_class",
        unsafe extern "C" fn(Object) -> Object
    );
    let enum_class = sym!("mono_class_is_enum", unsafe extern "C" fn(Object) -> i32);
    let enum_base = sym!(
        "mono_class_enum_basetype",
        unsafe extern "C" fn(Object) -> Object
    );
    let value_size = sym!(
        "mono_class_value_size",
        unsafe extern "C" fn(Object, *mut u32) -> i32
    );
    let [core, render, urp] = images;
    let parameter = unsafe {
        find(
            render,
            c"UnityEngine.Rendering".as_ptr(),
            c"VolumeParameter".as_ptr(),
        )
    };
    if parameter.is_null() || object.is_null() || unsafe { object_class(object) } != class {
        return Err("component graph has no exact live root/parameter class".into());
    }
    let mut inline_classes = Vec::new();
    for (name, names) in [
        (c"Vector2", &[c"x", c"y"][..]),
        (c"Vector3", &[c"x", c"y", c"z"][..]),
        (c"Vector4", &[c"x", c"y", c"z", c"w"][..]),
        (c"Color", &[c"r", c"g", c"b", c"a"][..]),
    ] {
        let class = unsafe { find(core, c"UnityEngine".as_ptr(), name.as_ptr()) };
        if class.is_null() {
            return Err("locked inline value class absent".into());
        }
        let mut alignment = 0;
        let width = unsafe { value_size(class, &mut alignment) };
        if width != names.len() as i32 * 4 || alignment != 4 {
            return Err("inline value size/alignment mismatch".into());
        }
        for (index, name) in names.iter().enumerate() {
            let field = unsafe { (api.find)(class, name.as_ptr()) };
            if field.is_null()
                || unsafe { (api.flags)(field) } & (0x10 | 0x40) != 0
                || unsafe { (api.kind)((api.ty)(field)) } != 0xc
                || unsafe { (api.byref)((api.ty)(field)) } != 0
                || unsafe { (api.offset)(field) } != 16 + index as i32 * 4
            {
                return Err("inline value float field layout mismatch".into());
            }
        }
        inline_classes.push((class, width as usize));
    }
    let spec = |owner, field| -> Result<Spec, String> {
        let ty = unsafe { (api.ty)(field) };
        if ty.is_null() || unsafe { (api.byref)(ty) } != 0 {
            return Err("component field has unknown/byref type".into());
        }
        let kind = unsafe { (api.kind)(ty) };
        let target = if matches!(kind, 0x11 | 0x12) {
            Some(unsafe { (api.type_class)(ty) })
        } else {
            None
        };
        if target.is_some_and(|c| c.is_null()) {
            return Err("component field type class absent".into());
        }
        let inline = if kind == 0x11 {
            let class = target.unwrap();
            if unsafe { enum_class(class) } != 0 {
                if ![core, render, urp].contains(&unsafe { class_image(class) }) {
                    return Err("enum field outside locked images".into());
                }
                let base = unsafe { enum_base(class) };
                let mut alignment = 0;
                if base.is_null()
                    || !matches!(unsafe { (api.kind)(base) }, 8 | 9)
                    || unsafe { value_size(class, &mut alignment) } != 4
                    || alignment != 4
                {
                    return Err("enum field is not an exact four-byte value".into());
                }
                Some((4, 4))
            } else {
                Some((
                    inline_classes
                        .iter()
                        .find(|(c, _)| *c == class)
                        .ok_or("unadmitted inline struct field")?
                        .1,
                    4,
                ))
            }
        } else {
            None
        };
        let name = unsafe { field_name(field) };
        if name.is_null() {
            return Err("component field has no name".into());
        }
        Ok(Spec {
            owner,
            name: unsafe { CStr::from_ptr(name) }.to_owned(),
            kind,
            type_class: target,
            guest_pointer: None,
            inline,
            nested: None,
        })
    };
    let mut specs = vec![Spec {
        owner: object,
        name: c"active".to_owned(),
        kind: 2,
        type_class: None,
        guest_pointer: None,
        inline: None,
        nested: None,
    }];
    let mut iterator = ptr::null_mut();
    let mut count = 0;
    loop {
        let field = unsafe { fields(class, &mut iterator) };
        if field.is_null() {
            break;
        }
        count += 1;
        if count > 64 {
            return Err("component declared field capacity exceeded".into());
        }
        if unsafe { (api.flags)(field) } & (0x10 | 0x40) != 0 {
            continue;
        }
        let root_field = spec(object, field)?;
        let Some(target) = root_field.type_class else {
            return Err("component root field is not a parameter reference".into());
        };
        if root_field.kind != 0x12 || unsafe { (api.assignable)(parameter, target) } == 0 {
            return Err("component root field is not a VolumeParameter".into());
        }
        let mut child: Object = ptr::null_mut();
        unsafe {
            (api.get)(object, field, (&mut child as *mut Object).cast());
        }
        if child.is_null() || unsafe { object_class(child) } != target {
            return Err("component constructor did not initialize exact parameter class".into());
        }
        specs.push(root_field);
        let mut declaring = target;
        for depth in 0..8 {
            if declaring.is_null() {
                return Err("parameter parent chain ended early".into());
            }
            if ![render, urp].contains(&unsafe { class_image(declaring) }) {
                return Err("parameter class outside locked render images".into());
            }
            let mut iterator = ptr::null_mut();
            let mut child_count = 0;
            loop {
                let field = unsafe { fields(declaring, &mut iterator) };
                if field.is_null() {
                    break;
                }
                child_count += 1;
                if child_count > 32 {
                    return Err("parameter field capacity exceeded".into());
                }
                if unsafe { (api.flags)(field) } & (0x10 | 0x40) != 0 {
                    continue;
                }
                let child_field = spec(child, field)?;
                if child_field.kind == 0x12 {
                    let mut native: Object = ptr::null_mut();
                    unsafe {
                        (api.get)(child, field, (&mut native as *mut Object).cast());
                    }
                    if !native.is_null() {
                        return Err("non-null nested parameter reference needs a separate checked graph adapter".into());
                    }
                }
                specs.push(child_field);
            }
            if declaring == parameter {
                break;
            }
            if depth == 7 {
                return Err("parameter inheritance depth exceeded".into());
            }
            declaring = unsafe { parent(declaring) };
        }
    }
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell as Counter, RefCell};
    #[test]
    fn nested_instance_fields_keep_padding_private_and_barrier_complete_owner() {
        let (fixture, _, _, _, _) = fixture();
        let api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let mut storage = [0u64; 8];
        let native = storage.as_mut_ptr().cast::<std::ffi::c_void>();
        {
            let bytes = unsafe { std::slice::from_raw_parts_mut(native.cast::<u8>(), 64) };
            bytes[16..40].fill(0xcc);
            bytes[16] = 1;
            bytes[24..32].copy_from_slice(&4u64.to_le_bytes());
            bytes[32..36].copy_from_slice(&7u32.to_le_bytes());
        }
        let owner = roots.borrow(api, native).unwrap();
        let class = 2usize as Object;
        let layout = value_layout::Layout {
            class,
            width: 24,
            alignment: 8,
            value: true,
            slots: vec![
                value_layout::Slot {
                    nested: false,
                    offset: 0,
                    field: 1usize as Object,
                    shape: value_layout::Shape::Scalar {
                        width: 1,
                        boolean: true,
                    },
                },
                value_layout::Slot {
                    nested: false,
                    offset: 8,
                    field: 2usize as Object,
                    shape: value_layout::Shape::Reference(class),
                },
                value_layout::Slot {
                    nested: false,
                    offset: 16,
                    field: 3usize as Object,
                    shape: value_layout::Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                },
            ],
        };
        let field = 10usize as Object;
        let mut fields = Registry {
            api: fixture.api,
            ..Registry::default()
        };
        let mut memory = Memory::default();
        fields
            .install_inline(
                api,
                &mut roots,
                &mut memory,
                (owner, field),
                layout.clone(),
                &mut |roots, native| roots.borrow(api, native),
            )
            .unwrap();
        let initial = fields.read(owner, field).unwrap();
        assert_eq!(&initial[1..8], &[0; 7]);
        assert_eq!(&initial[20..24], &[0; 4]);
        assert_ne!(u64::from_le_bytes(initial[8..16].try_into().unwrap()), 4);
        assert!(fields
            .install_inline(
                api,
                &mut roots,
                &mut memory,
                (owner, field),
                layout,
                &mut |roots, native| roots.borrow(api, native)
            )
            .is_err());
        // A later invalid Boolean/padding/reference must reject earlier scalar
        // edits too, before changing any native field or running a barrier.
        memory.write_int(owner + 32, 4, 99).unwrap();
        for (at, width, bad) in [(16, 1, 2), (17, 1, 0xaa), (24, 8, 0xdeadbeef)] {
            let mut previous = vec![0; width];
            memory.read(owner + at, &mut previous).unwrap();
            memory.write_int(owner + at, width, bad).unwrap();
            assert!(fields.sync(api, &roots).is_err());
            assert_eq!(storage[4] as u32, 7);
            memory.write(owner + at, &previous).unwrap();
        }
        memory.write_int(owner + 24, 8, 0).unwrap();
        fields.sync(api, &roots).unwrap();
        assert_eq!(storage[4] as u32, 99);
        assert_eq!(storage[3], 0);
        assert_eq!(BARRIER_OWNER.with(Counter::get), native as usize);
        assert_eq!((storage[2] >> 8) & 255, 0xcc);
        fields.verify(api, &roots).unwrap();
        // Exercise the actual whole-field C API, rather than treating a leaf
        // MonoClassField as if it were declared by the complete receiver.
        memory.map(0x2000, 4096, READ | WRITE).unwrap();
        let mut whole = fields.read(owner, field).unwrap();
        whole[0] = 0;
        whole[16..20].copy_from_slice(&123u32.to_le_bytes());
        memory.write(0x2000, &whole).unwrap();
        fields
            .store_value(api, &roots, &mut memory, owner, field, 0x2000)
            .unwrap();
        assert_eq!(storage[2] as u8, 0);
        assert_eq!(storage[4] as u32, 123);
        storage[2] = (storage[2] & !255) | 1;
        assert_eq!(storage[2] as u8, 1);
        fields.refresh(api, &mut roots, owner).unwrap();
        assert_eq!(fields.read(owner, field).unwrap()[0], 1);
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(owner + 16, 1).is_err());
    }
    thread_local! {
        static SCALAR: Counter<u8> = const { Counter::new(1) };
        static REFERENCE: Counter<usize> = const { Counter::new(4) };
        static SETS: Counter<usize> = const { Counter::new(0) };
        static BARRIER_OWNER: Counter<usize> = const { Counter::new(0) };
        static INLINE: RefCell<[u8; 16]> = const { RefCell::new([0; 16]) };
    }
    unsafe extern "C" fn get(_: Object, field: Object, dest: Object) {
        unsafe {
            if field as usize == 3 {
                INLINE
                    .with(|c| ptr::copy_nonoverlapping(c.borrow().as_ptr(), dest.cast::<u8>(), 16));
            } else if field as usize == 1 {
                dest.cast::<u8>().write(SCALAR.with(Counter::get));
            } else {
                dest.cast::<usize>()
                    .write_unaligned(REFERENCE.with(Counter::get));
            }
        }
    }
    unsafe extern "C" fn scalar_size(_: Object, alignment: *mut u32) -> i32 {
        unsafe { alignment.write(4) };
        4
    }
    unsafe extern "C" fn not_value(_: Object) -> i32 {
        0
    }
    unsafe extern "C" fn set(_: Object, field: Object, value: Object) {
        if field as usize == 3 {
            INLINE.with(|c| {
                c.borrow_mut()
                    .copy_from_slice(unsafe { std::slice::from_raw_parts(value.cast::<u8>(), 16) })
            });
        } else if field as usize == 1 {
            SCALAR.with(|c| c.set(unsafe { value.cast::<u8>().read() }));
        } else {
            // Reference C API values must be objects themselves, not &object.
            assert!(value.is_null() || value as usize == 4);
            REFERENCE.with(|c| c.set(value as usize));
        }
        SETS.with(|c| c.set(c.get() + 1));
    }
    unsafe extern "C" fn find(_: Object, _: *const c_char) -> Object {
        1usize as Object
    }
    unsafe extern "C" fn pointer(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn number(_: Object) -> u32 {
        0
    }
    unsafe extern "C" fn offset(_: Object) -> i32 {
        16
    }
    unsafe extern "C" fn size(_: Object) -> i32 {
        64
    }
    unsafe extern "C" fn assignable(_: Object, _: Object) -> i32 {
        1
    }
    fn fixture() -> (Registry, instances::Api, instances::Registry, Memory, u64) {
        fixture_with(instances::tests::api())
    }
    fn fixture_with(
        api: instances::Api,
    ) -> (Registry, instances::Api, instances::Registry, Memory, u64) {
        SCALAR.with(|c| c.set(1));
        REFERENCE.with(|c| c.set(4));
        SETS.with(|c| c.set(0));
        let mut roots = instances::Registry::default();
        let owner = roots.borrow(api, 4usize as Object).unwrap();
        let mut fields = Registry {
            api: Some(Api {
                find,
                parent: pointer,
                flags: number,
                offset,
                ty: pointer,
                kind: number,
                byref: number,
                type_class: pointer,
                is_value: not_value,
                value_size: scalar_size,
                instance_size: size,
                assignable,
                get,
                set,
                barrier: {
                    unsafe extern "C" fn barrier(
                        owner: Object,
                        destination: Object,
                        reference: Object,
                    ) {
                        BARRIER_OWNER.with(|c| c.set(owner as usize));
                        unsafe {
                            destination.cast::<Object>().write_unaligned(reference);
                        }
                        SETS.with(|c| c.set(c.get() + 1));
                    }
                    barrier
                },
            }),
            ..Registry::default()
        };
        let mut memory = Memory::default();
        for (offset, field, shape, bytes) in [
            (
                16,
                1,
                Shape::Scalar {
                    width: 1,
                    boolean: true,
                },
                vec![1],
            ),
            (
                24,
                2,
                Shape::Reference(2usize as Object),
                owner.to_le_bytes().to_vec(),
            ),
        ] {
            let view = HostRegion::owned(bytes.len()).unwrap();
            view.with_native(|p| unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), p.as_ptr(), bytes.len())
            })
            .unwrap();
            memory
                .map_shared_data(owner + offset, view.clone(), READ | WRITE, READ | WRITE)
                .unwrap();
            fields.cells.insert(
                owner + offset,
                Cell {
                    payload_offset: None,
                    owner,
                    field: field as Object,
                    shape,
                    view,
                    view_offset: 0,
                    committed: bytes,
                    published: true,
                },
            );
        }
        (fields, api, roots, memory, owner)
    }
    #[test]
    fn nested_box_leaves_keep_complete_gc_owner_and_reject_dirty_padding_before_native_stores() {
        let (mut fields, _, _, _, _) = fixture();
        fields.cells.clear();
        let api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let mut object = [0u8; 48];
        let native = object.as_mut_ptr().cast();
        object[17..24].fill(0xcc);
        object[24..32].copy_from_slice(&(native as usize as u64).to_le_bytes());
        let owner = roots.borrow(api, native).unwrap();
        let layout = value_layout::Layout {
            class: 2usize as Object,
            width: 24,
            alignment: 8,
            value: true,
            slots: vec![
                value_layout::Slot {
                    nested: true,
                    offset: 0,
                    field: 1usize as Object,
                    shape: value_layout::Shape::Scalar {
                        width: 1,
                        boolean: true,
                    },
                },
                value_layout::Slot {
                    nested: true,
                    offset: 8,
                    field: 2usize as Object,
                    shape: value_layout::Shape::Reference(2usize as Object),
                },
            ],
        };
        let mut memory = Memory::default();
        fields
            .promote_payload(api, &mut roots, &mut memory, owner, layout)
            .unwrap();
        assert_eq!(memory.read_int(owner + 24, 8).unwrap(), owner);
        assert_eq!(memory.read_int(owner + 17, 1).unwrap(), 0);
        assert!(memory.read_int(owner, 8).is_err());
        assert!(fields.read(owner, 1usize as Object).is_err());
        assert!(fields
            .store_value(
                api,
                &roots,
                &mut memory,
                owner,
                1usize as Object,
                owner + 16
            )
            .is_err());
        memory.write_int(owner + 16, 1, 1).unwrap();
        memory.write_int(owner + 24, 8, 0).unwrap();
        fields.sync(api, &roots).unwrap();
        assert_eq!(object[16], 1);
        assert_eq!(u64::from_le_bytes(object[24..32].try_into().unwrap()), 0);
        assert_eq!(BARRIER_OWNER.with(Counter::get), native as usize);
        assert_eq!(&object[17..24], &[0xcc; 7]);
        fields.verify(api, &roots).unwrap();
        memory.write_int(owner + 16, 1, 0).unwrap();
        memory.write_int(owner + 18, 1, 1).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(object[16], 1);
        memory.write_int(owner + 18, 1, 0).unwrap();
        memory.write_int(owner + 16, 1, 2).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(object[16], 1);
    }
    #[test]
    fn unbox_promotes_fields_to_one_writable_payload_and_keeps_tokens_private() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        let layout = value_layout::Layout {
            class: 2usize as Object,
            width: 16,
            alignment: 8,
            value: true,
            slots: vec![
                value_layout::Slot {
                    nested: false,
                    offset: 0,
                    field: 1usize as Object,
                    shape: value_layout::Shape::Scalar {
                        width: 1,
                        boolean: true,
                    },
                },
                value_layout::Slot {
                    nested: false,
                    offset: 8,
                    field: 2usize as Object,
                    shape: value_layout::Shape::Reference(2usize as Object),
                },
            ],
        };
        fields
            .promote_payload(api, &mut roots, &mut memory, owner, layout.clone())
            .unwrap();
        fields
            .promote_payload(api, &mut roots, &mut memory, owner, layout)
            .unwrap();
        let mut bytes = [0; 16];
        memory.read(owner + 16, &mut bytes).unwrap();
        assert_eq!(bytes[0], 1);
        assert_eq!(u64::from_le_bytes(bytes[8..].try_into().unwrap()), owner);
        assert!(memory.read_int(owner, 8).is_err());
        memory.write_int(owner + 16, 1, 0).unwrap();
        memory.write_int(owner + 24, 8, owner + 8).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(SETS.with(Counter::get), 0);
        memory.write_int(owner + 24, 8, 0).unwrap();
        memory.write_int(owner + 17, 1, 1).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(SETS.with(Counter::get), 0);
        memory.write_int(owner + 17, 1, 0).unwrap();
        fields.sync(api, &roots).unwrap();
        fields.verify(api, &roots).unwrap();
        assert_eq!(SCALAR.with(Counter::get), 0);
        assert_eq!(REFERENCE.with(Counter::get), 0);
        assert_eq!(
            fields.read(owner, 2usize as Object).unwrap(),
            0u64.to_le_bytes()
        );
        assert!(fields.retire(&mut memory, &roots).is_err());
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(owner + 16, 8).is_err());
    }
    #[test]
    fn scalar_and_reference_updates_use_typed_api_and_survive_checked_verification() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        assert!(memory.read_int(owner, 8).is_err());
        assert!(memory.write_int(owner + 16, 2, 0).is_err());
        assert!(memory.read_int(owner + 17, 1).is_err());
        fields.verify(api, &roots).unwrap();
        memory.write_int(owner + 16, 1, 0).unwrap();
        fields
            .store_reference(api, &roots, &mut memory, owner, owner + 24, 0)
            .unwrap();
        assert_eq!(SCALAR.with(Counter::get), 0);
        assert_eq!(REFERENCE.with(Counter::get), 0);
        fields.verify(api, &roots).unwrap();
        fields
            .store_reference(api, &roots, &mut memory, owner, owner + 24, owner)
            .unwrap();
        assert_eq!(REFERENCE.with(Counter::get), 4);
        fields.verify(api, &roots).unwrap();
        assert!(fields.retire(&mut memory, &roots).is_err());
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(owner + 24, 8).is_err());
        assert!(fields.sync(api, &roots).is_err());
        assert!(fields.read(owner, 2usize as Object).is_err());
    }
    #[test]
    fn embedding_field_api_distinguishes_direct_refs_from_scalar_addresses() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        memory.map(0x1000, 4096, READ | WRITE).unwrap();
        memory.write_int(0x1000, 1, 2).unwrap();
        assert!(fields
            .store_value(api, &roots, &mut memory, owner, 1usize as Object, 0x1000)
            .is_err());
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 1);
        assert_eq!(SETS.with(Counter::get), 0);
        memory.write_int(0x1000, 1, 0).unwrap();
        fields
            .store_value(api, &roots, &mut memory, owner, 1usize as Object, 0x1000)
            .unwrap();
        assert_eq!(SCALAR.with(Counter::get), 0);
        // The token has no readable object header. A pointer-to-token is not a
        // valid reference input to Mono's embedding field setter.
        fields
            .store_value(api, &roots, &mut memory, owner, 2usize as Object, 0)
            .unwrap();
        fields
            .store_value(api, &roots, &mut memory, owner, 2usize as Object, owner)
            .unwrap();
        assert_eq!(REFERENCE.with(Counter::get), 4);
        memory.write_int(0x1000, 8, owner).unwrap();
        assert!(fields
            .store_value(api, &roots, &mut memory, owner, 2usize as Object, 0x1000)
            .is_err());
        assert_eq!(REFERENCE.with(Counter::get), 4);
        unsafe extern "C" fn readonly(_: Object) -> u32 {
            0x20
        }
        fields.api.as_mut().unwrap().flags = readonly;
        assert!(fields
            .store_value(api, &roots, &mut memory, owner, 2usize as Object, 0)
            .is_err());
        fields.verify(api, &roots).unwrap();
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
        assert!(fields
            .store_value(api, &roots, &mut memory, owner, 1usize as Object, 0)
            .is_err());
    }
    #[test]
    fn graph_refresh_includes_child_owner_and_late_dirty_cell_rejects_entire_batch() {
        let (mut fields, api, mut roots, mut memory, owner) =
            fixture_with(instances::tests::identity_api());
        let child = roots.borrow(api, 8usize as Object).unwrap();
        let view = HostRegion::owned(1).unwrap();
        view.with_native(|p| unsafe { p.as_ptr().write(1) })
            .unwrap();
        memory
            .map_shared_data(child + 16, view.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        fields.cells.insert(
            child + 16,
            Cell {
                payload_offset: None,
                owner: child,
                field: 1usize as Object,
                shape: Shape::Scalar {
                    width: 1,
                    boolean: true,
                },
                view,
                view_offset: 0,
                committed: vec![1],
                published: true,
            },
        );
        SCALAR.with(|c| c.set(0));
        REFERENCE.with(|c| c.set(0));
        memory.write_int(child + 16, 1, 0).unwrap();
        assert!(fields
            .prepare_refresh(api, &mut roots, None, |roots, native| roots
                .borrow(api, native))
            .is_err());
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 1);
        memory.write_int(child + 16, 1, 1).unwrap();
        let plan = fields
            .prepare_refresh(api, &mut roots, None, |roots, native| {
                roots.borrow(api, native)
            })
            .unwrap();
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 1);
        fields.commit_refresh(plan).unwrap();
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 0);
        assert_eq!(memory.read_int(child + 16, 1).unwrap(), 0);
        fields.verify(api, &roots).unwrap();
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
    }
    #[test]
    fn managed_refresh_keeps_native_stores_and_rejects_pending_guest_writes_and_late_bad_roots() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        SCALAR.with(|s| s.set(0));
        REFERENCE.with(|s| s.set(8));
        assert!(fields.refresh(api, &mut roots, owner).is_err());
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 1);
        REFERENCE.with(|s| s.set(0));
        memory.write_int(owner + 24, 8, 0).unwrap();
        assert!(fields.refresh(api, &mut roots, owner).is_err());
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 1);
        memory.write_int(owner + 24, 8, owner).unwrap();
        fields.refresh(api, &mut roots, owner).unwrap();
        assert_eq!(memory.read_int(owner + 16, 1).unwrap(), 0);
        assert_eq!(memory.read_int(owner + 24, 8).unwrap(), 0);
        assert_eq!(SETS.with(Counter::get), 0);
        fields.verify(api, &roots).unwrap();
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
    }
    #[test]
    fn inline_float_value_keeps_negative_zero_nan_payload_and_all_lanes_across_sync() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        INLINE.with(|c| c.borrow_mut().fill(0));
        let view = HostRegion::owned(16).unwrap();
        memory
            .map_shared_data(owner + 32, view.clone(), READ | WRITE, READ | WRITE)
            .unwrap();
        fields.cells.insert(
            owner + 32,
            Cell {
                payload_offset: None,
                owner,
                field: 3usize as Object,
                shape: Shape::Inline {
                    width: 16,
                    alignment: 4,
                },
                view,
                view_offset: 0,
                committed: vec![0; 16],
                published: true,
            },
        );
        let bits: Vec<u8> = [0x80000000u32, 0x7fc01234, 0x3f800000, 0xbf800000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        memory.write(owner + 32, &bits).unwrap();
        fields.sync(api, &roots).unwrap();
        fields.verify(api, &roots).unwrap();
        assert_eq!(INLINE.with(|c| c.borrow().to_vec()), bits);
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(owner + 32, 8).is_err());
    }
    #[test]
    fn invalid_late_reference_prevents_earlier_scalar_store_and_preserves_failed_output() {
        let (mut fields, api, mut roots, mut memory, owner) = fixture();
        memory.write_int(owner + 16, 1, 0).unwrap();
        memory.write_int(owner + 24, 8, owner + 8).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(SCALAR.with(Counter::get), 1);
        assert_eq!(SETS.with(Counter::get), 0);
        memory.write_int(owner + 24, 8, owner).unwrap();
        memory.write_int(owner + 16, 1, 2).unwrap();
        assert!(fields.sync(api, &roots).is_err());
        assert_eq!(SETS.with(Counter::get), 0);
        assert!(!fields.has_reference(owner + 1, owner + 24));
        assert!(!fields.has_reference(owner, owner + 25));
        assert!(fields
            .store_reference(api, &roots, &mut memory, owner, owner + 24, 4)
            .is_err());
        assert_eq!(memory.read_int(owner + 24, 8).unwrap(), owner);
        roots.release(api);
        fields.retire(&mut memory, &roots).unwrap();
    }
}
