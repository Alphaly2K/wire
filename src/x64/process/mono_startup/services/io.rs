//! Trusted-classlib MonoIO entries. Handles belong to the current service owner;
//! managed byte arrays are pinned only across the bounded service operation.
use super::*;

fn result<T: Copy>(
    error: *mut i32,
    failure: T,
    operation: impl FnOnce(&mut Services<'_>) -> Result<T, u32>,
) -> T {
    if error.is_null() {
        unsafe {
            *libc::__error() = 87;
        }
        return failure;
    }
    let value = service(operation);
    unsafe {
        error.write_unaligned(value.as_ref().err().copied().unwrap_or(0) as i32);
    }
    value.unwrap_or(failure)
}
pub(super) unsafe extern "C" fn open(
    path: *const u16,
    mode: i32,
    access: i32,
    share: i32,
    options: i32,
    error: *mut i32,
) -> Object {
    let name = match unsafe { wide(path) } {
        Ok(name) => name,
        Err(status) => {
            if !error.is_null() {
                unsafe {
                    error.write_unaligned(status as i32);
                }
            }
            return usize::MAX as Object;
        }
    };
    result(error, u64::MAX, move |services| {
        let access = match access {
            1 => 0x80000000,
            2 => 0x40000000,
            3 => 0xc0000000,
            _ => return Err(87),
        };
        let disposition = match mode {
            1..=5 => mode as u64,
            6 => 4,
            _ => return Err(87),
        };
        let flags = options as u32;
        // Async/encrypted/unknown modes need a production scheduler/contract.
        if flags & !0x9c000000 != 0 {
            return Err(50);
        }
        let path = services.path(&name)?;
        let handle = services.call(
            "kernel32.dll",
            "CreateFileW",
            &[
                path,
                access,
                share as u32 as u64,
                0,
                disposition,
                (flags | 0x80) as u64,
                0,
            ],
        )?;
        if handle == u64::MAX {
            return Err(services.last_error());
        }
        if mode == 6 {
            if let Err(error) = services.boolean("SetFilePointerEx", &[handle, 0, 0, 2]) {
                let _ = services.call("kernel32.dll", "CloseHandle", &[handle]);
                return Err(error);
            }
        }
        Ok(handle)
    }) as Object
}
pub(super) unsafe extern "C" fn close(handle: Object, error: *mut i32) -> i32 {
    let handle = handle as u64;
    result(error, 0, move |services| {
        services.boolean("CloseHandle", &[handle]).map(|()| 1)
    })
}
pub(super) unsafe extern "C" fn flush(handle: Object, error: *mut i32) -> i32 {
    let handle = handle as u64;
    result(error, 0, move |services| {
        services.boolean("FlushFileBuffers", &[handle]).map(|()| 1)
    })
}
pub(super) unsafe extern "C" fn file_type(handle: Object, error: *mut i32) -> i32 {
    let handle = handle as u64;
    result(error, 0, move |services| {
        let kind = services.call("kernel32.dll", "GetFileType", &[handle])? as i32;
        if kind == 0 {
            Err(services.last_error())
        } else {
            Ok(kind)
        }
    })
}
pub(super) unsafe extern "C" fn length(handle: Object, error: *mut i32) -> i64 {
    let handle = handle as u64;
    result(error, -1, move |services| {
        services.boolean("GetFileSizeEx", &[handle, OUTPUT])?;
        Ok(services.memory.read_int(OUTPUT, 8).map_err(|_| 998u32)? as i64)
    })
}
pub(super) unsafe extern "C" fn seek(
    handle: Object,
    offset: i64,
    origin: i32,
    error: *mut i32,
) -> i64 {
    let handle = handle as u64;
    result(error, -1, move |services| {
        services.boolean(
            "SetFilePointerEx",
            &[handle, offset as u64, OUTPUT, origin as u32 as u64],
        )?;
        Ok(services.memory.read_int(OUTPUT, 8).map_err(|_| 998u32)? as i64)
    })
}
struct Pin(Handle);
impl Drop for Pin {
    fn drop(&mut self) {
        unsafe {
            (API.get().unwrap().free_handle)(self.0);
        }
    }
}
unsafe fn transfer(
    handle: Object,
    array: Object,
    offset: i32,
    count: i32,
    error: *mut i32,
    writing: bool,
) -> i32 {
    let api = API.get().unwrap();
    if array.is_null() || offset < 0 || count < 0 {
        if !error.is_null() {
            unsafe {
                error.write_unaligned(87);
            }
        }
        return -1;
    }
    // Admitted callers are real classlib methods with byte[] signatures. Check
    // the element type before treating their native storage as scalar bytes.
    let class = unsafe { (api.object_class)(array) };
    if unsafe { (api.element_class)(class) } != api.byte_class as Object {
        if !error.is_null() {
            unsafe {
                error.write_unaligned(87);
            }
        }
        return -1;
    }
    let total = unsafe { (api.array_length)(array) };
    if offset as usize > total || count as usize > total - offset as usize {
        if !error.is_null() {
            unsafe {
                error.write_unaligned(87);
            }
        }
        return -1;
    }
    let pin = Pin(unsafe { (api.new_handle)(array, 1) });
    if pin.0.is_null() {
        if !error.is_null() {
            unsafe {
                error.write_unaligned(8);
            }
        }
        return -1;
    }
    let data = unsafe { (api.array_addr)(array, 1, offset as usize) }.cast::<u8>();
    let length = (count as usize).min(65536);
    if error.is_null() {
        unsafe {
            *libc::__error() = 87;
        }
        return -1;
    }
    let handle = handle as u64;
    let input = if writing {
        unsafe { std::slice::from_raw_parts(data, length) }.to_vec()
    } else {
        Vec::new()
    };
    let reply = service(move |services| {
        if writing {
            services.memory.write(SCRATCH, &input).map_err(|_| 998u32)?;
        }
        services.boolean(
            if writing { "WriteFile" } else { "ReadFile" },
            &[handle, SCRATCH, length as u64, OUTPUT, 0],
        )?;
        let transferred = services.memory.read_int(OUTPUT, 4).map_err(|_| 998u32)? as usize;
        if transferred > length {
            return Err(998);
        }
        let mut bytes = if writing {
            Vec::new()
        } else {
            vec![0; transferred]
        };
        if !writing {
            services
                .memory
                .read(SCRATCH, &mut bytes)
                .map_err(|_| 998u32)?;
        }
        Ok((transferred as i32, bytes))
    });
    let (transferred, bytes) = match reply {
        Ok(value) => value,
        Err(status) => {
            unsafe {
                error.write_unaligned(status as i32);
            }
            return -1;
        }
    };
    // The caller retains the byte[] pin for the entire wait and copy. No
    // managed/native pointer enters the owner queue, even for read/ref output.
    if !writing {
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        }
    }
    unsafe {
        error.write_unaligned(0);
    }
    transferred
}
pub(super) unsafe extern "C" fn read(
    handle: Object,
    array: Object,
    offset: i32,
    count: i32,
    error: *mut i32,
) -> i32 {
    unsafe { transfer(handle, array, offset, count, error, false) }
}
pub(super) unsafe extern "C" fn write(
    handle: Object,
    array: Object,
    offset: i32,
    count: i32,
    error: *mut i32,
) -> i32 {
    unsafe { transfer(handle, array, offset, count, error, true) }
}

