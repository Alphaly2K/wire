use super::{
    call::Call,
    catalog::{ApiId, FileApi as F, HandleApi as H},
    context, endpoints, enumeration,
    file_information::{self, Information, Metadata},
    file_rights, handles, mappings,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    filesystem::{FileMetadata, Files, Mutation},
    machine::{Mode, State},
    memory::{Memory, READ, WRITE},
    process::Loaded,
};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::File(
            F::CreateFileA
                | F::CreateFileW
                | F::ReadFile
                | F::WriteFile
                | F::GetFileSize
                | F::GetFileSizeEx
                | F::SetFilePointer
                | F::SetFilePointerEx
                | F::SetEndOfFile
                | F::FlushFileBuffers
                | F::GetFileType
                | F::GetFileAttributesA
                | F::GetFileAttributesW
                | F::SetFileAttributesA
                | F::SetFileAttributesW
                | F::GetFileAttributesExA
                | F::GetFileAttributesExW
                | F::GetFileTime
                | F::GetFileInformationByHandle
                | F::GetFileInformationByHandleEx
                | F::CreateDirectoryA
                | F::CreateDirectoryW
                | F::DeleteFileA
                | F::DeleteFileW
                | F::MoveFileA
                | F::MoveFileW
                | F::MoveFileExA
                | F::MoveFileExW
                | F::RemoveDirectoryA
                | F::RemoveDirectoryW
                | F::AreFileApisAnsi
                | F::NtQueryDirectory
                | F::CreateFileMappingA
                | F::CreateFileMappingW
                | F::MapViewOfFile
                | F::MapViewOfFileEx
                | F::UnmapViewOfFile
                | F::FlushViewOfFile
                | F::CreatePipe
                | F::GetConsoleMode
                | F::WriteConsoleW
                | F::PeekNamedPipe
                | F::CancelIo
                | F::CancelIoEx
                | F::OverlappedResult
                | F::OverlappedResultEx
                | F::OpenFileMappingA
                | F::OpenFileMappingW
                | F::Lock
                | F::Unlock
                | F::CancelSynchronous
        ) | ApiId::Handle(
            H::GetStdHandle
                | H::SetStdHandle
                | H::SetHandleCount
                | H::CloseHandle
                | H::DuplicateHandle
                | H::GetHandleInformation
                | H::SetHandleInformation
        )
    ) || matches!(id, ApiId::File(f) if f.is_search())
}

pub(super) fn uses_wall(id: ApiId) -> bool {
    matches!(id, ApiId::File(F::WriteFile | F::SetEndOfFile))
}

pub(super) fn metadata(value: FileMetadata, identity: u64) -> Metadata {
    Metadata {
        attributes: value.attributes,
        size: value.size,
        times: value.times.map(Ok),
        change: Ok(value.times[2]),
        identity: Ok((1, identity, 1, value.size.div_ceil(4096) * 4096)),
    }
}

pub(super) fn security(memory: &Memory, pointer: u64) -> Result<bool, Failure> {
    if pointer == 0 {
        return Ok(false);
    }
    let (size, offset) = if memory.mode() == Mode::X86 {
        (12, 4)
    } else {
        (24, 8)
    };
    memory.check(pointer, size, READ)?;
    if memory.read_int(pointer, 4)? != size as u64 {
        return Err(Failure::Windows(87));
    }
    if memory.read_int(pointer + offset, memory.mode() as usize / 8)? != 0 {
        return Err(Failure::Windows(50));
    }
    Ok(memory.read_int(pointer + offset * 2, 4)? != 0)
}

pub(super) fn image_path(image: &Loaded, path: &str, children: bool) -> bool {
    if image.process.image_path(path, children) {
        return true;
    }
    let path = path.to_lowercase();
    image.modules.modules.values().any(|m| {
        let mapped = m.path.to_lowercase();
        mapped == path
            || children
                && mapped
                    .strip_prefix(&path)
                    .is_some_and(|tail| tail.starts_with('\\'))
    })
}

