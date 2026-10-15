//! Confined directory searches with 64-bit handles and fault-atomic cursor advancement.
use super::{files, Memory, Runtime64, WRITE};
use crate::win32::catalog::FileApi;
use std::{collections::BTreeMap, path::PathBuf};
struct Search {
    entries: Vec<PathBuf>,
    next: usize,
}
pub(super) struct Searches {
    handles: BTreeMap<u64, Search>,
    next: u64,
}
impl Default for Searches {
    fn default() -> Self {
        Self {
            handles: BTreeMap::new(),
            next: super::TEB + 0x670000,
        }
    }
}
impl Runtime64 {
    fn find_data(&self, path: &std::path::Path, wide: bool) -> Result<Vec<u8>, u32> {
        let meta = path.metadata().map_err(|e| crate::fs::win_error(&e))?;
        let filename = path.file_name().ok_or(123u32)?.to_str().ok_or(1113u32)?;
        let name = if wide {
            filename
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        } else {
            crate::codepage::encode(filename, 932)?.0
        };
        if name.len() >= if wide { 520 } else { 260 } {
            return Err(206);
        }
        let mut bytes = vec![0; if wide { 592 } else { 320 }];
        bytes[..4].copy_from_slice(
            &files::attributes(&meta, self.files.is_not_indexed(path)).to_le_bytes(),
        );
        for (offset, time) in [
            (4, meta.created()),
            (12, meta.accessed()),
            (20, meta.modified()),
        ] {
            let ticks = match files::filetime(time) {
                Ok(ticks) => ticks,
                Err(50) => 0,
                Err(error) => return Err(error),
            };
            bytes[offset..offset + 8].copy_from_slice(&ticks.to_le_bytes());
        }
        let size = if meta.is_dir() { 0 } else { meta.len() };
        bytes[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[44..44 + name.len()].copy_from_slice(&name);
        // The confined filesystem has no DOS 8.3 aliases or reparse-point service.
        Ok(bytes)
    }
    pub(super) fn find_call(
        &mut self,
        mem: &mut Memory,
        id: FileApi,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = id.wide();
        let first = matches!(
            id,
            FileApi::FindFirstFileA
                | FileApi::FindFirstFileW
                | FileApi::FindFirstFileExA
                | FileApi::FindFirstFileExW
        );
        if id == FileApi::FindClose {
            if self.searches.handles.remove(&args[0]).is_some() {
                return Ok(1);
            }
            self.set_error(mem, 6)?;
            return Ok(0);
        }
        if !first && !self.searches.handles.contains_key(&args[0]) {
            self.set_error(mem, 6)?;
            return Ok(0);
        }
        let extended = matches!(id, FileApi::FindFirstFileExA | FileApi::FindFirstFileExW);
        if first && extended && (args[1] > 1 || args[3] != 0 || args[4] != 0 || args[5] & !2 != 0) {
            self.set_error(mem, 87)?;
            return Ok(u64::MAX);
        }
        let output = args[if first && extended { 2 } else { 1 }];
        mem.check(output, if wide { 592 } else { 320 }, WRITE)?;
        if first {
            let pattern = Self::path_name(mem, args[0], wide)?.replace('\\', "/");
            let entries = (|| -> Result<Vec<PathBuf>, u32> {
                if pattern.ends_with('/') {
                    return Err(2);
                }
                let (directory, wildcard) = pattern.rsplit_once('/').unwrap_or((".", &pattern));
                let fs = self.files.filesystem().ok_or(3u32)?;
                let directory = fs.resolve(if directory == "C:" {
                    "C:/"
                } else if directory.is_empty() {
                    "/"
                } else {
                    directory
                })?;
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(directory).map_err(|e| crate::fs::win_error(&e))? {
                    let entry = entry.map_err(|e| crate::fs::win_error(&e))?;
                    if crate::file_api::wildcard(wildcard, &entry.file_name().to_string_lossy()) {
                        if let Ok(path) = fs.resolve(&fs.guest_path(&entry.path())) {
                            entries.push(path);
                            if entries.len() > 65536 {
                                return Err(8);
                            }
                        }
                    }
                }
                entries.sort_by_key(|path| path.to_string_lossy().to_lowercase());
                if entries.is_empty() {
                    Err(2)
                } else {
                    Ok(entries)
                }
            })();
            let prepared = entries.and_then(|entries| {
                if self.searches.handles.len() >= 4096
                    || self.searches.next >= super::TEB + 0x680000
                {
                    return Err(8);
                }
                let data = self.find_data(&entries[0], wide)?;
                Ok((entries, data))
            });
            let (entries, data) = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.set_error(mem, error)?;
                    return Ok(u64::MAX);
                }
            };
            mem.write(output, &data)?;
            let handle = self.searches.next;
            self.searches.next += 16;
            self.searches
                .handles
                .insert(handle, Search { entries, next: 1 });
            Ok(handle)
        } else {
            let search = &self.searches.handles[&args[0]];
            let prepared = search
                .entries
                .get(search.next)
                .ok_or(18u32)
                .and_then(|path| self.find_data(path, wide));
            let data = match prepared {
                Ok(data) => data,
                Err(error) => {
                    self.set_error(mem, error)?;
                    return Ok(0);
                }
            };
            mem.write(output, &data)?;
            self.searches.handles.get_mut(&args[0]).unwrap().next += 1;
            Ok(1)
        }
    }
}
