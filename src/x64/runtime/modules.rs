//! Queries describe loaded guest images and the logical built-in service providers.
use super::{Memory, Runtime64, WRITE};

pub(super) fn is_query(name: &str) -> bool {
    matches!(
        name.strip_prefix("K32").unwrap_or(name),
        "EnumProcessModules"
            | "EnumProcessModulesEx"
            | "GetModuleInformation"
            | "GetModuleBaseNameA"
            | "GetModuleBaseNameW"
            | "GetModuleFileNameExA"
            | "GetModuleFileNameExW"
    )
}
impl Runtime64 {
    pub(super) fn absolute_module_path(name: &str) -> bool {
        crate::win32::x64::modules::Modules::absolute_path(name)
    }
    /// Supported confined search order, including the process SetDllDirectory.
    /// PATH and AddDllDirectory cookies are not part of this bring-up loader.
    pub(crate) fn module_path(
        &self,
        fs: &crate::fs::GuestFs,
        name: &str,
        flags: u64,
    ) -> Result<std::path::PathBuf, u32> {
        self.module_path_with_search(fs, name, flags, None)
    }
    pub(crate) fn module_path_with_search(
        &self,
        fs: &crate::fs::GuestFs,
        name: &str,
        flags: u64,
        altered_directory: Option<&str>,
    ) -> Result<std::path::PathBuf, u32> {
        self.modules
            .resolve_path(fs, &self.exe_path, name, flags, altered_directory)
    }
    pub(super) fn dll_directory_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = name.ends_with('W');
        if name.starts_with("Set") {
            let directory = if args[0] == 0 {
                None
            } else {
                let text = Self::path_name(mem, args[0], wide)?;
                if text.is_empty() {
                    Some(text)
                } else {
                    let fs = self
                        .fs
                        .as_ref()
                        .ok_or("Win64 DLL directory requires filesystem")?;
                    match fs.resolve(&text) {
                        Ok(path) => Some(fs.guest_path(&path)),
                        Err(error) => {
                            self.set_error(mem, error)?;
                            return Ok(0);
                        }
                    }
                }
            };
            self.modules.set_directory(directory);
            return Ok(1);
        }
        let text = self.modules.directory().unwrap_or("");
        let bytes: Vec<u8> = if wide {
            text.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            crate::codepage::encode(&format!("{text}\0"), 932)
                .map_err(|e| format!("DLL directory codepage error {e}"))?
                .0
        };
        let length = bytes.len() / if wide { 2 } else { 1 } - 1;
        if args[0] == 0 {
            return Ok(if length == 0 { 0 } else { length as u64 + 1 });
        }
        if args[0] <= length as u64 {
            self.set_error(mem, 122)?;
            return Ok(length as u64 + 1);
        }
        mem.write(args[1], &bytes)?;
        Ok(length as u64)
    }
    pub(super) fn module_handle_ex(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        mem.check(args[2], 8, WRITE)?;
        let flags = args[0];
        if flags & !7 != 0 || flags & 3 == 3 {
            self.set_error(mem, 87)?;
            return Ok(0);
        }
        let base = if flags & 4 != 0 {
            self.modules.containing_image(args[1]).unwrap_or(0)
        } else if args[1] == 0 {
            self.image_base
        } else {
            let module = Self::path_name(mem, args[1], name.ends_with('W'))?;
            self.native_module(&module, false)?
        };
        if base == 0 || !self.modules.contains(base) {
            mem.write_int(args[2], 8, 0)?;
            self.set_error(mem, 126)?;
            return Ok(0);
        }
        let update = self.modules.prepare_reference(base, flags)?;
        mem.write_int(args[2], 8, base)?;
        update.commit();
        Ok(1)
    }
    pub(super) fn format_message(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        let flags = args[0] as u32;
        if flags & !0x37ff != 0 || flags & 0x1400 == 0 || flags & 0x1400 == 0x1400 {
            self.set_error(mem, 87)?;
            return Ok(0);
        }
        if flags & 0xff != 0 || flags & 0x800 != 0 || !matches!(args[3], 0 | 0x400 | 0x409) {
            self.set_error(mem, 50)?;
            return Ok(0);
        }
        let wide = name.ends_with('W');
        let text = if flags & 0x400 != 0 {
            Self::path_name(mem, args[1], wide)?
        } else {
            let message = match args[2] {
                0 => "The operation completed successfully.",
                2 => "The system cannot find the file specified.",
                3 => "The system cannot find the path specified.",
                5 => "Access is denied.",
                6 => "The handle is invalid.",
                8 => "Not enough memory resources are available to process this command.",
                32 => "The process cannot access the file because it is being used by another process.",
                50 => "The request is not supported.",
                87 => "The parameter is incorrect.",
                122 => "The data area passed to a system call is too small.",
                126 => "The specified module could not be found.",
                127 => "The specified procedure could not be found.",
                183 => "Cannot create a file when that file already exists.",
                203 => "The system could not find the environment option that was entered.",
                _ => { self.set_error(mem, 317)?; return Ok(0); }
            };
            format!("{message}\r\n")
        };
        if flags & 0x200 == 0 && (args[6] != 0 || text.contains('%')) {
            self.set_error(mem, 50)?;
            return Ok(0);
        }
        let bytes: Vec<u8> = if wide {
            text.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            crate::codepage::encode(&(text + "\0"), 932)
                .map_err(|e| format!("message codepage error {e}"))?
                .0
        };
        let width = if wide { 2 } else { 1 };
        let count = bytes.len() / width - 1;
        if flags & 0x100 != 0 {
            mem.check(args[4], 8, WRITE)?;
            let size = (count as u64 + 1).max(args[5]) * width as u64;
            let pointer = self.heap_alloc(mem, size)?;
            if pointer == 0 {
                return Ok(0);
            }
            mem.write(pointer, &bytes)?;
            mem.write_int(args[4], 8, pointer)?;
            self.local_blocks.insert(pointer);
        } else if args[5] <= count as u64 {
            self.set_error(mem, 122)?;
            return Ok(0);
        } else {
            mem.write(args[4], &bytes)?;
        }
        Ok(count as u64)
    }
    pub(super) fn module_query(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        if args[0] != u64::MAX {
            self.set_error(mem, 6)?;
            return Ok(0);
        }
        let query = name.strip_prefix("K32").unwrap_or(name);
        if query.starts_with("EnumProcessModules") {
            if query.ends_with("Ex") && args[4] > 3 {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            let handles: Vec<_> = if query.ends_with("Ex") && args[4] == 1 {
                Vec::new() // No 32-bit modules in an AMD64 process.
            } else {
                self.modules.bases().collect()
            };
            let count = handles.len().min(args[2] as usize / 8);
            mem.check(args[3], 4, WRITE)?;
            if count != 0 {
                mem.check(args[1], count * 8, WRITE)?;
            }
            for (n, &handle) in handles[..count].iter().enumerate() {
                mem.write_int(args[1] + n as u64 * 8, 8, handle)?;
            }
            mem.write_int(args[3], 4, handles.len() as u64 * 8)?;
            return Ok(1);
        }
        let base = if args[1] == 0 && query != "GetModuleInformation" {
            self.image_base
        } else {
            args[1]
        };
        let Some(module) = self.modules.name(base) else {
            self.set_error(mem, 6)?;
            return Ok(0);
        };
        if query == "GetModuleInformation" {
            if args[3] < 24 {
                self.set_error(mem, 122)?;
                return Ok(0);
            }
            let Some((size, entry)) = self.modules.image(base) else {
                self.set_error(mem, 50)?; // Built-ins have no guest PE image.
                return Ok(0);
            };
            let mut bytes = vec![0; 24];
            bytes[..8].copy_from_slice(&base.to_le_bytes());
            bytes[8..12].copy_from_slice(&size.to_le_bytes());
            bytes[16..24].copy_from_slice(&entry.to_le_bytes());
            mem.write(args[2], &bytes)?;
            return Ok(1);
        }
        let value = if query.starts_with("GetModuleBaseName") {
            module.clone()
        } else {
            self.modules
                .path(base)
                .cloned()
                .unwrap_or_else(|| format!("C:\\Windows\\System32\\{module}"))
        };
        let wide = query.ends_with('W');
        let bytes: Vec<u8> = if wide {
            value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            crate::codepage::encode(&(value + "\0"), 932)
                .map_err(|e| format!("module codepage error {e}"))?
                .0
        };
        let width = if wide { 2 } else { 1 };
        let capacity = args[3] as u32 as usize;
        if capacity == 0 {
            self.set_error(mem, 122)?;
            return Ok(0);
        }
        let units = bytes.len() / width;
        let count = units.min(capacity);
        let mut output = bytes[..count * width].to_vec();
        if count < units {
            output[(count - 1) * width..].fill(0);
        }
        mem.write(args[2], &output)?;
        Ok(if count < units { count } else { units - 1 } as u64)
    }
}
