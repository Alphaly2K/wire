use super::{
    catalog::FileApi as F,
    file_cursor::{self, Batch, BUFFER_TOO_SMALL, NO_MORE_FILES, SUCCESS},
    file_information, file_search,
    files::metadata,
    modules::{name, Failure},
};
use crate::{
    filesystem::Files,
    memory::{Memory, READ, WRITE},
    process::Loaded,
};

#[path = "../../../../src/win32/kernel/file_patterns.rs"]
mod patterns;

fn basename(path: &str) -> &str {
    path.rsplit('\\').next().unwrap()
}

fn entries(files: &Files, directory: &str, pattern: &str) -> Result<Vec<String>, u32> {
    let mut entries: Vec<_> = files
        .entries(directory)
        .map_err(|error| if matches!(error, 2 | 31) { 3 } else { error })?
        .into_iter()
        .filter(|path| patterns::wildcard(pattern, basename(path)))
        .collect();
    entries.sort_by_cached_key(|path| basename(path).to_lowercase());
    Ok(entries)
}

fn find_data(files: &Files, path: &str, wide: bool) -> Result<Vec<u8>, u32> {
    file_search::find_data(metadata(files.metadata(path)?, 0), basename(path), wide)
}

pub(super) fn find(
    id: F,
    a: &[u64; 16],
    memory: &mut Memory,
    files: &Files,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    let searches = &mut image.file_searches;
    if id == F::FindClose {
        let search = searches.handles.remove(&a[0]).ok_or(Failure::Windows(6))?;
        searches.used -= search.charge;
        return Ok(1);
    }
    let first = matches!(
        id,
        F::FindFirstFileA | F::FindFirstFileW | F::FindFirstFileExA | F::FindFirstFileExW
    );
    let extended = matches!(id, F::FindFirstFileExA | F::FindFirstFileExW);
    if !first && !searches.handles.contains_key(&a[0]) {
        return Err(Failure::Windows(6));
    }
    if extended && (a[1] > 1 || a[3] != 0 || a[4] != 0 || a[5] & !2 != 0) {
        return Err(Failure::Windows(87));
    }
    let output = a[if extended { 2 } else { 1 }];
    memory.check(output, if id.wide() { 592 } else { 320 }, WRITE)?;
    if first {
        let pattern = name(memory, a[0], id.wide())?.replace('\\', "/");
        if pattern.ends_with('/') {
            return Err(Failure::Windows(2));
        }
        let (directory, wildcard) = pattern.rsplit_once('/').unwrap_or((".", &pattern));
        let directory = if directory.eq_ignore_ascii_case("c:") {
            "C:/"
        } else if directory.is_empty() {
            "/"
        } else {
            directory
        };
        let entries = entries(files, directory, wildcard).map_err(Failure::Windows)?;
        let first = entries.first().ok_or(Failure::Windows(2))?;
        let charge = entries.capacity() * std::mem::size_of::<String>()
            + entries.iter().map(String::capacity).sum::<usize>();
        if searches.handles.len() >= 4096
            || searches.next >= searches.end
            || searches
                .budget
                .is_some_and(|budget| charge > budget.saturating_sub(searches.used))
        {
            return Err(Failure::Windows(8));
        }
        let data = find_data(files, first, id.wide()).map_err(Failure::Windows)?;
        memory.write(output, &data)?;
        let handle = searches.next;
        searches.next += 16;
        searches.used += charge;
        searches.handles.insert(
            handle,
            file_search::Search {
                entries,
                next: 1,
                charge,
            },
        );
        Ok(handle)
    } else {
        let search = &searches.handles[&a[0]];
        let path = search
            .entries
            .get(search.next)
            .ok_or(Failure::Windows(18))?;
        let data = find_data(files, path, id.wide()).map_err(Failure::Windows)?;
        memory.write(output, &data)?;
        searches.handles.get_mut(&a[0]).unwrap().next += 1;
        Ok(1)
    }
}

fn batch(
    a: &[u64; 16],
    files: &Files,
    image: &Loaded,
    capacity: usize,
    single: bool,
    restart: bool,
    pattern: Option<String>,
) -> Result<Batch, u32> {
    let cursor = image.open_files.directory(a[0])?;
    Batch::prepare(cursor, capacity, single, restart, pattern, |pattern| {
        entries(files, image.open_files.path(a[0])?, pattern)?
            .into_iter()
            .map(|path| {
                file_information::directory_record(
                    metadata(files.metadata(&path)?, 0),
                    basename(&path),
                )
            })
            .collect()
    })
}

pub(super) fn full_directory(
    a: &[u64; 16],
    memory: &mut Memory,
    files: &Files,
    image: &Loaded,
    error: u64,
) -> Result<u64, Failure> {
    if a[3] > 16 * 1024 * 1024 {
        return Err(Failure::Windows(8));
    }
    memory.check(a[2], a[3] as usize, WRITE)?;
    let batch =
        batch(a, files, image, a[3] as usize, false, a[1] == 15, None).map_err(Failure::Windows)?;
    if batch.status == SUCCESS {
        memory.write(a[2], &batch.bytes)?;
        batch.commit();
        Ok(1)
    } else {
        memory.write_int(
            error,
            4,
            if batch.status == NO_MORE_FILES {
                18
            } else {
                24
            },
        )?;
        if batch.status == NO_MORE_FILES {
            batch.commit();
        }
        Ok(0)
    }
}

fn pattern(memory: &Memory, pointer: u64) -> Result<Result<Option<String>, u32>, Failure> {
    if pointer == 0 {
        return Ok(Ok(None));
    }
    memory.check(pointer, 16, READ)?;
    let length = memory.read_int(pointer, 2)? as usize;
    let maximum = memory.read_int(pointer + 2, 2)? as usize;
    if length > maximum || length > 512 || !length.is_multiple_of(2) {
        return Ok(Err(87));
    }
    let mut bytes = vec![0; length];
    memory.read(memory.read_int(pointer + 8, 8)?, &mut bytes)?;
    Ok(String::from_utf16(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect::<Vec<_>>(),
    )
    .map(Some)
    .map_err(|_| 87))
}

pub(super) fn nt_directory(
    a: &[u64; 16],
    memory: &mut Memory,
    files: &Files,
    image: &Loaded,
) -> Result<u64, Failure> {
    memory.check(a[4], 16, WRITE)?;
    let pattern = pattern(memory, a[9])?;
    let result = if a[1] != 0 || a[2] != 0 {
        Err(50)
    } else if a[7] != 2 {
        Err(124)
    } else if a[6] > 16 * 1024 * 1024 {
        Err(8)
    } else {
        memory.check(a[5], a[6] as usize, WRITE)?;
        pattern.and_then(|pattern| {
            batch(
                a,
                files,
                image,
                a[6] as usize,
                a[8] as u8 != 0,
                a[10] as u8 != 0,
                pattern,
            )
        })
    };
    let (status, size) = match &result {
        Ok(batch) => (batch.status, batch.bytes.len()),
        Err(error) => (file_cursor::nt_error(*error), 0),
    };
    let mut iosb = [0; 16];
    iosb[..4].copy_from_slice(&status.to_le_bytes());
    iosb[8..].copy_from_slice(&(size as u64).to_le_bytes());
    if let Ok(batch) = &result {
        if !batch.bytes.is_empty() {
            memory.write(a[5], &batch.bytes)?;
        }
    }
    memory.write(a[4], &iosb)?;
    if let Ok(batch) = result {
        if batch.status != BUFFER_TOO_SMALL {
            batch.commit();
        }
    }
    Ok(u64::from(status))
}
