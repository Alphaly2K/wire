use super::{
    call::Call,
    catalog::{ApiId, FileApi, ModuleApi},
    context,
    modules::{name, Failure},
    ServiceError,
};
use crate::memory::WRITE;
use crate::{filesystem::Files, machine::State, memory::Memory, process::Loaded};

fn full_path(call: &Call, wide: bool, memory: &mut Memory, files: &Files) -> Result<u64, Failure> {
    let a = &call.arguments;
    let input = name(memory, a[0], wide)?;
    let mut path = files.resolve(&input).map_err(Failure::Windows)?;
    if input.ends_with(['\\', '/']) && !path.ends_with('\\') {
        path.push('\\');
    }
    let width = if wide { 2 } else { 1 };
    let encode = |s: &str| -> Result<Vec<u8>, Failure> {
        if wide {
            Ok(s.encode_utf16().flat_map(u16::to_le_bytes).collect())
        } else {
            crate::codepage::encode(s, 932)
                .map(|r| r.0)
                .map_err(Failure::Windows)
        }
    };
    let offset = path
        .rfind('\\')
        .map(|n| n + 1)
        .filter(|&n| n < path.len())
        .map(|n| encode(&path[..n]).map(|v| v.len()))
        .transpose()?;
    let mut bytes = encode(&path)?;
    bytes.resize(bytes.len() + width, 0);
    let required = bytes.len() / width;
    if a[1] < required as u64 {
        return Ok(required as u64);
    }
    memory.check(a[2], bytes.len(), WRITE)?;
    if a[3] != 0 {
        memory.check(a[3], memory.mode() as usize / 8, WRITE)?;
    }
    memory.write(a[2], &bytes)?;
    if a[3] != 0 {
        memory.write_int(
            a[3],
            memory.mode() as usize / 8,
            offset.map_or(0, |n| a[2] + n as u64),
        )?;
    }
    Ok(required as u64 - 1)
}

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Module(
            ModuleApi::GetDllDirectoryA
                | ModuleApi::GetDllDirectoryW
                | ModuleApi::SetDllDirectoryA
                | ModuleApi::SetDllDirectoryW
        ) | ApiId::File(
            FileApi::GetCurrentDirectoryA
                | FileApi::GetCurrentDirectoryW
                | FileApi::SetCurrentDirectoryA
                | FileApi::SetCurrentDirectoryW
                | FileApi::GetFullPathNameA
                | FileApi::GetFullPathNameW
        )
    )
}

pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    files: &mut Files,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "directory services require a loaded image",
    ))?;
    let result = (|| -> Result<u64, Failure> {
        if let ApiId::File(id @ (FileApi::GetFullPathNameA | FileApi::GetFullPathNameW)) = id {
            return full_path(call, id.wide(), memory, files);
        }
        let (wide, set, dll) = match id {
            ApiId::Module(id) => (
                id.wide(),
                matches!(
                    id,
                    ModuleApi::SetDllDirectoryA | ModuleApi::SetDllDirectoryW
                ),
                true,
            ),
            ApiId::File(id) => (
                id.wide(),
                matches!(
                    id,
                    FileApi::SetCurrentDirectoryA | FileApi::SetCurrentDirectoryW
                ),
                false,
            ),
            _ => unreachable!(),
        };
        if set {
            let pointer = call.arguments[0];
            if dll && pointer == 0 {
                image.search.directory = None;
                return Ok(1);
            }
            if pointer == 0 {
                return Err(Failure::Windows(87));
            }
            let value = name(memory, pointer, wide)?;
            if dll {
                let value = if value.is_empty() {
                    value
                } else {
                    files.resolve(&value).map_err(Failure::Windows)?
                };
                image.search.directory = Some(value);
            } else {
                files.set_cwd(&value).map_err(Failure::Windows)?;
            }
            return Ok(1);
        }
        let text = if dll {
            image.search.directory.as_deref().unwrap_or("")
        } else {
            files.cwd()
        };
        let width = if wide { 2 } else { 1 };
        let mut bytes = if wide {
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            crate::codepage::encode(text, 932)
                .map_err(Failure::Windows)?
                .0
        };
        let length = bytes.len() / width;
        let capacity = call.arguments[0] as u32 as usize;
        if capacity == 0 {
            return Ok(if length == 0 { 0 } else { length as u64 + 1 });
        }
        if capacity <= length {
            if dll {
                memory.write_int(context::error_address(state, memory.mode()), 4, 122)?;
            }
            return Ok(length as u64 + 1);
        }
        bytes.resize(bytes.len() + width, 0);
        memory.write(call.arguments[1], &bytes)?;
        Ok(length as u64)
    })();
    match result {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(code),
            )?;
            Ok(0)
        }
    }
}
