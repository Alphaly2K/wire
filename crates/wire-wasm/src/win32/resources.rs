use super::{
    call::Call,
    catalog::ResourceApi as R,
    context,
    modules::{self, Failure},
    user::{
        images::Key,
        resources::{self, Directory},
    },
    ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::Memory,
    process::Loaded,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum View {
    Resource {
        module: u64,
        descriptor: u64,
    },
    Data {
        module: u64,
        address: u64,
        length: u32,
    },
}
impl View {
    fn module(self) -> u64 {
        match self {
            Self::Resource { module, .. } | Self::Data { module, .. } => module,
        }
    }
}
pub(crate) struct Views {
    handles: BTreeMap<u64, View>,
    identities: BTreeMap<View, u64>,
    next: u64,
    end: u64,
}
impl Views {
    pub(crate) fn new(mode: Mode) -> Self {
        let start = if mode == Mode::X86 {
            0x46000000
        } else {
            0x7ffd06000000
        };
        Self {
            handles: BTreeMap::new(),
            identities: BTreeMap::new(),
            next: start,
            end: start + 0x1000000,
        }
    }
    fn intern(&mut self, view: View) -> Result<u64, Failure> {
        if let Some(handle) = self.identities.get(&view) {
            return Ok(*handle);
        }
        if self.handles.len() >= 8192 || self.next >= self.end {
            return Err(Failure::Windows(8));
        }
        let handle = self.next;
        self.next += 16;
        self.handles.insert(handle, view);
        self.identities.insert(view, handle);
        Ok(handle)
    }
    fn get(&self, handle: u64) -> Result<View, Failure> {
        self.handles
            .get(&handle)
            .copied()
            .ok_or(Failure::Windows(6))
    }
    pub(crate) fn retire(&mut self, module: u64) {
        self.handles.retain(|_, view| view.module() != module);
        self.identities.retain(|view, _| view.module() != module);
    }
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
        self.identities.clear();
    }
}
pub(super) struct Reader<'a>(pub &'a Memory);
impl resources::Reader for Reader<'_> {
    type Error = Failure;
    fn read(&self, address: u64, output: &mut [u8]) -> Result<(), Failure> {
        self.0.read(address, output).map_err(Failure::Fault)
    }
    fn invalid(&self, _message: &'static str) -> Failure {
        Failure::Windows(13)
    }
}
fn directory(image: &Loaded, module: u64) -> Result<Directory, Failure> {
    let module = if module == 0 { image.info[0] } else { module };
    let pe = &image
        .modules
        .modules
        .get(&module)
        .ok_or(Failure::Windows(6))?
        .image;
    let dir = pe.dir(2);
    Ok(Directory {
        base: module,
        image_size: pe.size,
        rva: dir.rva,
        size: dir.size,
    })
}
fn key(memory: &Memory, pointer: u64, wide: bool) -> Result<Key, Failure> {
    if pointer <= 65535 {
        Ok(Key::Id(pointer as u32))
    } else {
        modules::name(memory, pointer, wide).map(Key::Name)
    }
}
fn value(id: R, a: &[u64; 16], memory: &mut Memory, image: &mut Loaded) -> Result<u64, Failure> {
    match id {
        R::Free => Ok(0),
        R::FindA | R::FindW | R::FindExA | R::FindExW => {
            let ex = matches!(id, R::FindExA | R::FindExW);
            let wide = matches!(id, R::FindW | R::FindExW);
            let kind = key(memory, a[if ex { 1 } else { 2 }], wide)?;
            let item = key(memory, a[if ex { 2 } else { 1 }], wide)?;
            let dir = directory(image, a[0])?;
            let descriptor = dir
                .find(&Reader(memory), &kind, &item, ex.then_some(a[3] as u32))?
                .ok_or(Failure::Windows(1814))?;
            image.resources.intern(View::Resource {
                module: dir.base,
                descriptor,
            })
        }
        R::Size | R::Load => {
            let dir = directory(image, a[0])?;
            let View::Resource { module, descriptor } = image.resources.get(a[1])? else {
                return Err(Failure::Windows(6));
            };
            if module != dir.base {
                return Err(Failure::Windows(6));
            }
            let (address, length) = dir.data(&Reader(memory), descriptor)?;
            if id == R::Size {
                Ok(length.into())
            } else {
                image.resources.intern(View::Data {
                    module,
                    address,
                    length,
                })
            }
        }
        R::Lock => {
            let View::Data {
                module, address, ..
            } = image.resources.get(a[0])?
            else {
                return Err(Failure::Windows(6));
            };
            directory(image, module)?;
            Ok(address)
        }
        R::LoadStringA | R::LoadStringW => {
            let wide = id == R::LoadStringW;
            let count = a[3] as u32 as i32;
            if count < 0 || !wide && count == 0 {
                return Ok(0);
            }
            let dir = directory(image, a[0])?;
            let id = a[1] as u16 as u32;
            let selected = if let Some(descriptor) =
                dir.find(&Reader(memory), &Key::Id(6), &Key::Id((id >> 4) + 1), None)?
            {
                let (pointer, size) = dir.data(&Reader(memory), descriptor)?;
                resources::string_slot(&Reader(memory), pointer, size, id)?
            } else {
                None
            };
            let (pointer, length) = selected.unwrap_or((0, 0));
            if wide && count == 0 {
                memory.write_int(a[2], memory.mode() as usize / 8, pointer)?;
                return Ok(length.into());
            }
            let read = if wide {
                length.min(count as u32 - 1)
            } else {
                length
            };
            let mut bytes = vec![0; read as usize * 2];
            if !bytes.is_empty() {
                memory.read(pointer, &mut bytes)?;
            }
            if !wide {
                let units: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                bytes = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)
                    .map_err(Failure::Windows)?
                    .0;
                bytes.truncate(count as usize - 1);
            }
            let length = bytes.len() / if wide { 2 } else { 1 };
            bytes.resize(bytes.len() + if wide { 2 } else { 1 }, 0);
            memory.write(a[2], &bytes)?;
            Ok(length as u64)
        }
    }
}
pub(super) fn dispatch(
    id: R,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    if id == R::Free {
        return Ok(0);
    }
    let image = image.ok_or(ServiceError::Unavailable(
        "resources require a loaded image",
    ))?;
    match value(id, &call.arguments, memory, image) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
