use super::{
    call::{self, Call},
    catalog::{ApiId, FileApi as F},
    context,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    filesystem::Files,
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::File(
            F::CopyFileA
                | F::CopyFileW
                | F::CopyFileExA
                | F::CopyFileExW
                | F::ReplaceFileA
                | F::ReplaceFileW
                | F::GetTempFileNameA
                | F::GetTempFileNameW
                | F::SetFileTime
        )
    )
}
pub(super) fn uses_wall(id: ApiId, state: &State, memory: &Memory) -> bool {
    matches!(id, ApiId::File(F::GetTempFileNameA | F::GetTempFileNameW))
        && call::peek_slot(state, memory, 2).is_some_and(|n| n as u32 == 0)
}
fn encoded(text: &str, wide: bool) -> Result<Vec<u8>, Failure> {
    if wide {
        Ok(text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    } else {
        let (bytes, replaced) =
            crate::codepage::encode(&(text.to_owned() + "\0"), 932).map_err(Failure::Windows)?;
        if replaced {
            return Err(Failure::Windows(1113));
        }
        Ok(bytes)
    }
}
pub(super) fn copy(
    source: &str,
    destination: &str,
    fail_existing: bool,
    files: &mut Files,
    image: &Loaded,
) -> Result<(), u32> {
    let source = files.resolve(source)?;
    let destination = files.resolve(destination)?;
    image.open_files.check_share(&source, 1, 7)?;
    image.open_files.check_share(&destination, 2, 7)?;
    if super::files::image_path(image, &destination, false) {
        return Err(32);
    }
    if files
        .open_data(&destination)
        .ok()
        .and_then(|o| o.node)
        .is_some_and(|n| image.mappings.has_file(&n))
    {
        return Err(1224);
    }
    files.copy_file(&source, &destination, fail_existing)
}
fn invoke(
    id: F,
    a: &[u64; 16],
    memory: &mut Memory,
    files: &mut Files,
    image: &mut Loaded,
    sample: u64,
) -> Result<u64, Failure> {
    match id {
        F::CopyFileA | F::CopyFileW | F::CopyFileExA | F::CopyFileExW => {
            let source = files
                .resolve(&name(memory, a[0], id.wide())?)
                .map_err(Failure::Windows)?;
            let destination = files
                .resolve(&name(memory, a[1], id.wide())?)
                .map_err(Failure::Windows)?;
            let fail_existing = if matches!(id, F::CopyFileExA | F::CopyFileExW) {
                if a[2] != 0 || a[5] as u32 & !3 != 0 {
                    return Err(Failure::Windows(50));
                }
                if a[4] != 0 && memory.read_int(a[4], 4)? != 0 {
                    return Err(Failure::Windows(1235));
                }
                a[5] as u32 & 1 != 0
            } else {
                a[2] as u32 != 0
            };
            copy(&source, &destination, fail_existing, files, image).map_err(Failure::Windows)?;
            Ok(1)
        }
        F::ReplaceFileA | F::ReplaceFileW => {
            if a[4] != 0 || a[5] != 0 || a[3] as u32 & !6 != 0 {
                return Err(Failure::Windows(87));
            }
            let original = files
                .resolve(&name(memory, a[0], id.wide())?)
                .map_err(Failure::Windows)?;
            let replacement = files
                .resolve(&name(memory, a[1], id.wide())?)
                .map_err(Failure::Windows)?;
            let backup = if a[2] == 0 {
                None
            } else {
                Some(
                    files
                        .resolve(&name(memory, a[2], id.wide())?)
                        .map_err(Failure::Windows)?,
                )
            };
            for path in [&original, &replacement].into_iter().chain(backup.iter()) {
                if super::files::image_path(image, path, true) || image.open_files.in_use(path) {
                    return Err(Failure::Windows(32));
                }
                if files
                    .open_data(path)
                    .ok()
                    .and_then(|o| o.node)
                    .is_some_and(|n| image.mappings.has_file(&n))
                {
                    return Err(Failure::Windows(1224));
                }
            }
            files
                .replace_file(&original, &replacement, backup.as_deref())
                .map_err(Failure::Windows)?;
            Ok(1)
        }
        F::SetFileTime => {
            image
                .open_files
                .write_attributes(a[0])
                .map_err(Failure::Windows)?;
            let mut times = [None; 3];
            for n in 0..3 {
                if a[n + 1] != 0 {
                    let value = memory.read_int(a[n + 1], 8)?;
                    if value >= 1 << 63 && !(n != 0 && value == u64::MAX) {
                        return Err(Failure::Windows(87));
                    }
                    times[n] = (value != 0).then_some(value);
                }
            }
            image
                .open_files
                .set_times(files, a[0], times)
                .map_err(Failure::Windows)?;
            Ok(1)
        }
        F::GetTempFileNameA | F::GetTempFileNameW => {
            let directory = name(memory, a[0], id.wide())?.replace('/', "\\");
            let prefix_units: Vec<_> = name(memory, a[1], id.wide())?
                .encode_utf16()
                .take(3)
                .collect();
            let prefix = String::from_utf16(&prefix_units).map_err(|_| Failure::Windows(1113))?;
            if directory.is_empty() {
                return Err(Failure::Windows(3));
            }
            if directory.encode_utf16().count() > 246 {
                return Err(Failure::Windows(111));
            }
            if prefix.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
                return Err(Failure::Windows(123));
            }
            let make =
                |n: u16| format!("{}\\{prefix}{n:04X}.tmp", directory.trim_end_matches('\\'));
            let unique = a[2] as u32;
            let mut number = unique as u16;
            if unique == 0 {
                if files
                    .metadata(&directory)
                    .map_err(|error| Failure::Windows(if error == 2 { 3 } else { error }))?
                    .attributes
                    & 0x10
                    == 0
                {
                    return Err(Failure::Windows(267));
                }
                number = image.temp_unique;
                let mut found = false;
                for _ in 0..65535 {
                    number = number.wrapping_add(1).max(1);
                    match files.metadata(&make(number)) {
                        Ok(_) => {}
                        Err(2) => {
                            found = true;
                            break;
                        }
                        Err(error) => return Err(Failure::Windows(error)),
                    }
                }
                if !found {
                    return Err(Failure::Windows(80));
                }
            }
            let path = make(number);
            let bytes = encoded(&path, id.wide())?;
            if bytes.len() / if id.wide() { 2 } else { 1 } > 260 {
                return Err(Failure::Windows(111));
            }
            memory.check(a[3], bytes.len(), WRITE)?;
            if unique == 0 {
                files.create_file(&path).map_err(Failure::Windows)?;
                files
                    .open_data(&path)
                    .expect("created temporary file")
                    .node
                    .unwrap()
                    .borrow_mut()
                    .metadata
                    .times = [sample; 3];
                image.temp_unique = number;
            }
            memory
                .write(a[3], &bytes)
                .expect("preflighted temporary filename");
            Ok(if unique == 0 {
                number.into()
            } else {
                unique.into()
            })
        }
        _ => unreachable!(),
    }
}
pub(super) fn dispatch(
    id: F,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    files: &mut Files,
    image: Option<&mut Loaded>,
    sample: u64,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "file mutation requires a loaded image",
    ))?;
    match invoke(id, &call.arguments, memory, files, image, sample) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
