//! Bounded typed SZ arrays. Value elements use metadata-derived scalar and
//! reference slots; native pointers never enter the owned guest payload.
use super::{instances, value_layout, Memory, Object};
use crate::x64::memory::{shared::HostRegion, READ, WRITE};
use std::{collections::BTreeMap, ptr};
const BASE: u64 = 0x6ffeb0000000;
const STRIDE: u64 = 65536;
fn validate_engine_handles(bytes: &[u8], mut live: impl FnMut(u64) -> bool) -> Result<(), String> {
    if !bytes.len().is_multiple_of(8) {
        return Err("IntPtr payload width mismatch".into());
    }
    for data in bytes.as_chunks::<8>().0 {
        let value = u64::from_le_bytes(*data);
        if value != 0 && !live(value) {
            return Err(format!(
                "IntPtr array value is not NULL or a live guest engine data handle: {value:#x}"
            ));
        }
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub(super) struct Api {
    new: unsafe extern "C" fn(Object, Object, usize) -> Object,
    class: unsafe extern "C" fn(Object) -> Object,
    array_class: unsafe extern "C" fn(Object, u32) -> Object,
    length: unsafe extern "C" fn(Object) -> usize,
    element_size: unsafe extern "C" fn(Object) -> i32,
    address: unsafe extern "C" fn(Object, i32, usize) -> Object,
    barrier: unsafe extern "C" fn(Object, Object, Object),
    image: unsafe extern "C" fn(Object) -> Object,
    value: unsafe extern "C" fn(Object) -> i32,
    element: unsafe extern "C" fn(Object) -> Object,
    rank: unsafe extern "C" fn(Object) -> u32,
    intptr: Object,
    reference_images: [Object; 6],
    values: [Option<value_layout::Api>; 6],
}
impl Api {
    pub(super) fn load(library: Object, reference_images: [Object; 6]) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing reference array API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let mut values = [None; 6];
        for (api, image) in values.iter_mut().zip(reference_images) {
            *api = Some(value_layout::Api::load(library, image)?);
        }
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        Ok(Self {
            new: sym!(
                "mono_array_new",
                unsafe extern "C" fn(Object, Object, usize) -> Object
            ),
            class: sym!(
                "mono_object_get_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            array_class: sym!(
                "mono_array_class_get",
                unsafe extern "C" fn(Object, u32) -> Object
            ),
            length: sym!("mono_array_length", unsafe extern "C" fn(Object) -> usize),
            element_size: sym!(
                "mono_array_element_size",
                unsafe extern "C" fn(Object) -> i32
            ),
            address: sym!(
                "mono_array_addr_with_size",
                unsafe extern "C" fn(Object, i32, usize) -> Object
            ),
            barrier: sym!(
                "mono_gc_wbarrier_set_arrayref",
                unsafe extern "C" fn(Object, Object, Object)
            ),
            image: sym!(
                "mono_class_get_image",
                unsafe extern "C" fn(Object) -> Object
            ),
            value: sym!(
                "mono_class_is_valuetype",
                unsafe extern "C" fn(Object) -> i32
            ),
            element: sym!(
                "mono_class_get_element_class",
                unsafe extern "C" fn(Object) -> Object
            ),
            rank: sym!("mono_class_get_rank", unsafe extern "C" fn(Object) -> u32),
            intptr: unsafe { intptr() },
            reference_images,
            values,
        })
    }
    fn element_layout(self, element: Object) -> Result<value_layout::Layout, String> {
        if !element.is_null() && element == self.intptr {
            // Exact System.IntPtr SZ arrays carry original engine data handles.
            // This does not admit raw pointer fields or ordinary invoke args.
            let layout = value_layout::Layout {
                class: element,
                width: 8,
                alignment: 8,
                slots: vec![value_layout::Slot {
                    nested: false,
                    offset: 0,
                    field: ptr::null_mut(),
                    shape: value_layout::Shape::Scalar {
                        width: 8,
                        boolean: false,
                    },
                }],
                value: true,
            };
            layout.check()?;
            return Ok(layout);
        }
        if !element.is_null()
            && unsafe { (self.value)(element) } == 0
            && self
                .reference_images
                .contains(&unsafe { (self.image)(element) })
        {
            Ok(value_layout::Layout::reference(element))
        } else {
            self.values
                .iter()
                .flatten()
                .copied()
                .find(|api| api.owns(element))
                .ok_or("value element outside owned image family")?
                .layout(element)
        }
    }
    pub(super) fn supports(self, element: Object) -> bool {
        self.element_layout(element).is_ok()
    }
}
struct Entry {
    native: Object,
    class: Object,
    element: Object,
    length: usize,
    layout: value_layout::Layout,
    header: HostRegion,
    payload: HostRegion,
    committed: Vec<u8>,
    published: bool,
    engine_handles: bool,
}
/// Retained domain/element metadata and bounded scalar length only. Managed
/// allocation and pinning occur on the actual attached worker before replying.
pub(super) struct WorkerRequest {
    api: Api,
    roots: instances::Api,
    domain: Object,
    element: Object,
    length: usize,
}
unsafe impl Send for WorkerRequest {}
pub(super) struct WorkerAllocation {
    root: instances::WorkerRoot,
    element: Object,
    length: usize,
    layout: value_layout::Layout,
}
// Private owned pin and nonmoving layout capabilities; no unrooted pointer.
unsafe impl Send for WorkerAllocation {}
impl WorkerRequest {
    pub(super) fn execute(self) -> Result<WorkerAllocation, String> {
        let layout = self.api.element_layout(self.element)?;
        let payload_size = layout
            .width
            .checked_mul(self.length)
            .ok_or("worker array size overflow")?;
        if self.length > 4096 || payload_size > 65504 || self.domain.is_null() {
            return Err("worker typed array exceeds allocation bounds".into());
        }
        let native = unsafe { (self.api.new)(self.domain, self.element, self.length) };
        Ok(WorkerAllocation {
            root: self.roots.worker_root(native)?,
            element: self.element,
            length: self.length,
            layout,
        })
    }
}
#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<u64, Entry>,
    retired: bool,
    writes: usize,
    verified: usize,
    value_verified: usize,
    scalar_writes: usize,
    borrowed: usize,
    refreshed: usize,
}
pub(super) struct Refresh {
    updates: Vec<(u64, Vec<u8>)>,
    pub(super) references: Vec<Object>,
}
impl Registry {
    fn layout(api: Api, entry: &Entry) -> Result<(), String> {
        let native = entry.native;
        entry.layout.check()?;
        if native.is_null()
            || unsafe { (api.class)(native) } != entry.class
            || unsafe { (api.array_class)(entry.element, 1) } != entry.class
            || unsafe { (api.element_size)(entry.class) } != entry.layout.width as i32
            || unsafe { (api.length)(native) } != entry.length
            || !unsafe {
                native
                    .cast::<u8>()
                    .add(16)
                    .cast::<Object>()
                    .read_unaligned()
            }
            .is_null()
            || unsafe { native.cast::<u8>().add(24).cast::<usize>().read_unaligned() }
                != entry.length
            || unsafe { (api.address)(native, entry.layout.width as i32, 0) }
                != unsafe { native.cast::<u8>().add(32).cast() }
        {
            return Err("typed SZ array identity/layout mismatch".into());
        }
        Ok(())
    }
    pub(super) fn allocate(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        domain: Object,
        element: Object,
        length: u64,
    ) -> Result<u64, String> {
        if length == 0 {
            return Err("nonempty typed allocation requires positive length".into());
        }
        let allocation = self
            .prepare_worker(api, root_api, domain, element, length)?
            .execute()?;
        self.adopt_worker(api, root_api, roots, allocation)
    }
    pub(super) fn prepare_worker(
        &self,
        api: Api,
        root_api: instances::Api,
        domain: Object,
        element: Object,
        length: u64,
    ) -> Result<WorkerRequest, String> {
        let layout = api.element_layout(element)?;
        let payload_size = layout
            .width
            .checked_mul(length as usize)
            .ok_or("typed array payload overflow")?;
        if self.retired
            || self.entries.len() >= 256
            || domain.is_null()
            || length > 4096
            || payload_size > 65504
        {
            return Err("typed array outside bounded allocation contract".into());
        }
        Ok(WorkerRequest {
            api,
            roots: root_api,
            domain,
            element,
            length: length as usize,
        })
    }
    pub(super) fn adopt_worker(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        allocation: WorkerAllocation,
    ) -> Result<u64, String> {
        if self.retired || self.entries.len() >= 256 {
            return Err("worker array reply reached retired/full registry".into());
        }
        let WorkerAllocation {
            root,
            element,
            length,
            layout,
        } = allocation;
        let native = root.target()?;
        let payload_size = layout
            .width
            .checked_mul(length)
            .ok_or("worker array reply size overflow")?;
        let token = BASE + self.entries.len() as u64 * STRIDE;
        roots.adopt_reference_array(root_api, token, native)?;
        let class = unsafe { (api.array_class)(element, 1) };
        let header = HostRegion::owned(16)?;
        header.with_native(|p| unsafe {
            p.as_ptr()
                .add(8)
                .cast::<u64>()
                .write_unaligned(length as u64)
        })?;
        let entry = Entry {
            native,
            class,
            element,
            length,
            layout,
            header,
            payload: HostRegion::owned(payload_size.max(1))?,
            committed: vec![0; payload_size],
            published: false,
            engine_handles: element == api.intptr,
        };
        Self::layout(api, &entry)?;
        // Only admitted fields are read; native padding is never published.
        for index in 0..length {
            for slot in &entry.layout.slots {
                let p = unsafe {
                    (api.address)(native, entry.layout.width as i32, index)
                        .cast::<u8>()
                        .add(slot.offset)
                };
                if unsafe { std::slice::from_raw_parts(p, slot.shape.width()) }
                    .iter()
                    .any(|&b| b != 0)
                {
                    return Err("new typed array has nonzero admitted field".into());
                }
            }
        }
        self.entries.insert(token, entry);
        Ok(token)
    }
    /// Root first, then convert declared slots from an existing native SZ array.
    /// Canonical instance identity is reused even if the field registry saw it
    /// before this array acquired a view. Native padding remains private.
    #[cfg(test)]
    pub(super) fn borrow(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        native: Object,
    ) -> Result<u64, String> {
        self.borrow_with(api, root_api, roots, native, |roots, native| {
            roots.borrow(root_api, native)
        })
    }
    pub(super) fn borrow_with(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        native: Object,
        reference: impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<u64, String> {
        if self.retired || native.is_null() {
            return Err("borrowed array outside live registry".into());
        }
        if let Some((&token, _)) = self.entries.iter().find(|(_, e)| e.native == native) {
            if roots.native(root_api, token)? != native {
                return Err("borrowed array identity changed".into());
            }
            return Ok(token);
        }
        if self.entries.len() >= 256 {
            return Err("borrowed array capacity exceeded".into());
        }
        let token = roots.borrow(root_api, native)?;
        let class = roots.class(root_api, token)?;
        if unsafe { (api.rank)(class) } != 1 {
            return Err("borrowed array is not rank one".into());
        }
        let element = unsafe { (api.element)(class) };
        let layout = api.element_layout(element)?;
        let length = unsafe { (api.length)(native) };
        let payload_size = layout
            .width
            .checked_mul(length)
            .ok_or("borrowed array payload overflow")?;
        if length > 4096 || payload_size > 65504 {
            return Err("borrowed array payload exceeds bounds".into());
        }
        let header = HostRegion::owned(16)?;
        header.with_native(|p| unsafe {
            p.as_ptr()
                .add(8)
                .cast::<u64>()
                .write_unaligned(length as u64)
        })?;
        let mut entry = Entry {
            native,
            class,
            element,
            length,
            layout,
            header,
            payload: HostRegion::owned(payload_size.max(1))?,
            committed: vec![0; payload_size],
            published: false,
            engine_handles: element == api.intptr,
        };
        Self::layout(api, &entry)?;
        let bytes = Self::native_values(api, root_api, roots, &entry, reference)?;
        entry.payload.with_native(|p| unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), p.as_ptr(), bytes.len())
        })?;
        entry.committed = bytes;
        self.entries.insert(token, entry);
        self.borrowed += 1;
        Ok(token)
    }
    fn native_values(
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        entry: &Entry,
        mut reference: impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0; entry.layout.width * entry.length];
        for index in 0..entry.length {
            let base = unsafe { (api.address)(entry.native, entry.layout.width as i32, index) }
                .cast::<u8>();
            for slot in &entry.layout.slots {
                let start = index * entry.layout.width + slot.offset;
                let data = &mut bytes[start..start + slot.shape.width()];
                match slot.shape {
                    value_layout::Shape::Reference(target) => {
                        let native =
                            unsafe { base.add(slot.offset).cast::<Object>().read_unaligned() };
                        let token = if native.is_null() {
                            0
                        } else {
                            let token = reference(roots, native)?;
                            roots.compatible(root_api, token, target)?;
                            token
                        };
                        data.copy_from_slice(&token.to_le_bytes());
                    }
                    value_layout::Shape::Scalar { .. } | value_layout::Shape::Inline { .. } => unsafe {
                        ptr::copy_nonoverlapping(
                            base.add(slot.offset),
                            data.as_mut_ptr(),
                            data.len(),
                        );
                    },
                }
            }
            let start = index * entry.layout.width;
            entry
                .layout
                .validate(&bytes[start..start + entry.layout.width], root_api, roots)?;
        }
        if entry.engine_handles
            && bytes.as_chunks::<8>().0.iter().any(|data| {
                let value = u64::from_le_bytes(*data);
                value != 0
                    && !entry
                        .committed
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .any(|known| known == data)
            })
        {
            return Err("native IntPtr array returned an unadmitted engine handle".into());
        }
        Ok(bytes)
    }
    pub(super) fn prepare_refresh(
        &self,
        api: Api,
        root_api: instances::Api,
        roots: &mut instances::Registry,
        mut reference: impl FnMut(&mut instances::Registry, Object) -> Result<u64, String>,
    ) -> Result<Refresh, String> {
        if self.retired {
            return Err("typed array refresh after retirement".into());
        }
        for (&token, entry) in &self.entries {
            if roots.native(root_api, token)? != entry.native {
                return Err("typed array refresh root changed".into());
            }
            Self::layout(api, entry)?;
            if Self::values(entry)? != entry.committed {
                return Err("array refresh would discard pending guest write".into());
            }
        }
        let mut updates = Vec::new();
        let mut references = Vec::new();
        for (&token, entry) in &self.entries {
            let bytes = Self::native_values(api, root_api, roots, entry, &mut reference)?;
            if bytes != entry.committed {
                references.extend(self.native_references(api, root_api, roots, token)?);
                updates.push((token, bytes));
            }
        }
        Ok(Refresh {
            updates,
            references,
        })
    }
    pub(super) fn commit_refresh(&mut self, plan: Refresh) -> Result<(), String> {
        for (token, bytes) in plan.updates {
            let entry = self
                .entries
                .get_mut(&token)
                .ok_or("array refresh layout changed")?;
            entry.payload.with_native(|p| unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), p.as_ptr(), bytes.len());
            })?;
            entry.committed = bytes;
            self.refreshed += 1;
        }
        Ok(())
    }
    pub(super) fn native_references(
        &self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
        token: u64,
    ) -> Result<Vec<Object>, String> {
        let entry = self
            .entries
            .get(&token)
            .ok_or("unknown typed array graph")?;
        if self.retired || roots.native(root_api, token)? != entry.native {
            return Err("typed array graph outside live root".into());
        }
        Self::layout(api, entry)?;
        let mut references = Vec::new();
        for index in 0..entry.length {
            for slot in &entry.layout.slots {
                if matches!(slot.shape, value_layout::Shape::Reference(_)) {
                    let native = unsafe {
                        (api.address)(entry.native, entry.layout.width as i32, index)
                            .cast::<u8>()
                            .add(slot.offset)
                            .cast::<Object>()
                            .read_unaligned()
                    };
                    if !native.is_null() {
                        references.push(native);
                    }
                }
            }
        }
        Ok(references)
    }
    pub(super) fn publish(&mut self, memory: &mut Memory, token: u64) -> Result<(), String> {
        if self.retired {
            return Err("typed array publication after retirement".into());
        }
        let entry = self.entries.get_mut(&token).ok_or("unknown typed array")?;
        if !entry.published {
            memory.map_shared_data(token + 16, entry.header.clone(), READ, READ)?;
            if entry.length != 0 {
                if let Err(error) = memory.map_shared_data(
                    token + 32,
                    entry.payload.clone(),
                    READ | WRITE,
                    READ | WRITE,
                ) {
                    memory.unmap_shared_data(token + 16)?;
                    return Err(error);
                }
            }
            entry.published = true;
        }
        Ok(())
    }
    fn values(entry: &Entry) -> Result<Vec<u8>, String> {
        entry.payload.with_native(|p| unsafe {
            std::slice::from_raw_parts(p.as_ptr(), entry.committed.len()).to_vec()
        })
    }
    fn slot(&self, owner: u64, address: u64) -> Option<(u64, usize, usize)> {
        if self.retired {
            return None;
        }
        let (&token, entry) = self.entries.range(..=address).next_back()?;
        let offset = address.checked_sub(token + 32)? as usize;
        let index = offset / entry.layout.width;
        if (owner != 0 && owner != token) || !entry.published || index >= entry.length {
            return None;
        }
        let slot = entry.layout.slots.iter().position(|s| {
            s.offset == offset % entry.layout.width
                && matches!(s.shape, value_layout::Shape::Reference(_))
        })?;
        Some((token, index, slot))
    }
    pub(super) fn has_reference(&self, owner: u64, address: u64) -> bool {
        self.slot(owner, address).is_some()
    }
    pub(super) fn length(
        &self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
        token: u64,
    ) -> Result<u64, String> {
        let entry = self
            .entries
            .get(&token)
            .ok_or("unknown typed array length")?;
        if self.retired || !entry.published || roots.native(root_api, token)? != entry.native {
            return Err("typed array length requires a live published identity".into());
        }
        Self::layout(api, entry)?;
        Ok(entry.length as u64)
    }
    pub(super) fn address(&self, token: u64, size: u64, index: u64) -> Result<u64, String> {
        let entry = self
            .entries
            .get(&token)
            .ok_or("unknown typed array address")?;
        if self.retired
            || !entry.published
            || size != entry.layout.width as u64
            || index >= entry.length as u64
        {
            return Err("typed array address size/index/retirement mismatch".into());
        }
        Ok(token + 32 + index * size)
    }
    pub(super) fn sync(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
        memory: &Memory,
    ) -> Result<(), String> {
        if self.retired {
            return Err("typed array sync after retirement".into());
        }
        let mut updates = Vec::new();
        for (&token, entry) in &self.entries {
            if roots.native(root_api, token)? != entry.native {
                return Err("typed array root changed".into());
            }
            Self::layout(api, entry)?;
            let bytes = Self::values(entry)?;
            if entry.engine_handles {
                validate_engine_handles(&bytes, |value| memory.guest_private_data_slot(value, 1))?;
            }
            for index in 0..entry.length {
                let start = index * entry.layout.width;
                let refs = entry.layout.validate(
                    &bytes[start..start + entry.layout.width],
                    root_api,
                    roots,
                )?;
                for (slot, reference) in entry.layout.slots.iter().zip(refs) {
                    let offset = start + slot.offset;
                    let end = offset + slot.shape.width();
                    if bytes[offset..end] != entry.committed[offset..end] {
                        updates.push((
                            token,
                            index,
                            slot.offset,
                            bytes[offset..end].to_vec(),
                            reference,
                        ));
                    }
                }
            }
        }
        // Validate the complete batch before any scalar or reference store.
        for (token, index, offset, bytes, reference) in updates {
            let entry = self.entries.get_mut(&token).unwrap();
            let destination = unsafe {
                (api.address)(entry.native, entry.layout.width as i32, index)
                    .cast::<u8>()
                    .add(offset)
            };
            if let Some(reference) = reference {
                unsafe { (api.barrier)(entry.native, destination.cast(), reference) };
                self.writes += 1;
            } else {
                unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len()) };
                self.scalar_writes += 1;
            }
            let start = index * entry.layout.width + offset;
            entry.committed[start..start + bytes.len()].copy_from_slice(&bytes);
        }
        Ok(())
    }
    pub(super) fn store(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
        memory: &mut Memory,
        args: [u64; 3],
    ) -> Result<(), String> {
        let [owner, address, value] = args;
        let (token, index, slot) = self
            .slot(owner, address)
            .ok_or("barrier destination is not a typed array reference slot")?;
        let entry = &self.entries[&token];
        let slot = &entry.layout.slots[slot];
        let value_layout::Shape::Reference(class) = slot.shape else {
            unreachable!()
        };
        let native = if value == 0 {
            ptr::null_mut()
        } else {
            roots.compatible(root_api, value, class)?
        };
        let offset = index * entry.layout.width + slot.offset;
        let changed = entry.committed[offset..offset + 8] != value.to_le_bytes();
        let slot_offset = slot.offset;
        memory.write_int(address, 8, value)?;
        self.sync(api, root_api, roots, memory)?;
        if !changed {
            let entry = &self.entries[&token];
            unsafe {
                (api.barrier)(
                    entry.native,
                    (api.address)(entry.native, entry.layout.width as i32, index)
                        .cast::<u8>()
                        .add(slot_offset)
                        .cast(),
                    native,
                )
            };
            self.writes += 1;
        }
        Ok(())
    }
    pub(super) fn verify(
        &mut self,
        api: Api,
        root_api: instances::Api,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Err("typed array verification after retirement".into());
        }
        for (&token, entry) in &self.entries {
            if roots.native(root_api, token)? != entry.native {
                return Err("typed array root lost across GC".into());
            }
            Self::layout(api, entry)?;
            if Self::values(entry)? != entry.committed {
                return Err("typed array guest values not synchronized before GC".into());
            }
            for index in 0..entry.length {
                let start = index * entry.layout.width;
                let refs = entry.layout.validate(
                    &entry.committed[start..start + entry.layout.width],
                    root_api,
                    roots,
                )?;
                for (slot, reference) in entry.layout.slots.iter().zip(refs) {
                    let p = unsafe {
                        (api.address)(entry.native, entry.layout.width as i32, index)
                            .cast::<u8>()
                            .add(slot.offset)
                    };
                    let expected = reference
                        .map(|r| (r as u64).to_le_bytes().to_vec())
                        .unwrap_or_else(|| {
                            entry.committed
                                [start + slot.offset..start + slot.offset + slot.shape.width()]
                                .to_vec()
                        });
                    if unsafe { std::slice::from_raw_parts(p, slot.shape.width()) } != expected {
                        return Err("native typed array slot differs across GC".into());
                    }
                }
            }
            if entry.layout.value {
                self.value_verified += 1;
            } else {
                self.verified += 1;
            }
        }
        Ok(())
    }
    pub(super) fn retire(
        &mut self,
        memory: &mut Memory,
        roots: &instances::Registry,
    ) -> Result<(), String> {
        if self.retired {
            return Ok(());
        }
        if !roots.roots_released() && !self.entries.is_empty() {
            return Err("typed array views retired before roots".into());
        }
        for (&token, entry) in &self.entries {
            if entry.published {
                memory.unmap_shared_data(token + 16)?;
                if entry.length != 0 {
                    memory.unmap_shared_data(token + 32)?;
                }
            }
            entry.header.revoke()?;
            entry.payload.revoke()?;
        }
        self.retired = true;
        Ok(())
    }
    pub(super) fn report(&self) -> String {
        format!("native-reference-arrays={}\nnative-reference-array-writes={}\nnative-reference-array-gc-verifications={}\nnative-reference-array-views-retired={}\n",self.entries.values().filter(|e|!e.layout.value).count(),self.writes,self.verified,self.retired)+&format!("native-value-arrays={}\nnative-value-array-gc-verifications={}\nnative-value-array-scalar-writes={}\n",self.entries.values().filter(|e|e.layout.value).count(),self.value_verified,self.scalar_writes)+&format!("native-borrowed-typed-arrays={}\n", self.borrowed)+&format!("native-typed-array-refreshes={}\n", self.refreshed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intptr_arrays_accept_only_null_and_live_guest_data_handles() {
        let valid = [0u64, 0x30001000];
        let bytes: Vec<_> = valid.into_iter().flat_map(u64::to_le_bytes).collect();
        validate_engine_handles(&bytes, |value| value == 0x30001000).unwrap();
        for value in [
            0x180001000u64,
            0x6ffe40000000,
            0xffffffffffffffff,
            0x100000000,
        ] {
            assert!(validate_engine_handles(&value.to_le_bytes(), |_| false).is_err());
        }
        assert!(validate_engine_handles(&[0; 7], |_| true).is_err());
    }
    unsafe extern "C" fn test_image(_: Object) -> Object {
        1usize as Object
    }
    unsafe extern "C" fn test_value(_: Object) -> i32 {
        0
    }
    unsafe extern "C" fn test_element(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn test_rank(_: Object) -> u32 {
        1
    }
    use std::cell::{Cell, RefCell};
    thread_local! {
        static RAW: RefCell<Vec<Box<[usize]>>> = const { RefCell::new(Vec::new()) };
        static WRITES: Cell<usize> = const { Cell::new(0) };
    }
    unsafe extern "C" fn new(_: Object, _: Object, length: usize) -> Object {
        RAW.with(|raw| {
            let mut data = vec![0; 4 + length].into_boxed_slice();
            data[3] = length;
            let p = data.as_mut_ptr().cast();
            raw.borrow_mut().push(data);
            p
        })
    }
    unsafe extern "C" fn class(_: Object) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn array_class(_: Object, _: u32) -> Object {
        2usize as Object
    }
    unsafe extern "C" fn length(p: Object) -> usize {
        unsafe { p.cast::<usize>().add(3).read() }
    }
    unsafe extern "C" fn size(_: Object) -> i32 {
        8
    }
    unsafe extern "C" fn address(p: Object, _: i32, index: usize) -> Object {
        unsafe { p.cast::<usize>().add(4 + index).cast() }
    }
    unsafe extern "C" fn barrier(_: Object, p: Object, value: Object) {
        unsafe {
            p.cast::<Object>().write(value);
        }
        WRITES.with(|c| c.set(c.get() + 1));
    }
    fn api() -> Api {
        Api {
            new,
            class,
            array_class,
            length,
            element_size: size,
            address,
            barrier,
            intptr: ptr::null_mut(),
            image: test_image,
            value: test_value,
            element: test_element,
            rank: test_rank,
            reference_images: [1usize as Object; 6],
            values: [None; 6],
        }
    }
    #[test]
    fn worker_allocates_on_its_os_thread_and_keeps_array_root_through_publication() {
        unsafe extern "C" fn worker_new(_: Object, _: Object, length: usize) -> Object {
            assert_eq!(std::thread::current().name(), Some("wire-test-array"));
            let mut data = vec![0usize; 4 + length].into_boxed_slice();
            data[3] = length;
            Box::into_raw(data).cast::<usize>().cast()
        }
        let api = Api {
            new: worker_new,
            ..api()
        };
        let roots_api = instances::tests::identity_api();
        let mut arrays = Registry::default();
        let mut roots = instances::Registry::default();
        let mut memory = Memory::default();
        let mut backing = Vec::new();
        for length in [0, 1] {
            let request = arrays
                .prepare_worker(api, roots_api, 1usize as Object, 2usize as Object, length)
                .unwrap();
            let reply = std::thread::Builder::new()
                .name("wire-test-array".into())
                .spawn(move || request.execute())
                .unwrap()
                .join()
                .unwrap()
                .unwrap();
            backing.push((reply.root.target().unwrap(), length as usize));
            let token = arrays
                .adopt_worker(api, roots_api, &mut roots, reply)
                .unwrap();
            arrays.publish(&mut memory, token).unwrap();
            assert_eq!(
                arrays.length(api, roots_api, &roots, token).unwrap(),
                length
            );
            assert_eq!(memory.read_int(token + 24, 8).unwrap(), length);
            assert_eq!(memory.read_int(token + 32, 8).is_ok(), length != 0);
            assert!(memory.read_int(token, 8).is_err());
        }
        assert!(arrays
            .prepare_worker(api, roots_api, 1usize as Object, 2usize as Object, 4097)
            .is_err());
        arrays.verify(api, roots_api, &roots).unwrap();
        roots.release(roots_api);
        arrays.retire(&mut memory, &roots).unwrap();
        assert!(arrays
            .prepare_worker(api, roots_api, 1usize as Object, 2usize as Object, 1)
            .is_err());
        for (pointer, length) in backing {
            unsafe {
                drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                    pointer.cast::<usize>(),
                    4 + length,
                )));
            }
        }
    }
    #[test]
    fn refresh_is_atomic_across_arrays_rejects_dirty_guest_and_late_bad_reference() {
        let api = api();
        let root_api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let a = unsafe { new(1usize as Object, 2usize as Object, 1) };
        let b = unsafe { new(1usize as Object, 2usize as Object, 1) };
        let ta = registry.borrow(api, root_api, &mut roots, a).unwrap();
        let tb = registry.borrow(api, root_api, &mut roots, b).unwrap();
        registry.publish(&mut memory, ta).unwrap();
        registry.publish(&mut memory, tb).unwrap();
        unsafe {
            address(a, 8, 0).cast::<Object>().write(4usize as Object);
            address(b, 8, 0).cast::<Object>().write(8usize as Object);
        }
        assert!(registry
            .prepare_refresh(api, root_api, &mut roots, |roots, native| {
                if native as usize == 8 {
                    return Err("late invalid native reference".into());
                }
                roots.borrow(root_api, native)
            })
            .is_err());
        assert_eq!(memory.read_int(ta + 32, 8).unwrap(), 0);
        assert_eq!(memory.read_int(tb + 32, 8).unwrap(), 0);
        unsafe {
            address(b, 8, 0).cast::<Object>().write(ptr::null_mut());
        }
        memory.write_int(tb + 32, 8, ta).unwrap();
        assert!(registry
            .prepare_refresh(api, root_api, &mut roots, |roots, native| roots
                .borrow(root_api, native))
            .is_err());
        assert_eq!(memory.read_int(ta + 32, 8).unwrap(), 0);
        memory.write_int(tb + 32, 8, 0).unwrap();
        let plan = registry
            .prepare_refresh(api, root_api, &mut roots, |roots, native| {
                roots.borrow(root_api, native)
            })
            .unwrap();
        assert_eq!(memory.read_int(ta + 32, 8).unwrap(), 0);
        registry.commit_refresh(plan).unwrap();
        let value = roots.borrow(root_api, 4usize as Object).unwrap();
        assert_eq!(memory.read_int(ta + 32, 8).unwrap(), value);
        registry.verify(api, root_api, &roots).unwrap();
        roots.release(root_api);
        registry.retire(&mut memory, &roots).unwrap();
        assert!(registry
            .prepare_refresh(api, root_api, &mut roots, |roots, native| roots
                .borrow(root_api, native))
            .is_err());
    }
    #[test]
    fn borrowed_arrays_preserve_existing_identity_and_convert_references() {
        let api = api();
        let root_api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let native = unsafe { new(1usize as Object, 2usize as Object, 2) };
        unsafe {
            address(native, 8, 0)
                .cast::<Object>()
                .write(4usize as Object);
        }
        let identity = roots.borrow(root_api, native).unwrap();
        let mut registry = Registry::default();
        let token = registry.borrow(api, root_api, &mut roots, native).unwrap();
        assert_eq!(token, identity);
        assert_eq!(
            registry.borrow(api, root_api, &mut roots, native).unwrap(),
            token
        );
        let mut memory = Memory::default();
        registry.publish(&mut memory, token).unwrap();
        let reference = memory.read_int(token + 32, 8).unwrap();
        assert_ne!(reference, 4);
        assert_eq!(roots.native(root_api, reference).unwrap(), 4usize as Object);
        assert_eq!(memory.read_int(token + 40, 8).unwrap(), 0);
        assert!(memory.read_int(token, 8).is_err());
        let empty = unsafe { new(1usize as Object, 2usize as Object, 0) };
        let empty = registry.borrow(api, root_api, &mut roots, empty).unwrap();
        registry.publish(&mut memory, empty).unwrap();
        assert_eq!(memory.read_int(empty + 24, 8).unwrap(), 0);
        assert!(memory.write_int(empty + 24, 8, 1).is_err());
        assert!(memory.read_int(empty + 32, 1).is_err());
        registry.verify(api, root_api, &roots).unwrap();
        roots.release(root_api);
        registry.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(token + 32, 8).is_err());
        assert!(memory.read_int(empty + 24, 8).is_err());
        unsafe extern "C" fn invalid_rank(_: Object) -> u32 {
            2
        }
        let mut roots = instances::Registry::default();
        assert!(Registry::default()
            .borrow(
                Api {
                    rank: invalid_rank,
                    ..api
                },
                root_api,
                &mut roots,
                native
            )
            .is_err());
        assert!(Registry::default()
            .borrow(
                Api {
                    reference_images: [ptr::null_mut(); 6],
                    ..api
                },
                root_api,
                &mut roots,
                native
            )
            .is_err());
    }
    #[test]
    fn value_arrays_validate_scalars_and_all_references_before_native_writes() {
        WRITES.with(|c| c.set(0));
        unsafe extern "C" fn wide_size(_: Object) -> i32 {
            24
        }
        unsafe extern "C" fn wide_address(p: Object, _: i32, index: usize) -> Object {
            unsafe { p.cast::<u8>().add(32 + index * 24).cast() }
        }
        let api = Api {
            element_size: wide_size,
            address: wide_address,
            ..api()
        };
        let root_api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let value = roots.borrow(root_api, 4usize as Object).unwrap();
        let mut data = vec![0usize; 7].into_boxed_slice();
        data[3] = 1;
        let native = data.as_mut_ptr().cast();
        let token = BASE;
        roots
            .adopt_reference_array(root_api, token, native)
            .unwrap();
        let header = HostRegion::owned(16).unwrap();
        header
            .with_native(|p| unsafe { p.as_ptr().add(8).cast::<u64>().write(1) })
            .unwrap();
        let layout = value_layout::Layout {
            class: 2usize as Object,
            width: 24,
            alignment: 8,
            value: true,
            slots: vec![
                value_layout::Slot {
                    nested: false,
                    offset: 0,
                    field: 1usize as Object,
                    shape: value_layout::Shape::Reference(2usize as Object),
                },
                value_layout::Slot {
                    nested: false,
                    offset: 8,
                    field: 2usize as Object,
                    shape: value_layout::Shape::Scalar {
                        width: 4,
                        boolean: false,
                    },
                },
                value_layout::Slot {
                    nested: false,
                    offset: 16,
                    field: 3usize as Object,
                    shape: value_layout::Shape::Reference(2usize as Object),
                },
            ],
        };
        let mut registry = Registry::default();
        registry.entries.insert(
            token,
            Entry {
                native,
                class: 2usize as Object,
                element: 2usize as Object,
                length: 1,
                layout,
                header,
                payload: HostRegion::owned(24).unwrap(),
                committed: vec![0; 24],
                published: false,
                engine_handles: false,
            },
        );
        let mut memory = Memory::default();
        registry.publish(&mut memory, token).unwrap();
        memory.write_int(token + 32, 8, value).unwrap();
        memory.write_int(token + 40, 4, 0x80000001).unwrap();
        memory.write_int(token + 48, 8, 99).unwrap();
        assert!(registry.sync(api, root_api, &roots, &memory).is_err());
        assert_eq!(WRITES.with(Cell::get), 0);
        assert_eq!(data[5], 0);
        memory.write_int(token + 48, 8, 0).unwrap();
        registry.sync(api, root_api, &roots, &memory).unwrap();
        assert_eq!(data[4], 4);
        assert_eq!(data[5], 0x80000001);
        assert_eq!(WRITES.with(Cell::get), 1);
        assert!(!registry.has_reference(0, token + 40));
        assert!(registry
            .store(api, root_api, &roots, &mut memory, [0, token + 48, value])
            .is_ok());
        assert_eq!(data[6], 4);
        registry.verify(api, root_api, &roots).unwrap();
        assert_eq!(registry.value_verified, 1);
        roots.release(root_api);
        registry.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(token + 32, 8).is_err());
    }
    #[test]
    fn reference_arrays_reject_late_forgery_before_any_native_store_and_keep_headers_private() {
        WRITES.with(|c| c.set(0));
        let api = api();
        let root_api = instances::tests::identity_api();
        let mut roots = instances::Registry::default();
        let mut registry = Registry::default();
        let mut memory = Memory::default();
        let value = roots.borrow(root_api, 4usize as Object).unwrap();
        let a = registry
            .allocate(
                api,
                root_api,
                &mut roots,
                1usize as Object,
                2usize as Object,
                2,
            )
            .unwrap();
        let b = registry
            .allocate(
                api,
                root_api,
                &mut roots,
                1usize as Object,
                2usize as Object,
                1,
            )
            .unwrap();
        assert!(registry.length(api, root_api, &roots, a).is_err());
        registry.publish(&mut memory, a).unwrap();
        registry.publish(&mut memory, b).unwrap();
        assert_eq!(registry.length(api, root_api, &roots, a).unwrap(), 2);
        assert!(registry.length(api, root_api, &roots, value).is_err());
        assert!(registry.length(api, root_api, &roots, a + 8).is_err());
        assert!(memory.read_int(a, 8).is_err());
        assert!(memory.write_int(a + 24, 8, 7).is_err());
        assert_eq!(memory.read_int(a + 24, 8).unwrap(), 2);
        assert!(memory.read_int(a + 48, 8).is_err());
        assert!(registry.address(a, 4, 0).is_err());
        assert!(registry.address(a, 8, 2).is_err());
        assert!(registry
            .store(api, root_api, &roots, &mut memory, [0, a + 33, value])
            .is_err());
        memory.write_int(a + 32, 8, value).unwrap();
        memory.write_int(b + 32, 8, 0x1234).unwrap();
        assert!(registry.sync(api, root_api, &roots, &memory).is_err());
        assert_eq!(WRITES.with(Cell::get), 0);
        memory.write_int(b + 32, 8, 0).unwrap();
        registry.sync(api, root_api, &roots, &memory).unwrap();
        assert_eq!(WRITES.with(Cell::get), 1);
        registry
            .store(api, root_api, &roots, &mut memory, [a, a + 32, value])
            .unwrap();
        assert_eq!(WRITES.with(Cell::get), 2);
        registry.verify(api, root_api, &roots).unwrap();
        assert!(registry.retire(&mut memory, &roots).is_err());
        roots.release(root_api);
        registry.retire(&mut memory, &roots).unwrap();
        assert!(memory.read_int(a + 32, 8).is_err());
        assert!(registry.address(a, 8, 0).is_err());
        assert!(registry.length(api, root_api, &roots, a).is_err());
    }
}