pub(super) fn entries() -> Vec<Entry> {
    vec![
        (
            "System.IO",
            "MonoIO",
            "Open",
            "System.IntPtr",
            vec![
                "System.Char*",
                "System.IO.FileMode",
                "System.IO.FileAccess",
                "System.IO.FileShare",
                "System.IO.FileOptions",
                "System.IO.MonoIOError&",
            ],
            open as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "Close",
            "System.Boolean",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            close as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "Flush",
            "System.Boolean",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            flush as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetFileType",
            "System.IO.MonoFileType",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            file_type as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "GetLength",
            "System.Int64",
            vec!["System.IntPtr", "System.IO.MonoIOError&"],
            length as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "Seek",
            "System.Int64",
            vec![
                "System.IntPtr",
                "System.Int64",
                "System.IO.SeekOrigin",
                "System.IO.MonoIOError&",
            ],
            seek as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "Read",
            "System.Int32",
            vec![
                "System.IntPtr",
                "System.Byte[]",
                "System.Int32",
                "System.Int32",
                "System.IO.MonoIOError&",
            ],
            read as *const () as usize,
        ),
        (
            "System.IO",
            "MonoIO",
            "Write",
            "System.Int32",
            vec![
                "System.IntPtr",
                "System.Byte[]",
                "System.Int32",
                "System.Int32",
                "System.IO.MonoIOError&",
            ],
            write as *const () as usize,
        ),
    ]
}
