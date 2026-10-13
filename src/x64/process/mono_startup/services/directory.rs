//! Windows corelib directory P/Invokes. Marshaled host storage is copied into
//! the current guest owner's VFS service; host pointers never enter Memory.
use super::*;

pub(super) struct Entry {
    pub(super) owner: &'static str,
    pub(super) name: &'static str,
    pub(super) scope: &'static str,
    pub(super) entry: &'static str,
    pub(super) flags: u32,
    pub(super) returns: &'static str,
    pub(super) parameters: Vec<&'static str>,
    pub(super) target: usize,
}
pub(super) fn entries() -> Vec<Entry> {
    vec![
        Entry {
            owner: "Kernel32",
            name: "CreateFilePrivate",
            scope: "kernel32.dll",
            entry: "CreateFileW",
            flags: 0x165,
            returns: "System.IntPtr",
            parameters: vec![
                "System.String",
                "System.Int32",
                "System.IO.FileShare",
                "Interop.Kernel32.SECURITY_ATTRIBUTES*",
                "System.IO.FileMode",
                "System.Int32",
                "System.IntPtr",
            ],
            target: create_file as *const () as usize,
        },
        Entry {
            owner: "Kernel32",
            name: "CloseHandle",
            scope: "kernel32.dll",
            entry: "CloseHandle",
            flags: 0x140,
            returns: "System.Boolean",
            parameters: vec!["System.IntPtr"],
            target: close as *const () as usize,
        },
        Entry {
            owner: "NtDll",
            name: "NtQueryDirectoryFile",
            scope: "ntdll.dll",
            entry: "NtQueryDirectoryFile",
            flags: 0x105,
            returns: "System.Int32",
            parameters: vec![
                "System.IntPtr",
                "System.IntPtr",
                "System.IntPtr",
                "System.IntPtr",
                "Interop.NtDll.IO_STATUS_BLOCK&",
                "System.IntPtr",
                "System.UInt32",
                "Interop.NtDll.FILE_INFORMATION_CLASS",
                "Interop.BOOLEAN",
                "Interop.UNICODE_STRING*",
                "Interop.BOOLEAN",
            ],
            target: query as *const () as usize,
        },
        Entry {
            owner: "NtDll",
            name: "RtlNtStatusToDosError",
            scope: "ntdll.dll",
            entry: "RtlNtStatusToDosError",
            flags: 0x101,
            returns: "System.UInt32",
            parameters: vec!["System.Int32"],
            target: dos_error as *const () as usize,
        },
    ]
}
unsafe extern "C" fn create_file(
    path: *const u16,
    access: u32,
    share: u32,
    security: Object,
    disposition: u32,
    flags: u32,
    template: Object,
) -> Object {
    // SECURITY_ATTRIBUTES and template-file ownership need an independent
    // contract. Ordinary classlib directory opens use NULL for both.
    if !security.is_null() || !template.is_null() {
        errno::<()>(50);
        return usize::MAX as Object;
    }
    let name = match unsafe { wide(path) } {
        Ok(name) => name,
        Err(error) => {
            errno::<()>(error);
            return usize::MAX as Object;
        }
    };
    let result = service(|s| {
        let path = s.path(&name)?;
        let handle = s.call(
            "kernel32.dll",
            "CreateFileW",
            &[
                path,
                access as u64,
                share as u64,
                0,
                disposition as u64,
                flags as u64,
                0,
            ],
        )?;
        if handle == u64::MAX {
            Err(s.last_error())
        } else {
            Ok(handle)
        }
    });
    match result {
        Ok(handle) => handle as Object,
        Err(error) => {
            errno::<()>(error);
            usize::MAX as Object
        }
    }
}
unsafe extern "C" fn close(handle: Object) -> i32 {
    service(|s| s.boolean("CloseHandle", &[handle as u64]).map(|()| 1)).unwrap_or_else(errno)
}
unsafe fn pattern(name: Object) -> Result<Option<Vec<u8>>, u32> {
    if name.is_null() {
        return Ok(None);
    }
    let length = unsafe { name.cast::<u16>().read_unaligned() } as usize;
    let maximum = unsafe { name.cast::<u8>().add(2).cast::<u16>().read_unaligned() } as usize;
    let buffer = unsafe {
        name.cast::<u8>()
            .add(8)
            .cast::<*const u8>()
            .read_unaligned()
    };
    if length > maximum
        || length > 512
        || !length.is_multiple_of(2)
        || (length != 0 && buffer.is_null())
    {
        return Err(87);
    }
    let mut bytes = vec![0; length];
    if length != 0 {
        unsafe { ptr::copy_nonoverlapping(buffer, bytes.as_mut_ptr(), length) };
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    String::from_utf16(&units).map_err(|_| 123u32)?;
    Ok(Some(bytes))
}
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn query(
    handle: Object,
    event: Object,
    apc: Object,
    context: Object,
    iosb: *mut u8,
    output: *mut u8,
    capacity: u32,
    class: u32,
    single: u8,
    filename: Object,
    restart: u8,
) -> i32 {
    if iosb.is_null()
        || (capacity != 0 && output.is_null())
        || capacity > 65536
        || single > 1
        || restart > 1
    {
        return 0xc000000du32 as i32;
    }
    if !event.is_null() || !apc.is_null() || !context.is_null() {
        return 0xc00000bbu32 as i32;
    }
    let pattern = match unsafe { pattern(filename) } {
        Ok(p) => p,
        Err(_) => return 0xc000000du32 as i32,
    };
    let result = service(|s| {
        let name = if let Some(pattern) = &pattern {
            s.memory
                .write_int(SCRATCH, 2, pattern.len() as u64)
                .map_err(|_| 998u32)?;
            s.memory
                .write_int(SCRATCH + 2, 2, pattern.len() as u64)
                .map_err(|_| 998u32)?;
            s.memory
                .write_int(SCRATCH + 8, 8, SCRATCH + 16)
                .map_err(|_| 998u32)?;
            s.memory.write(SCRATCH + 16, pattern).map_err(|_| 998u32)?;
            SCRATCH
        } else {
            0
        };
        let status = s.call(
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &[
                handle as u64,
                0,
                0,
                0,
                STACK + 0x800,
                OUTPUT,
                capacity as u64,
                class as u64,
                single as u64,
                name,
                restart as u64,
            ],
        )? as u32;
        let mut status_block = [0; 16];
        s.memory
            .read(STACK + 0x800, &mut status_block)
            .map_err(|_| 998u32)?;
        let length = u64::from_le_bytes(status_block[8..].try_into().unwrap());
        if u32::from_le_bytes(status_block[..4].try_into().unwrap()) != status
            || length > capacity as u64
            || (status != 0 && length != 0)
        {
            return Err(998);
        }
        let mut bytes = vec![0; length as usize];
        s.memory.read(OUTPUT, &mut bytes).map_err(|_| 998u32)?;
        Ok((status, status_block, bytes))
    });
    match result {
        Ok((status, status_block, bytes)) => {
            if !bytes.is_empty() {
                unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
            }
            unsafe { ptr::copy_nonoverlapping(status_block.as_ptr(), iosb, 16) };
            status as i32
        }
        Err(_) => 0xc0000001u32 as i32,
    }
}
unsafe extern "C" fn dos_error(status: i32) -> u32 {
    match status as u32 {
        0 => 0,
        0x80000006 => 18,
        0xc000000f => 2,
        0xc000003a => 3,
        0xc0000022 => 5,
        0xc0000008 => 6,
        0xc0000017 => 8,
        0xc00000bb => 50,
        0xc000000d => 87,
        0xc0000003 => 124,
        0xc0000103 => 267,
        0xc0000023 => 122,
        _ => 317,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_pinvokes_use_current_handles_and_copy_only_committed_bytes() {
        struct Folder(std::path::PathBuf);
        impl Drop for Folder {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Folder(std::env::temp_dir().join(format!(
                "wire-native-directory-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir(&root.0).unwrap();
        std::fs::write(root.0.join("alpha.txt"), [1, 2, 3]).unwrap();
        std::fs::write(root.0.join("日本語.txt"), [4]).unwrap();
        let mut memory = Memory::default();
        let stack = crate::x64::process::STACK;
        memory.map(stack, 65536, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(
            &mut memory,
            0x140000000,
            stack,
            65536,
            "directory-pinvoke-test",
        )
        .unwrap();
        runtime.set_filesystem(crate::fs::GuestFs::new(&root.0).unwrap());
        let path: Vec<_> = "C:\\".encode_utf16().chain(Some(0)).collect();
        let mut output = [0xa5u8; 4096];
        let mut iosb = [0xa5u8; 16];
        let ((), _) = callbacks::with_owner(&mut memory, &mut runtime, [0; 7], || {
            let handle = unsafe {
                create_file(
                    path.as_ptr(),
                    1,
                    7,
                    ptr::null_mut(),
                    3,
                    0x02000000,
                    ptr::null_mut(),
                )
            };
            assert_ne!(handle as usize, usize::MAX);
            let mut names = Vec::new();
            for expected_size in [3, 1] {
                assert_eq!(
                    unsafe {
                        query(
                            handle,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            iosb.as_mut_ptr(),
                            output.as_mut_ptr(),
                            4096,
                            2,
                            1,
                            ptr::null_mut(),
                            0,
                        )
                    },
                    0
                );
                let size = u64::from_le_bytes(iosb[8..].try_into().unwrap()) as usize;
                assert_eq!(u32::from_le_bytes(output[..4].try_into().unwrap()), 0);
                assert_eq!(
                    u64::from_le_bytes(output[40..48].try_into().unwrap()),
                    expected_size
                );
                assert_eq!(&output[size..], &[0xa5; 4096][size..]);
                let length = u32::from_le_bytes(output[60..64].try_into().unwrap()) as usize;
                let units: Vec<_> = output[68..68 + length]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                names.push(String::from_utf16(&units).unwrap());
                output.fill(0xa5);
            }
            assert_eq!(names, ["alpha.txt", "日本語.txt"]);
            assert_eq!(
                unsafe {
                    query(
                        handle,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        iosb.as_mut_ptr(),
                        output.as_mut_ptr(),
                        4096,
                        2,
                        1,
                        ptr::null_mut(),
                        0,
                    )
                } as u32,
                0x80000006
            );
            assert_eq!(iosb[8..], [0; 8]);
            assert_eq!(output, [0xa5; 4096]);
            assert_eq!(unsafe { dos_error(0x80000006u32 as i32) }, 18);
            assert_eq!(
                unsafe {
                    query(
                        handle,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        iosb.as_mut_ptr(),
                        output.as_mut_ptr(),
                        1,
                        2,
                        0,
                        ptr::null_mut(),
                        1,
                    )
                } as u32,
                0xc0000023
            );
            assert_eq!(output, [0xa5; 4096]);
            assert_eq!(
                unsafe {
                    query(
                        handle,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        iosb.as_mut_ptr(),
                        output.as_mut_ptr(),
                        4096,
                        2,
                        1,
                        ptr::null_mut(),
                        1,
                    )
                },
                0
            );
            iosb.fill(0xa5);
            output.fill(0xa5);
            assert_eq!(
                unsafe {
                    query(
                        handle,
                        1usize as Object,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        iosb.as_mut_ptr(),
                        output.as_mut_ptr(),
                        4096,
                        2,
                        1,
                        ptr::null_mut(),
                        0,
                    )
                } as u32,
                0xc00000bb
            );
            assert_eq!(iosb, [0xa5; 16]);
            assert_eq!(output, [0xa5; 4096]);
            assert_eq!(unsafe { close(handle) }, 1);
            assert_eq!(unsafe { close(handle) }, 0);
            assert_eq!(unsafe { *libc::__error() }, 6);
            assert_eq!(
                unsafe {
                    query(
                        handle,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        iosb.as_mut_ptr(),
                        output.as_mut_ptr(),
                        4096,
                        2,
                        1,
                        ptr::null_mut(),
                        0,
                    )
                } as u32,
                0xc0000008
            );
            assert_eq!(output, [0xa5; 4096]);
        })
        .unwrap();
        assert!(memory.read_int(SCRATCH, 1).is_err());
        iosb.fill(0xa5);
        assert_eq!(
            unsafe {
                query(
                    1usize as Object,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    iosb.as_mut_ptr(),
                    output.as_mut_ptr(),
                    4096,
                    2,
                    1,
                    ptr::null_mut(),
                    0,
                )
            } as u32,
            0xc0000001
        );
        assert_eq!(iosb, [0xa5; 16]);
        assert_eq!(output, [0xa5; 4096]);
    }
}
