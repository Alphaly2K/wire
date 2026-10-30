//! File versions read the shared VFS; query values remain inside caller storage.
use super::{
    call::Call,
    catalog::VersionResourceApi as V,
    context,
    modules::{name, Failure},
    version_resource_core::{node, resource},
    ServiceError,
};
use crate::{
    filesystem::Files,
    machine::State,
    memory::{Fault, FaultKind, Memory, WRITE},
    pe::Image,
    process::Loaded,
};

fn add(memory: &Memory, base: u64, offset: usize) -> Result<u64, Failure> {
    let address = base.checked_add(offset as u64).ok_or(Fault {
        address: base,
        length: offset,
        access: WRITE,
        kind: FaultKind::Address,
    })?;
    if memory.mode().address(address) != address {
        return Err(Failure::Fault(Fault {
            address: base,
            length: offset,
            access: WRITE,
            kind: FaultKind::Address,
        }));
    }
    Ok(address)
}
#[derive(Default)]
struct Answer {
    value: u64,
    error: Option<u32>,
    outputs: Vec<(u64, Vec<u8>)>,
}
impl Answer {
    fn dword(&mut self, address: u64, value: u32) {
        self.outputs.push((address, value.to_le_bytes().to_vec()));
    }
}
fn fetch(
    id: V,
    a: &[u64; 16],
    state: &State,
    memory: &Memory,
    files: &Files,
    image: Option<&Loaded>,
    answer: &mut Answer,
) -> Result<(), Failure> {
    if id.size() && a[1] != 0 {
        answer.dword(a[1], 0);
    }
    if !id.size() && a[3] == 0 {
        return Err(Failure::Windows(13));
    }
    if a[0] == 0 {
        return Err(Failure::Windows(87));
    }
    let filename = name(memory, a[0], id.wide())?;
    if filename.is_empty() {
        return Err(Failure::Windows(161));
    }
    let path = files.resolve(&filename).map_err(Failure::Windows)?;
    let opened = files.open_data(&path).map_err(Failure::Windows)?;
    let contents = opened.node.ok_or(Failure::Windows(5))?;
    if let Some(image) = image {
        image
            .open_files
            .check_share(&path, 1, 3)
            .map_err(Failure::Windows)?;
    }
    let bytes = contents
        .borrow()
        .materialize()
        .map_err(|e| Failure::Windows(e.code()))?;
    let pe = Image::parse(&bytes).map_err(|_| Failure::Windows(1812))?;
    let locale = if image.is_some() {
        memory.read_int(context::locale_address(state, memory.mode()), 4)? as u16
    } else {
        0x411
    };
    let blob = resource(
        &bytes,
        pe.headers,
        &pe.sections,
        pe.dir(2),
        &[0, locale, 0x411, 0x11, 0x409],
    )
    .ok_or(Failure::Windows(1813))?;
    if id.size() {
        answer.value = (blob.len() * 2 + 4) as u64;
    } else {
        let size = (a[2] as u32 as usize).min(blob.len());
        if size != 0 {
            answer.outputs.push((a[3], blob[..size].to_vec()));
        }
        let length = u16::from_le_bytes(blob[..2].try_into().unwrap()) as usize;
        if a[2] as u32 as usize >= length + 4 {
            answer
                .outputs
                .push((add(memory, a[3], length)?, b"FE2X".to_vec()));
        }
        answer.value = 1;
    }
    answer.error = Some(0);
    Ok(())
}
fn query(id: V, a: &[u64; 16], memory: &Memory, answer: &mut Answer) -> Result<(), Failure> {
    if a[0] == 0 {
        return Ok(());
    }
    let path = if a[1] == 0 {
        String::new()
    } else {
        name(memory, a[1], id.wide())?
    };
    let size = memory.read_int(a[0], 2)? as usize;
    let mut blob = vec![0; size];
    memory.read(a[0], &mut blob)?;
    let mut current = node(&blob, 0, size)
        .filter(|n| n.key == "VS_VERSION_INFO")
        .ok_or(Failure::Windows(13))?;
    for key in path.split('\\').filter(|key| !key.is_empty()) {
        let mut at = current.children;
        let mut found = None;
        while at < current.end {
            let child = node(&blob, at, current.end).ok_or(Failure::Windows(13))?;
            at = (child.end + 3) & !3;
            if child.key.eq_ignore_ascii_case(key) {
                found = Some(child);
                break;
            }
        }
        let Some(child) = found else {
            if a[3] != 0 {
                answer.dword(a[3], 0);
            }
            return Err(Failure::Windows(1813));
        };
        current = child;
    }
    let mut pointer = add(memory, a[0], current.value)?;
    let mut length = current.length as u32;
    if id == V::QueryA && current.text {
        let units: Vec<_> = blob[current.value..current.value + current.length * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        let bytes = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)
            .map_err(Failure::Windows)?
            .0;
        let offset = size + 4 + current.value;
        if offset + bytes.len() > size * 2 + 4 {
            return Err(Failure::Windows(13));
        }
        pointer = add(memory, a[0], offset)?;
        length = bytes.len() as u32;
        if !bytes.is_empty() {
            answer.outputs.push((pointer, bytes));
        }
    }
    answer.outputs.push((
        a[2],
        pointer.to_le_bytes()[..memory.mode() as usize / 8].to_vec(),
    ));
    if a[3] != 0 {
        answer.dword(a[3], length);
    }
    answer.value = 1;
    Ok(())
}
pub(super) fn dispatch(
    id: V,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    files: &Files,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let mut answer = Answer::default();
    let result = if id.fetch() {
        fetch(
            id,
            &call.arguments,
            state,
            memory,
            files,
            image.as_deref(),
            &mut answer,
        )
    } else {
        query(id, &call.arguments, memory, &mut answer)
    };
    match result {
        Ok(()) => {}
        Err(Failure::Fault(fault)) => return Err(fault.into()),
        Err(Failure::Windows(error)) => {
            if let Some(input) = super::file_input::suspend(files, image, call, state.ip) {
                return Err(input);
            }
            answer.error = Some(error);
            answer.value = 0;
        }
    }
    let error = context::error_address(state, memory.mode());
    for (address, bytes) in &answer.outputs {
        memory.check(*address, bytes.len(), WRITE)?;
    }
    if answer.error.is_some() {
        memory.check(error, 4, WRITE)?;
    }
    for (address, bytes) in &answer.outputs {
        memory
            .write(*address, bytes)
            .expect("preflighted version resource output");
    }
    if let Some(code) = answer.error {
        memory
            .write_int(error, 4, code.into())
            .expect("preflighted version resource error");
    }
    Ok(answer.value)
}
