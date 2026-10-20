//! Ordinary file queries and checked I/O output borrow only their actual resources.
use super::{attributes, change_time, filetime, identity, Files};
use crate::{
    win32::{catalog::FileApi, x64::text},
    x64::runtime::{mapping::Mappings, threads::Threads, Memory, WRITE},
};
use std::time::UNIX_EPOCH;

impl Files {
    pub(in crate::x64::runtime) fn query_call(
        &mut self,
        mem: &mut Memory,
        id: FileApi,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(match id {
            FileApi::GetFileInformationByHandle
            | FileApi::GetFileInformationByHandleEx
            | FileApi::GetFileTime => {
                let result = (|| -> Result<Vec<u8>, u32> {
                    let (meta, path) = self.metadata(args[0])?;
                    let attributes = attributes(
                        &meta,
                        path.canonicalize()
                            .ok()
                            .is_some_and(|p| self.is_not_indexed(&p)),
                    );
                    let kind = if id == FileApi::GetFileInformationByHandleEx {
                        args[1]
                    } else {
                        u64::MAX
                    };
                    let mut data = match kind {
                        u64::MAX if id == FileApi::GetFileTime => vec![0; 24],
                        u64::MAX => vec![0; 52],
                        0 => vec![0; 40],  // FILE_BASIC_INFO
                        1 => vec![0; 24],  // FILE_STANDARD_INFO
                        9 => vec![0; 8], // FILE_ATTRIBUTE_TAG_INFO: host regular files have no reparse tag.
                        18 => vec![0; 24], // FILE_ID_INFO: real host volume and inode identity.
                        _ => return Err(50),
                    };
                    if id == FileApi::GetFileInformationByHandleEx && args[3] < data.len() as u64 {
                        return Err(24);
                    }
                    if kind == u64::MAX || kind == 0 {
                        let offset = if id == FileApi::GetFileInformationByHandle {
                            4
                        } else {
                            0
                        };
                        for (n, time) in [meta.created(), meta.accessed(), meta.modified()]
                            .into_iter()
                            .enumerate()
                        {
                            // GetFileTime permits independent null outputs.
                            if id != FileApi::GetFileTime || args[n + 1] != 0 {
                                data[offset + n * 8..offset + n * 8 + 8]
                                    .copy_from_slice(&filetime(time)?.to_le_bytes());
                            }
                        }
                    }
                    match kind {
                        u64::MAX if id == FileApi::GetFileTime => {}
                        u64::MAX => {
                            let (volume, index, links, _) = identity(&meta)?;
                            data[..4].copy_from_slice(&attributes.to_le_bytes());
                            data[28..32].copy_from_slice(&(volume as u32).to_le_bytes());
                            data[32..36]
                                .copy_from_slice(&((meta.len() >> 32) as u32).to_le_bytes());
                            data[36..40].copy_from_slice(&(meta.len() as u32).to_le_bytes());
                            data[40..44].copy_from_slice(&links.to_le_bytes());
                            data[44..48].copy_from_slice(&((index >> 32) as u32).to_le_bytes());
                            data[48..52].copy_from_slice(&(index as u32).to_le_bytes());
                        }
                        0 => {
                            data[24..32].copy_from_slice(&change_time(&meta)?.to_le_bytes());
                            data[32..36].copy_from_slice(&attributes.to_le_bytes());
                        }
                        1 => {
                            let (_, _, links, allocated) = identity(&meta)?;
                            data[..8].copy_from_slice(&allocated.to_le_bytes());
                            data[8..16].copy_from_slice(&meta.len().to_le_bytes());
                            data[16..20].copy_from_slice(&links.to_le_bytes());
                            // FILE_FLAG_DELETE_ON_CLOSE is rejected at open, so pending is false.
                            data[21] = meta.is_dir() as u8;
                        }
                        9 => data[..4].copy_from_slice(&attributes.to_le_bytes()),
                        18 => {
                            let (volume, index, _, _) = identity(&meta)?;
                            data[..8].copy_from_slice(&volume.to_le_bytes());
                            data[8..16].copy_from_slice(&index.to_le_bytes());
                        }
                        _ => unreachable!(),
                    }
                    Ok(data)
                })();
                match result {
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        0
                    }
                    Ok(data) if id == FileApi::GetFileTime => {
                        for &address in &args[1..4] {
                            if address != 0 {
                                mem.check(address, 8, WRITE)?;
                            }
                        }
                        for (n, &address) in args[1..4].iter().enumerate() {
                            if address != 0 {
                                mem.write(address, &data[n * 8..n * 8 + 8])?;
                            }
                        }
                        1
                    }
                    Ok(data) => {
                        mem.write(
                            args[if id == FileApi::GetFileInformationByHandle {
                                1
                            } else {
                                2
                            }],
                            &data,
                        )?;
                        1
                    }
                }
            }
            FileApi::GetFileSizeEx | FileApi::GetFileSize | FileApi::FlushFileBuffers => {
                let result = if id == FileApi::FlushFileBuffers {
                    self.flush(args[0])
                } else {
                    self.size(args[0])
                };
                match result {
                    Ok(value) if id == FileApi::GetFileSizeEx => {
                        mem.write_int(args[1], 8, value)?;
                        1
                    }
                    Ok(value) if id == FileApi::GetFileSize => {
                        if args[1] != 0 {
                            mem.write_int(args[1], 4, value >> 32)?;
                        }
                        threads.set_error(mem, 0)?;
                        value as u32 as u64
                    }
                    Ok(value) => value,
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        if id == FileApi::GetFileSize {
                            u32::MAX as u64
                        } else {
                            0
                        }
                    }
                }
            }
            FileApi::SetFilePointerEx => {
                if args[2] != 0 {
                    mem.check(args[2], 8, WRITE)?;
                }
                match self.seek(args[0], args[1] as i64, args[3] as u32) {
                    Ok(position) => {
                        if args[2] != 0 {
                            mem.write_int(args[2], 8, position)?;
                        }
                        1
                    }
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        0
                    }
                }
            }
            FileApi::GetFileAttributesW
            | FileApi::GetFileAttributesA
            | FileApi::GetFileAttributesExW
            | FileApi::GetFileAttributesExA => {
                let extended = matches!(
                    id,
                    FileApi::GetFileAttributesExW | FileApi::GetFileAttributesExA
                );
                if extended && args[1] != 0 {
                    threads.set_error(mem, 87)?;
                    0
                } else {
                    let wide = id.wide();
                    let path = text::path_name(mem, args[0], wide)?;
                    let fs = self
                        .filesystem()
                        .ok_or("Win64 file query has no mounted filesystem")?;
                    match fs.resolve(&path).and_then(|p| {
                        std::fs::metadata(&p)
                            .map(|meta| (p, meta))
                            .map_err(|e| crate::fs::win_error(&e))
                    }) {
                        Ok((path, meta)) => {
                            let attributes = (if meta.is_dir() { 0x10 } else { 0 })
                                | if path
                                    .canonicalize()
                                    .ok()
                                    .is_some_and(|p| self.is_not_indexed(&p))
                                {
                                    0x2000
                                } else {
                                    0
                                }
                                | meta.permissions().readonly() as u64;
                            let attributes = if attributes == 0 { 0x80 } else { attributes };
                            if extended {
                                let mut data = [0; 36];
                                data[..4].copy_from_slice(&(attributes as u32).to_le_bytes());
                                for (offset, time) in [
                                    (4, meta.created()),
                                    (12, meta.accessed()),
                                    (20, meta.modified()),
                                ] {
                                    let time = time.map_err(|e| {
                                        format!("host file timestamp unavailable: {e}")
                                    })?;
                                    let ticks = match time.duration_since(UNIX_EPOCH) {
                                        Ok(d) => {
                                            116444736000000000i128 + (d.as_nanos() / 100) as i128
                                        }
                                        Err(e) => {
                                            116444736000000000i128
                                                - (e.duration().as_nanos() / 100) as i128
                                        }
                                    };
                                    let ticks = u64::try_from(ticks)
                                        .map_err(|_| "file timestamp outside FILETIME range")?;
                                    data[offset..offset + 8].copy_from_slice(&ticks.to_le_bytes());
                                }
                                let size = if meta.is_dir() { 0 } else { meta.len() };
                                data[28..32].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
                                data[32..36].copy_from_slice(&(size as u32).to_le_bytes());
                                mem.write(args[2], &data)?;
                                1
                            } else {
                                attributes
                            }
                        }
                        Err(error) => {
                            threads.set_error(mem, error)?;
                            if extended {
                                0
                            } else {
                                u32::MAX as u64
                            }
                        }
                    }
                }
            }
            FileApi::SetCurrentDirectoryW | FileApi::SetCurrentDirectoryA => {
                let path = text::path_name(mem, args[0], id.wide())?;
                match self
                    .filesystem_mut()
                    .ok_or("Win64 has no mounted filesystem")?
                    .set_cwd(&path)
                {
                    Ok(()) => 1,
                    Err(error) => {
                        threads.set_error(mem, error)?;
                        0
                    }
                }
            }
            FileApi::GetCurrentDirectoryW | FileApi::GetCurrentDirectoryA => {
                let path = self
                    .filesystem()
                    .ok_or("Win64 has no mounted filesystem")?
                    .cwd();
                let wide = id.wide();
                let bytes: Vec<u8> = if wide {
                    path.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect()
                } else {
                    crate::codepage::encode(&(path + "\0"), 932)
                        .map_err(|e| format!("cwd codepage error {e}"))?
                        .0
                };
                let required = bytes.len() / if wide { 2 } else { 1 };
                if args[0] < required as u64 {
                    required as u64
                } else {
                    mem.write(args[1], &bytes)?;
                    required as u64 - 1
                }
            }
            _ => unreachable!("file query route requires a query ID"),
        })
    }

    pub(in crate::x64::runtime) fn read_output(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        count: usize,
        threads: &Threads,
    ) -> Result<u64, String> {
        let mut bytes = vec![0; count];
        let result = match self.read(args[0], &mut bytes) {
            Ok(read) => {
                mem.write(args[1], &bytes[..read])?;
                mem.write_int(args[3], 4, read as u64)?;
                1
            }
            Err(error) => {
                mem.write_int(args[3], 4, 0)?;
                threads.set_error(mem, error)?;
                0
            }
        };
        Ok(result)
    }

    pub(in crate::x64::runtime) fn write_output(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        mappings: &Mappings,
        threads: &Threads,
    ) -> Result<u64, String> {
        if args[4] != 0 || args[2] > 16 * 1024 * 1024 || args[3] == 0 {
            return Err("unsupported Win64 file write overlap/size/result pointer".into());
        }
        mem.check(args[3], 4, WRITE)?;
        let mut bytes = vec![0; args[2] as usize];
        mem.read(args[1], &mut bytes)?;
        let position = self.position(args[0]);
        let metadata = self.metadata(args[0]).map(|(m, _)| m);
        let write = position.and_then(|position| {
            metadata.and_then(|metadata| {
                self.write(args[0], &bytes)
                    .map(|written| (position, metadata, written))
            })
        });
        let result = match write {
            Ok((position, metadata, written)) => {
                mappings.update(mem, &metadata, position, &bytes[..written])?;
                mem.write_int(args[3], 4, written as u64)?;
                1
            }
            Err(error) => {
                mem.write_int(args[3], 4, 0)?;
                threads.set_error(mem, error)?;
                0
            }
        };
        Ok(result)
    }
}
