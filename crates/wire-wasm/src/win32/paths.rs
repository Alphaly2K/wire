use super::{
    call::Call,
    catalog::PathApi as P,
    context,
    modules::{self, Failure},
    path_helpers, ServiceError,
};
use crate::{
    filesystem::Files,
    machine::State,
    memory::{Memory, WRITE},
};

fn name(memory: &Memory, pointer: u64, wide: bool) -> Result<String, Failure> {
    if pointer == 0 {
        return Err(Failure::Windows(87));
    }
    let bytes = modules::string(memory, pointer, wide, 32768).map_err(|error| match error {
        Failure::Windows(206) => Failure::Windows(if wide { 87 } else { 123 }),
        error => error,
    })?;
    if wide {
        let units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| u16::from_le_bytes(b))
            .collect();
        String::from_utf16(&units).map_err(|_| Failure::Windows(1113))
    } else {
        crate::codepage::decode(&bytes, 932).map_err(Failure::Windows)
    }
}
fn write(memory: &mut Memory, pointer: u64, text: &str, wide: bool) -> Result<(), Failure> {
    let mut bytes = if wide {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    } else {
        crate::codepage::encode(text, 932)
            .map_err(Failure::Windows)?
            .0
    };
    bytes.resize(bytes.len() + if wide { 2 } else { 1 }, 0);
    memory.write(pointer, &bytes)?;
    Ok(())
}
fn value(id: P, a: &[u64; 16], memory: &mut Memory, files: &mut Files) -> Result<u64, Failure> {
    let wide = id.wide();
    match id {
        P::CanonicalizeA | P::CanonicalizeW => {
            if a[0] == 0 || a[1] == 0 {
                return Err(Failure::Windows(87));
            }
            let source = modules::string(memory, a[1], wide, 260)?;
            let units: Vec<u16> = if wide {
                source
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| u16::from_le_bytes(b))
                    .collect()
            } else {
                crate::codepage::decode_lossy(&source, 932)
                    .map_err(Failure::Windows)?
                    .encode_utf16()
                    .collect()
            };
            let result = path_helpers::canonicalize(&units);
            let mut bytes = if wide {
                result
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                crate::codepage::encode(
                    &String::from_utf16(&result).map_err(|_| Failure::Windows(1113))?,
                    932,
                )
                .map_err(Failure::Windows)?
                .0
            };
            let unit = if wide { 2 } else { 1 };
            if bytes.len() / unit >= 260 {
                return Err(Failure::Windows(206));
            }
            bytes.resize(bytes.len() + unit, 0);
            memory.write(a[0], &bytes)?;
            Ok(1)
        }
        P::UniversalNameA | P::UniversalNameW => Ok(2250),
        P::RemoveFileSpecA | P::RemoveFileSpecW => {
            if a[0] == 0 {
                return Ok(0);
            }
            let cut = path_helpers::remove_file_spec(
                wide,
                |offset| {
                    memory
                        .read_int(a[0] + u64::from(offset), if wide { 2 } else { 1 })
                        .map(|unit| unit as u16)
                        .map_err(Failure::Fault)
                },
                || Failure::Windows(87),
            )?;
            if let Some(offset) = cut {
                memory.write_int(a[0] + u64::from(offset), if wide { 2 } else { 1 }, 0)?;
                Ok(1)
            } else {
                Ok(0)
            }
        }
        P::UnquoteSpacesA | P::UnquoteSpacesW => {
            if a[0] == 0 {
                return Ok(0);
            }
            let text = name(memory, a[0], wide)?;
            if let Some(text) = path_helpers::unquote(&text) {
                write(memory, a[0], text, wide)?;
            }
            Ok(0)
        }
        P::AddExtensionA | P::AddExtensionW | P::RenameExtensionA | P::RenameExtensionW => {
            if a[0] == 0 || a[1] == 0 {
                return Ok(0);
            }
            let text = name(memory, a[0], wide)?;
            let ext = name(memory, a[1], wide)?;
            let result = path_helpers::extension(
                &text,
                &ext,
                matches!(id, P::AddExtensionA | P::AddExtensionW),
                wide,
            )
            .map_err(Failure::Windows)?;
            if let Some(text) = result {
                write(memory, a[0], &text, wide)?;
                Ok(1)
            } else {
                Ok(0)
            }
        }
        P::FileExistsA | P::FileExistsW | P::IsDirectoryA | P::IsDirectoryW => {
            let text = name(memory, a[0], wide)?;
            let metadata = files.metadata(&text).map_err(Failure::Windows)?;
            Ok(u64::from(
                !matches!(id, P::IsDirectoryA | P::IsDirectoryW) || metadata.attributes & 0x10 != 0,
            ))
        }
        P::MakeSureDirectoryPathExists => {
            let text = name(memory, a[0], false)?;
            let path = files.resolve(&text).map_err(Failure::Windows)?;
            let path = if text.ends_with(['\\', '/']) {
                path.as_str()
            } else {
                let end = path.rfind('\\').ok_or(Failure::Windows(3))?;
                &path[..end.max(3)]
            };
            files.create_directory_all(path).map_err(Failure::Windows)?;
            Ok(1)
        }
    }
}
pub(super) fn dispatch(
    id: P,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    files: &mut Files,
) -> Result<u64, ServiceError> {
    match value(id, &call.arguments, memory, files) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            let error = context::error_address(state, memory.mode());
            let clear = matches!(id, P::CanonicalizeA | P::CanonicalizeW) && call.arguments[0] != 0;
            let width = if id.wide() { 2 } else { 1 };
            if clear {
                memory.check(call.arguments[0], width, WRITE)?;
            }
            memory.check(error, 4, WRITE)?;
            if clear {
                memory.write_int(call.arguments[0], width, 0)?;
            }
            memory.write_int(error, 4, code.into())?;
            Ok(0)
        }
    }
}
