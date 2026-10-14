//! Synchronous, confined FILE_FULL_DIR_INFORMATION batches on real directory handles.
use super::*;
use crate::x64::runtime::{Memory, Runtime64, WRITE};
use std::{cell::RefCell, rc::Rc};
const SUCCESS: u32 = 0;
const NO_MORE_FILES: u32 = 0x80000006;
const BUFFER_TOO_SMALL: u32 = 0xc0000023;
#[derive(Default)]
pub(super) struct Cursor {
    entries: Option<Rc<Vec<Vec<u8>>>>,
    next: usize,
    pattern: Option<String>,
}
struct Batch {
    cursor: Rc<RefCell<Cursor>>,
    entries: Rc<Vec<Vec<u8>>>,
    next: usize,
    pattern: String,
    bytes: Vec<u8>,
    status: u32,
}
impl Batch {
    fn commit(self) {
        let mut cursor = self.cursor.borrow_mut();
        cursor.entries = Some(self.entries);
        cursor.next = self.next;
        cursor.pattern = Some(self.pattern);
    }
}
fn nt_error(error: u32) -> u32 {
    match error {
        2 => 0xc000000f,
        3 => 0xc000003a,
        5 => 0xc0000022,
        6 => 0xc0000008,
        8 => 0xc0000017,
        50 => 0xc00000bb,
        87 => 0xc000000d,
        124 => 0xc0000003,
        267 => 0xc0000103,
        _ => 0xc0000001,
    }
}
fn live_path(file: &OpenFile) -> Result<PathBuf, u32> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        let mut path = [0u8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(file.file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) } < 0 {
            return Err(crate::fs::win_error(&std::io::Error::last_os_error()));
        }
        let length = path.iter().position(|&b| b == 0).ok_or(87u32)?;
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(path[..length].to_vec()).into())
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::fd::AsRawFd;
        std::fs::read_link(format!("/proc/self/fd/{}", file.file.as_raw_fd()))
            .map_err(|e| crate::fs::win_error(&e))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
    {
        Ok(file.path.clone())
    }
}
impl Files {
    fn directory_batch(
        &self,
        fs: &crate::fs::GuestFs,
        handle: u64,
        capacity: usize,
        single: bool,
        restart: bool,
        pattern: Option<String>,
    ) -> Result<Batch, u32> {
        let file = self.handles.get(&handle).ok_or(6u32)?;
        let cursor = file.directory.as_ref().ok_or(267u32)?.clone();
        if file.access & 1 == 0 {
            return Err(5);
        }
        let saved = cursor.borrow();
        // NtQueryDirectoryFile captures the search expression on its first call.
        let pattern = saved
            .pattern
            .clone()
            .unwrap_or_else(|| pattern.unwrap_or_else(|| "*".into()));
        let entries = if let Some(entries) = saved.entries.as_ref().filter(|_| !restart) {
            entries.clone()
        } else {
            let path = fs.confined_host_path(live_path(file)?)?;
            let current = path.metadata().map_err(|e| crate::fs::win_error(&e))?;
            let opened = file.file.metadata().map_err(|e| crate::fs::win_error(&e))?;
            let current_id = identity(&current)?;
            let opened_id = identity(&opened)?;
            if !current.is_dir() || (current_id.0, current_id.1) != (opened_id.0, opened_id.1) {
                return Err(5);
            }
            let mut paths = Vec::new();
            for item in std::fs::read_dir(path).map_err(|e| crate::fs::win_error(&e))? {
                let item = item.map_err(|e| crate::fs::win_error(&e))?;
                if !crate::file_api::wildcard(&pattern, &item.file_name().to_string_lossy()) {
                    continue;
                }
                // Follow only aliases whose target remains inside the guest drive.
                if let Ok(path) = fs.confined_host_path(item.path()) {
                    paths.push(path);
                    if paths.len() > 65536 {
                        return Err(8);
                    }
                }
            }
            paths.sort_by_key(|p| p.file_name().unwrap().to_string_lossy().to_lowercase());
            let mut entries = Vec::new();
            for path in paths {
                let metadata = path.metadata().map_err(|e| crate::fs::win_error(&e))?;
                let name = path.file_name().and_then(|n| n.to_str()).ok_or(1113u32)?;
                let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
                let mut data = vec![0; (68 + name.len()).next_multiple_of(8)];
                for (offset, time) in [
                    (8, metadata.created()),
                    (16, metadata.accessed()),
                    (24, metadata.modified()),
                ] {
                    let ticks = match filetime(time) {
                        Ok(t) => t,
                        Err(50) => 0,
                        Err(e) => return Err(e),
                    };
                    data[offset..offset + 8].copy_from_slice(&ticks.to_le_bytes());
                }
                data[32..40].copy_from_slice(&change_time(&metadata)?.to_le_bytes());
                let size = if metadata.is_dir() { 0 } else { metadata.len() };
                data[40..48].copy_from_slice(&size.to_le_bytes());
                data[48..56].copy_from_slice(&identity(&metadata)?.3.to_le_bytes());
                data[56..60].copy_from_slice(&attributes(&metadata, false).to_le_bytes());
                data[60..64].copy_from_slice(&(name.len() as u32).to_le_bytes());
                data[68..68 + name.len()].copy_from_slice(&name);
                entries.push(data);
            }
            Rc::new(entries)
        };
        let mut next = if restart { 0 } else { saved.next };
        drop(saved);
        let mut bytes = Vec::<u8>::new();
        let mut last = None;
        while let Some(record) = entries.get(next) {
            if record.len() > capacity.saturating_sub(bytes.len()) {
                break;
            }
            if let Some(at) = last {
                let offset = (bytes.len() - at) as u32;
                bytes[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            }
            last = Some(bytes.len());
            bytes.extend_from_slice(record);
            next += 1;
            if single {
                break;
            }
        }
        let status = if !bytes.is_empty() {
            SUCCESS
        } else if next == entries.len() {
            NO_MORE_FILES
        } else {
            BUFFER_TOO_SMALL
        };
        Ok(Batch {
            cursor,
            entries,
            next,
            pattern,
            bytes,
            status,
        })
    }
}
impl Runtime64 {
    pub(in crate::x64::runtime) fn nt_query_directory(
        &mut self,
        mem: &mut Memory,
        a: &[u64],
    ) -> Result<u64, String> {
        mem.check(a[4], 16, WRITE)?;
        let mut pattern = None;
        if a[9] != 0 {
            mem.check(a[9], 16, super::super::READ)?;
            let length = mem.read_int(a[9], 2)? as usize;
            let maximum = mem.read_int(a[9] + 2, 2)? as usize;
            if length > maximum || length > 512 || !length.is_multiple_of(2) {
                return Ok(0xc000000d);
            }
            let mut bytes = vec![0; length];
            mem.read(mem.read_int(a[9] + 8, 8)?, &mut bytes)?;
            pattern = Some(
                String::from_utf16(
                    &bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| "invalid directory search UTF-16")?,
            );
        }
        let capacity = a[6] as usize;
        let result = if a[1] != 0 || a[2] != 0 {
            Err(50)
        } else if a[7] != 2 {
            Err(124)
        } else if capacity > 16 * 1024 * 1024 {
            Err(8)
        } else {
            mem.check(a[5], capacity, WRITE)?;
            self.files.directory_batch(
                self.files
                    .filesystem()
                    .ok_or("directory query has no mounted filesystem")?,
                a[0],
                capacity,
                a[8] as u8 != 0,
                a[10] as u8 != 0,
                pattern,
            )
        };
        let (status, size) = match &result {
            Ok(batch) => (batch.status, batch.bytes.len()),
            Err(error) => (nt_error(*error), 0),
        };
        let mut iosb = [0u8; 16];
        iosb[..4].copy_from_slice(&status.to_le_bytes());
        iosb[8..].copy_from_slice(&(size as u64).to_le_bytes());
        if let Ok(batch) = &result {
            mem.write(a[5], &batch.bytes)?;
        }
        mem.write(a[4], &iosb)?;
        if let Ok(batch) = result {
            if batch.status != BUFFER_TOO_SMALL {
                batch.commit();
            }
        }
        Ok(status as u64)
    }
    pub(in crate::x64::runtime) fn full_directory_info(
        &mut self,
        mem: &mut Memory,
        a: &[u64],
    ) -> Result<u64, String> {
        if a[3] > 16 * 1024 * 1024 {
            self.set_error(mem, 8)?;
            return Ok(0);
        }
        mem.check(a[2], a[3] as usize, WRITE)?;
        let result = self.files.directory_batch(
            self.files
                .filesystem()
                .ok_or("directory query has no mounted filesystem")?,
            a[0],
            a[3] as usize,
            false,
            a[1] == 15,
            None,
        );
        match result {
            Ok(batch) if batch.status == SUCCESS => {
                mem.write(a[2], &batch.bytes)?;
                batch.commit();
                Ok(1)
            }
            Ok(batch) => {
                let error = if batch.status == NO_MORE_FILES {
                    18
                } else {
                    24
                };
                if batch.status == NO_MORE_FILES {
                    batch.commit();
                }
                self.set_error(mem, error)?;
                Ok(0)
            }
            Err(error) => {
                self.set_error(mem, error)?;
                Ok(0)
            }
        }
    }
}