fn attributes(value: FileMetadata) -> [u8; 36] {
    let mut data = [0; 36];
    data[..4].copy_from_slice(&value.attributes.to_le_bytes());
    for (n, time) in value.times.into_iter().enumerate() {
        data[4 + n * 8..12 + n * 8].copy_from_slice(&time.to_le_bytes());
    }
    let size = if value.attributes & 0x10 != 0 {
        0
    } else {
        value.size
    };
    data[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
    data[32..36].copy_from_slice(&(size as u32).to_le_bytes());
    data
}

fn regular(
    id: F,
    a: &[u64; 16],
    memory: &mut Memory,
    files: &mut Files,
    image: &mut Loaded,
    error: u64,
    sample: u64,
) -> Result<u64, Failure> {
    match id {
        F::CancelSynchronous => {
            if image
                .process
                .primary(a[0], 1)
                .map_err(Failure::Windows)?
                .is_none()
            {
                image.threads.target(a[0], 1).map_err(Failure::Windows)?;
            }
            Err(Failure::Windows(50))
        }
        F::Lock | F::Unlock => {
            if image.open_files.flags(a[0]).is_none() {
                return Err(Failure::Windows(6));
            }
            Err(Failure::Windows(50))
        }
        F::OpenFileMappingA | F::OpenFileMappingW => {
            if a[2] == 0 {
                return Err(Failure::Windows(87));
            }
            let name = name(memory, a[2], id == F::OpenFileMappingW)?;
            Err(Failure::Windows(if name.is_empty() { 87 } else { 2 }))
        }
        id if id.is_search() => enumeration::find(id, a, memory, files, image),
        F::NtQueryDirectory => enumeration::nt_directory(a, memory, files, image),
        F::CreatePipe => endpoints::create(a, memory, image),
        F::PeekNamedPipe => endpoints::peek(a, memory, image),
        F::CancelIo | F::CancelIoEx => endpoints::cancel(id, image, a[0]),
        F::OverlappedResult | F::OverlappedResultEx => super::overlapped::query(a, memory, error),
        F::GetConsoleMode | F::WriteConsoleW => Err(Failure::Windows(6)),
        F::CreateFileMappingA | F::CreateFileMappingW if files.shared_processes() => {
            Err(Failure::Windows(50))
        }
        F::CreateFileMappingA
        | F::CreateFileMappingW
        | F::MapViewOfFile
        | F::MapViewOfFileEx
        | F::UnmapViewOfFile
        | F::FlushViewOfFile => mappings::dispatch(id, a, memory, image),
        F::SetFileAttributesA | F::SetFileAttributesW => {
            let path = name(memory, a[0], id.wide())?;
            files
                .set_attributes(&path, a[1] as u32)
                .map_err(Failure::Windows)?;
            Ok(1)
        }
        F::CreateFileA | F::CreateFileW => {
            if a[6] != 0 {
                return Err(Failure::Windows(50));
            }
            let path = name(memory, a[0], id.wide())?;
            let inherit = security(memory, a[3])?;
            let path = files.resolve(&path).map_err(Failure::Windows)?;
            let values = [a[1] as u32, a[2] as u32, a[4] as u32, a[5] as u32];
            let rights = file_rights::open_rights(values).map_err(Failure::Windows)?;
            if image_path(image, &path, false) && (rights & 2 != 0 || a[2] & 1 == 0) {
                return Err(Failure::Windows(32));
            }
            if matches!(a[4], 2 | 5)
                && files
                    .open_data(&path)
                    .ok()
                    .and_then(|open| open.node)
                    .is_some_and(|node| image.mappings.has_file(&node))
            {
                return Err(Failure::Windows(1224));
            }
            if matches!(a[4], 2 | 4) {
                memory.check(error, 4, WRITE)?;
            }
            let (handle, existed) = image
                .open_files
                .open(files, &path, values, inherit)
                .map_err(Failure::Windows)?;
            if matches!(a[4], 2 | 4) {
                memory.write_int(error, 4, if existed { 183 } else { 0 })?;
            }
            Ok(handle)
        }
        F::ReadFile | F::WriteFile => {
            if a[4] != 0 {
                return Err(Failure::Windows(50));
            }
            if a[3] == 0 {
                return Err(Failure::Windows(87));
            }
            let count = a[2] as u32 as usize;
            memory.check(a[3], 4, WRITE)?;
            if count != 0 {
                memory.check(a[1], count, if id == F::ReadFile { WRITE } else { READ })?;
            }
            if id == F::ReadFile {
                let bytes = image
                    .open_files
                    .read_backed(a[0], count)
                    .map_err(Failure::Windows)?;
                if !bytes.is_empty() {
                    memory.write(a[1], &bytes)?;
                }
                memory.write_int(a[3], 4, bytes.len() as u64)?;
            } else {
                let (path, position) = image.open_files.writable(a[0]).map_err(Failure::Windows)?;
                if image_path(image, path, false) {
                    return Err(Failure::Windows(32));
                }
                if count > 256 * 1024 * 1024 {
                    return Err(Failure::Windows(8));
                }
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(count)
                    .map_err(|_| Failure::Windows(8))?;
                bytes.resize(count, 0);
                if count != 0 {
                    memory.read(a[1], &mut bytes)?;
                }
                let identity = image.open_files.identity(a[0]).map_err(Failure::Windows)?;
                for (base, offset, source) in image.mappings.updates(identity, position, count) {
                    memory
                        .check_file_view(base, offset, source.len())
                        .map_err(|_| Failure::Windows(487))?;
                }
                let written = files
                    .write(path, position, &bytes)
                    .map_err(Failure::Windows)?;
                if written != 0 && image.open_files.auto_write_time(a[0]) {
                    files
                        .open_data(path)
                        .map_err(Failure::Windows)?
                        .node
                        .unwrap()
                        .borrow_mut()
                        .metadata
                        .times[2] = sample;
                }
                image
                    .mappings
                    .update(memory, identity, position, &bytes[..written])
                    .expect("preflighted file view updates");
                image.open_files.advance(a[0], written);
                memory.write_int(a[3], 4, written as u64)?;
            }
            Ok(1)
        }
        F::GetFileSize | F::GetFileSizeEx => {
            let size = image
                .open_files
                .metadata(a[0])
                .map_err(Failure::Windows)?
                .size;
            if id == F::GetFileSizeEx {
                memory.write_int(a[1], 8, size)?;
                Ok(1)
            } else {
                if a[1] != 0 {
                    memory.check(a[1], 4, WRITE)?;
                }
                memory.check(error, 4, WRITE)?;
                if a[1] != 0 {
                    memory.write_int(a[1], 4, size >> 32)?;
                }
                memory.write_int(error, 4, 0)?;
                Ok(size as u32 as u64)
            }
        }
        F::SetFilePointer | F::SetFilePointerEx => {
            let distance = if id == F::SetFilePointerEx {
                a[1] as i64
            } else if a[2] == 0 {
                a[1] as i32 as i64
            } else {
                memory.check(a[2], 4, WRITE)?;
                ((memory.read_int(a[2], 4)? << 32) | a[1] as u32 as u64) as i64
            };
            let width = if id == F::SetFilePointerEx { 8 } else { 4 };
            if a[2] != 0 {
                memory.check(a[2], width, WRITE)?;
            }
            let position = image
                .open_files
                .seek_position(a[0], distance, a[3] as u32)
                .map_err(Failure::Windows)?;
            let clear = id == F::SetFilePointer && position as u32 == u32::MAX;
            if clear {
                memory.check(error, 4, WRITE)?;
            }
            if a[2] != 0 {
                memory.write_int(
                    a[2],
                    width,
                    if width == 8 { position } else { position >> 32 },
                )?;
            }
            if clear {
                memory.write_int(error, 4, 0)?;
            }
            image
                .open_files
                .seek(a[0], distance, a[3] as u32)
                .expect("validated file cursor");
            Ok(if id == F::SetFilePointerEx {
                1
            } else {
                position as u32 as u64
            })
        }
        F::SetEndOfFile => {
            let (path, position) = image.open_files.writable(a[0]).map_err(Failure::Windows)?;
            if image_path(image, path, false) {
                return Err(Failure::Windows(32));
            }
            let node = files
                .open_data(path)
                .map_err(Failure::Windows)?
                .node
                .unwrap();
            if image.mappings.has_file(&node) {
                return Err(Failure::Windows(1224));
            }
            files.resize64(path, position).map_err(Failure::Windows)?;
            if image.open_files.auto_write_time(a[0]) {
                files
                    .open_data(path)
                    .map_err(Failure::Windows)?
                    .node
                    .unwrap()
                    .borrow_mut()
                    .metadata
                    .times[2] = sample;
            }
            Ok(1)
        }
        F::FlushFileBuffers => {
            image.open_files.writable(a[0]).map_err(Failure::Windows)?;
            Ok(1)
        }
        F::GetFileType => {
            if endpoints::contains(image, a[0]) {
                return Ok(3);
            }
            image.open_files.flags(a[0]).ok_or(Failure::Windows(6))?;
            Ok(1)
        }
        F::GetFileAttributesA
        | F::GetFileAttributesW
        | F::GetFileAttributesExA
        | F::GetFileAttributesExW => {
            let extended = matches!(id, F::GetFileAttributesExA | F::GetFileAttributesExW);
            if extended && a[1] != 0 {
                return Err(Failure::Windows(87));
            }
            let path = name(memory, a[0], id.wide())?;
            let value = files.metadata(&path).map_err(Failure::Windows)?;
            if extended {
                memory.write(a[2], &attributes(value))?;
                Ok(1)
            } else {
                Ok(u64::from(value.attributes))
            }
        }
        F::GetFileTime | F::GetFileInformationByHandle | F::GetFileInformationByHandleEx => {
            if id == F::GetFileInformationByHandleEx && matches!(a[1], 14 | 15) {
                return enumeration::full_directory(a, memory, files, image, error);
            }
            let mut value = image.open_files.metadata(a[0]).map_err(Failure::Windows)?;
            if value.attributes & 0x10 != 0 {
                value = files
                    .metadata(image.open_files.path(a[0]).map_err(Failure::Windows)?)
                    .map_err(Failure::Windows)?;
            }
            let identity = image.open_files.identity(a[0]).map_err(Failure::Windows)?;
            let kind = match id {
                F::GetFileTime => Information::Times(std::array::from_fn(|n| a[n + 1] != 0)),
                F::GetFileInformationByHandle => Information::Handle,
                _ => Information::Class(a[1] as u32),
            };
            let data = file_information::information(metadata(value, identity), kind, a[3])
                .map_err(Failure::Windows)?;
            if id == F::GetFileTime {
                for &at in &a[1..4] {
                    if at != 0 {
                        memory.check(at, 8, WRITE)?;
                    }
                }
                for (n, &at) in a[1..4].iter().enumerate() {
                    if at != 0 {
                        memory.write(at, &data[n * 8..n * 8 + 8])?;
                    }
                }
            } else {
                memory.write(
                    a[if id == F::GetFileInformationByHandle {
                        1
                    } else {
                        2
                    }],
                    &data,
                )?;
            }
            Ok(1)
        }
        F::CreateDirectoryA | F::CreateDirectoryW => {
            security(memory, a[1])?;
            let path = name(memory, a[0], id.wide())?;
            let path = files.resolve(&path).map_err(Failure::Windows)?;
            let parent = crate::pe::search::parent(&path);
            if files.metadata(parent).map_err(Failure::Windows)?.attributes & 0x10 == 0 {
                return Err(Failure::Windows(3));
            }
            files.create_directory(&path).map_err(Failure::Windows)?;
            Ok(1)
        }
        F::DeleteFileA
        | F::DeleteFileW
        | F::MoveFileA
        | F::MoveFileW
        | F::MoveFileExA
        | F::MoveFileExW
        | F::RemoveDirectoryA
        | F::RemoveDirectoryW => {
            let path = name(memory, a[0], id.wide())?;
            let source = files
                .resolve(&path)
                .map_err(Failure::Windows)?
                .to_lowercase();
            let moving = matches!(
                id,
                F::MoveFileA | F::MoveFileW | F::MoveFileExA | F::MoveFileExW
            );
            let destination = if moving && a[1] != 0 {
                Some(name(memory, a[1], id.wide())?)
            } else {
                None
            };
            let flags = if matches!(id, F::MoveFileExA | F::MoveFileExW) {
                a[2]
            } else {
                0
            };
            if moving && (flags & !1 != 0 || destination.is_none()) {
                return Err(Failure::Windows(50));
            }
            let cwd = files.cwd().to_lowercase();
            if image_path(image, &source, true)
                || image.open_files.in_use(&source)
                || cwd == source
                || cwd
                    .strip_prefix(&source)
                    .is_some_and(|s| s.starts_with('\\'))
            {
                return Err(Failure::Windows(32));
            }
            let target = destination
                .as_deref()
                .map(|s| files.resolve(s))
                .transpose()
                .map_err(Failure::Windows)?
                .map(|s| s.to_lowercase());
            if let Some(target) = &target {
                if image_path(image, target, true) || image.open_files.in_use(target) {
                    return Err(Failure::Windows(32));
                }
            }
            let mutation = if moving {
                Mutation::Move {
                    destination: destination.as_deref(),
                    flags,
                }
            } else if matches!(id, F::RemoveDirectoryA | F::RemoveDirectoryW) {
                Mutation::RemoveDirectory
            } else {
                Mutation::DeleteFile
            };
            files.mutate(&path, mutation).map_err(Failure::Windows)?;
            Ok(1)
        }
        F::AreFileApisAnsi => Ok(1),
        _ => unreachable!(),
    }
}

pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    files: &mut Files,
    image: Option<&mut Loaded>,
    sample: u64,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "file services require a loaded image",
    ))?;
    let error = context::error_address(state, memory.mode());
    let a = &call.arguments;
    let result = match id {
        ApiId::File(id @ (F::ReadFile | F::WriteFile)) if endpoints::contains(image, a[0]) => {
            match endpoints::transfer(id, a, memory, image) {
                Ok(Some(value)) => Ok(value),
                Ok(None) => return Err(ServiceError::Blocked),
                Err(failure) => Err(failure),
            }
        }
        ApiId::File(id) => regular(id, a, memory, files, image, error, sample),
        ApiId::Handle(id) => handles::dispatch(id, a, memory, image),
        _ => unreachable!(),
    };
    match result {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            if let Some(error) = super::file_input::suspend(files, Some(image), call, state.ip) {
                return Err(error);
            }
            let count = matches!(id, ApiId::File(F::ReadFile | F::WriteFile))
                .then_some(a[3])
                .filter(|&at| at != 0);
            if let Some(at) = count {
                memory.check(at, 4, WRITE)?;
            }
            memory.check(error, 4, WRITE)?;
            if let Some(at) = count {
                memory.write_int(at, 4, 0)?;
            }
            memory.write_int(error, 4, u64::from(code))?;
            Ok(match id {
                ApiId::Handle(H::GetStdHandle) => memory.mode().address(u64::MAX),
                ApiId::File(
                    F::CreateFileA
                    | F::CreateFileW
                    | F::FindFirstFileA
                    | F::FindFirstFileW
                    | F::FindFirstFileExA
                    | F::FindFirstFileExW,
                ) => memory.mode().address(u64::MAX),
                ApiId::File(
                    F::GetFileSize
                    | F::SetFilePointer
                    | F::GetFileAttributesA
                    | F::GetFileAttributesW,
                ) => u64::from(u32::MAX),
                _ => 0,
            })
        }
    }
}
