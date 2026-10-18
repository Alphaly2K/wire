//! Queries describe loaded guest images and the logical built-in service providers.
use super::{devices, dxgi, kernel_export, registry, sockets, sspi, unwind, user, winhttp};
use super::{Memory, Runtime64, WRITE};

/// Module adapters request control; the gateway owns CPU, trace and count commits.
pub(super) enum ModuleControlResult {
    Value(u64),
    Returned { value: u64, unblock: bool },
    Load { path: String, flags: u64 },
}

impl Runtime64 {
    pub(super) fn control_module_call(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::ModuleApi,
        args: &[u64],
    ) -> Result<ModuleControlResult, String> {
        use crate::win32::catalog::ModuleApi;
        Ok(ModuleControlResult::Value(match id {
            ModuleApi::LoadLibraryA
            | ModuleApi::LoadLibraryW
            | ModuleApi::LoadLibraryExA
            | ModuleApi::LoadLibraryExW => {
                let flags = if id.extended() { args[2] } else { 0 };
                if id.extended() && (args[1] != 0 || flags & !0x1808 != 0) {
                    return Err("unsupported Win64 LoadLibraryEx flags/file".into());
                }
                let module = Self::path_name(mem, args[0], id.wide())?;
                if flags & 8 != 0 && (flags & 0x1800 != 0 || !Self::absolute_module_path(&module)) {
                    self.set_error(mem, 87)?;
                    return Ok(ModuleControlResult::Returned {
                        value: 0,
                        unblock: false,
                    });
                }
                match self.native_module(&module, true) {
                    Ok(handle) => handle,
                    Err(error) => {
                        let path = self.files.filesystem().ok_or(126u32).and_then(|fs| {
                            self.module_path(fs, &module, flags)
                                .map(|path| fs.guest_path(&path))
                        });
                        if let Ok(path) = path {
                            if !self.threads.enabled {
                                return Err(error);
                            }
                            return Ok(ModuleControlResult::Load { path, flags });
                        }
                        self.set_error(mem, path.unwrap_err())?;
                        0
                    }
                }
            }
            ModuleApi::FreeLibrary => {
                if matches!(
                    self.modules.release(args[0])?,
                    crate::win32::x64::modules::Release::GuestReference
                ) {
                    return Ok(ModuleControlResult::Returned {
                        value: 1,
                        unblock: false,
                    });
                }
                1
            }
            ModuleApi::GetProcAddress => {
                // Windows resolves a NULL module against the main executable.
                let module_handle = if args[0] == 0 {
                    self.image_base
                } else {
                    args[0]
                };
                let symbol = if args[1] <= 65535 {
                    format!("#{}", args[1])
                } else {
                    Self::ascii(mem, args[1])?
                };
                if self.api_inventory.is_some() {
                    let provider = self
                        .modules
                        .name(module_handle)
                        .cloned()
                        .unwrap_or_else(|| "?".into());
                    self.inventory_record("lookup", &provider, &symbol);
                }
                self.trace_note(format!(
                    "GetProcAddress lookup module={:#x} symbol={symbol}",
                    args[0]
                ));
                if let Some(exports) = self.modules.exports(module_handle) {
                    let address = if args[1] <= 65535 {
                        exports.ordinals.get(&(args[1] as u16)).copied()
                    } else {
                        exports.names.get(&Self::ascii(mem, args[1])?).copied()
                    };
                    if address.is_none() {
                        self.set_error(mem, 127)?;
                    }
                    address.unwrap_or(0)
                } else {
                    let Some(module) = self.modules.name(module_handle) else {
                        self.set_error(
                            mem,
                            if module_handle == self.image_base {
                                127
                            } else {
                                126
                            },
                        )?;
                        return Ok(ModuleControlResult::Returned {
                            value: 0,
                            unblock: true,
                        });
                    };
                    let function = if args[1] <= 65535 {
                        if module.eq_ignore_ascii_case("ws2_32.dll") {
                            sockets::ordinal(&symbol).unwrap_or(&symbol).to_owned()
                        } else if module.eq_ignore_ascii_case("d3d12.dll") {
                            dxgi::symbol(module, &symbol).to_owned()
                        } else if module.eq_ignore_ascii_case("oleaut32.dll") {
                            crate::win32::catalog::bstr_symbol(module, &symbol).to_owned()
                        } else {
                            return Err("Win64 native ordinal lookup is not implemented".into());
                        }
                    } else {
                        Self::ascii(mem, args[1])?
                    };
                    let provider = if matches!(
                        module.as_str(),
                        "api-ms-win-core-winrt-string-l1-1-0.dll"
                            | "api-ms-win-core-winrt-l1-1-0.dll"
                    ) {
                        "combase.dll"
                    } else if module.starts_with("api-ms-win-core-") {
                        "kernel32.dll"
                    } else {
                        module.as_str()
                    }
                    .to_owned();
                    let known = (matches!(provider.as_str(), "kernel32.dll" | "kernelbase.dll")
                        && kernel_export(&function))
                        || (matches!(
                            provider.as_str(),
                            "ntdll.dll" | "kernel32.dll" | "kernelbase.dll"
                        ) && unwind::is_call(&function))
                        || (provider == "ntdll.dll" && function == "NtQueryDirectoryFile")
                        || (provider == "ws2_32.dll" && sockets::is_call(&function))
                        || (provider == "secur32.dll" && sspi::arity(&function).is_some())
                        || (provider == "dxgi.dll" && dxgi::export(&function))
                        || (provider == "d3d12.dll" && dxgi::d3d12_export(&function))
                        || (provider == "d3d11.dll" && dxgi::d3d11_export(&function))
                        || matches!(
                            crate::win32::catalog::resolve(
                                crate::win32::catalog::Architecture::X64,
                                &provider,
                                &function
                            ),
                            crate::win32::catalog::Resolution::Bound(contract)
                                if contract.lookup_dlls(crate::win32::catalog::Architecture::X64)
                                    .contains(&provider.as_str())
                        )
                        || devices::export(&provider, &function)
                        || (provider == "user32.dll" && user::arity(&function).is_some())
                        || (provider == "advapi32.dll" && registry::arity(&function).is_some())
                        || winhttp::export(&provider, &function)
                        || (matches!(provider.as_str(), "kernel32.dll" | "kernelbase.dll")
                            && matches!(
                                function.as_str(),
                                "GetErrorMode"
                                    | "GetTempPath2W"
                                    | "WaitOnAddress"
                                    | "WakeByAddressSingle"
                                    | "WakeByAddressAll"
                                    | "AreFileApisANSI"
                                    | "InitializeCriticalSectionEx"
                            ))
                        || self.gateways.values().any(|export| {
                            export.split_once('!').is_some_and(|(dll, n)| {
                                dll.eq_ignore_ascii_case(&provider) && n == function
                            })
                        });
                    if !known {
                        self.set_error(mem, 127)?;
                        0
                    } else {
                        self.register(&provider, &function)
                    }
                }
            }
            _ => unreachable!("scalar module entered the control adapter"),
        }))
    }
    pub(super) fn scalar_module_call(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::ModuleApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use crate::win32::catalog::ModuleApi;
        Ok(match id {
            ModuleApi::GetModuleHandleExA | ModuleApi::GetModuleHandleExW => {
                return self.module_handle_ex(mem, id, args);
            }
            ModuleApi::GetModuleHandleA | ModuleApi::GetModuleHandleW => {
                if args[0] == 0 {
                    self.image_base
                } else {
                    let module = if id.wide() {
                        Self::utf16(mem, args[0])?
                    } else {
                        Self::ascii(mem, args[0])?
                    };
                    let handle = self.native_module(&module, false)?;
                    if handle == 0 {
                        self.set_error(mem, 126)?;
                    }
                    handle
                }
            }
            ModuleApi::GetModuleFileNameA | ModuleApi::GetModuleFileNameW => {
                let path = if args[0] == 0 || args[0] == self.image_base {
                    &self.exe_path
                } else {
                    self.modules
                        .path(args[0])
                        .ok_or("Win64 native module filename query not implemented")?
                };
                let wide = id.wide();
                let bytes = if wide {
                    path.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&(path.clone() + "\0"), 932)
                        .map_err(|e| format!("codepage error {e}"))?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                let capacity = args[2] as u32 as usize;
                if capacity == 0 {
                    self.set_error(mem, 122)?;
                    0
                } else {
                    let units = bytes.len() / width;
                    let n = capacity.min(units);
                    let mut output = bytes[..n * width].to_vec();
                    if n < units {
                        output[(n - 1) * width..].fill(0);
                    }
                    mem.write(args[1], &output)?;
                    if n < units {
                        self.set_error(mem, 122)?;
                        n as u64
                    } else {
                        (units - 1) as u64
                    }
                }
            }
            ModuleApi::DisableThreadLibraryCalls => {
                // Preserve explicit failure when static TLS notifications are needed.
                self.set_error(
                    mem,
                    if self.modules.exports(args[0]).is_none() {
                        6
                    } else {
                        50
                    },
                )?;
                0
            }
            ModuleApi::GetDllDirectoryA
            | ModuleApi::GetDllDirectoryW
            | ModuleApi::SetDllDirectoryA
            | ModuleApi::SetDllDirectoryW => {
                return self.dll_directory_call(mem, id, args);
            }
            _ => unreachable!("module control entered the scalar adapter"),
        })
    }
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
        id: crate::win32::catalog::ModuleApi,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = id.wide();
        if matches!(
            id,
            crate::win32::catalog::ModuleApi::SetDllDirectoryA
                | crate::win32::catalog::ModuleApi::SetDllDirectoryW
        ) {
            let directory = if args[0] == 0 {
                None
            } else {
                let text = Self::path_name(mem, args[0], wide)?;
                if text.is_empty() {
                    Some(text)
                } else {
                    let fs = self
                        .files
                        .filesystem()
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
        id: crate::win32::catalog::ModuleApi,
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
            let module = Self::path_name(mem, args[1], id.wide())?;
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
        id: crate::win32::catalog::ModuleQueryApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use crate::win32::catalog::ModuleQueryApi;
        if args[0] != u64::MAX {
            self.set_error(mem, 6)?;
            return Ok(0);
        }
        if matches!(
            id,
            ModuleQueryApi::EnumProcessModules | ModuleQueryApi::EnumProcessModulesEx
        ) {
            let extended = id == ModuleQueryApi::EnumProcessModulesEx;
            if extended && args[4] > 3 {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            let handles: Vec<_> = if extended && args[4] == 1 {
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
        let base = if args[1] == 0 && id != ModuleQueryApi::GetModuleInformation {
            self.image_base
        } else {
            args[1]
        };
        let Some(module) = self.modules.name(base) else {
            self.set_error(mem, 6)?;
            return Ok(0);
        };
        if id == ModuleQueryApi::GetModuleInformation {
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
        let value = if matches!(
            id,
            ModuleQueryApi::GetModuleBaseNameA | ModuleQueryApi::GetModuleBaseNameW
        ) {
            module.clone()
        } else {
            self.modules
                .path(base)
                .cloned()
                .unwrap_or_else(|| format!("C:\\Windows\\System32\\{module}"))
        };
        let wide = id.wide();
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
