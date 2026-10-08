//! Trusted-classlib MonoIO entries. Handles belong to the current service owner;
//! managed byte arrays are pinned only across the bounded service operation.
use super::*;
const OUTPUT: u64 = SCRATCH + 65536;
fn result<T: Copy>(
    error: *mut i32,
    failure: T,
    operation: impl FnOnce(&mut Services) -> Result<T, u32>,
) -> T {
    let value = with_services(|services| {
        if error.is_null() {
            return Err(87);
        }
        operation(services).map(Some)
    });
    let status = if value.is_some() {
        0
    } else {
        unsafe { *libc::__error() }
    };
    if !error.is_null() {
        unsafe {
            error.write_unaligned(status);
        }
    }
    value.unwrap_or(failure)
}
impl Services {
    fn last_error(&self) -> u32 {
        self.memory.read_int(TEB + 0x68, 4).unwrap_or(998) as u32
    }
    fn boolean_call(&mut self, name: &str, args: &[u64]) -> Result<(), u32> {
        if self.call(name, args)? == 0 {
            Err(self.last_error())
        } else {
            Ok(())
        }
    }
    fn path(&mut self, name: &str) -> Result<u64, u32> {
        let bytes: Vec<u8> = name
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        if bytes.len() > 65536 {
            return Err(206);
        }
        self.memory.write(SCRATCH, &bytes).map_err(|_| 998u32)?;
        Ok(SCRATCH)
    }
}
pub(super) unsafe extern "C" fn open(
    path: *const u16,
    mode: i32,
    access: i32,
    share: i32,
    options: i32,
    error: *mut i32,
) -> Object {
    result(error, usize::MAX as Object, |services| {
        let name = unsafe { wide(path) }?;
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
            if let Err(error) = services.boolean_call("SetFilePointerEx", &[handle, 0, 0, 2]) {
                let _ = services.call("CloseHandle", &[handle]);
                return Err(error);
            }
        }
        Ok(handle as Object)
    })
}
pub(super) unsafe extern "C" fn close(handle: Object, error: *mut i32) -> i32 {
    result(error, 0, |services| {
        services
            .boolean_call("CloseHandle", &[handle as u64])
            .map(|()| 1)
    })
}
pub(super) unsafe extern "C" fn flush(handle: Object, error: *mut i32) -> i32 {
    result(error, 0, |services| {
        services
            .boolean_call("FlushFileBuffers", &[handle as u64])
            .map(|()| 1)
    })
}
pub(super) unsafe extern "C" fn file_type(handle: Object, error: *mut i32) -> i32 {
    result(error, 0, |services| {
        let kind = services.call("GetFileType", &[handle as u64])? as i32;
        if kind == 0 {
            Err(services.last_error())
        } else {
            Ok(kind)
        }
    })
}
pub(super) unsafe extern "C" fn length(handle: Object, error: *mut i32) -> i64 {
    result(error, -1, |services| {
        services.boolean_call("GetFileSizeEx", &[handle as u64, OUTPUT])?;
        Ok(services.memory.read_int(OUTPUT, 8).map_err(|_| 998u32)? as i64)
    })
}
pub(super) unsafe extern "C" fn seek(
    handle: Object,
    offset: i64,
    origin: i32,
    error: *mut i32,
) -> i64 {
    result(error, -1, |services| {
        services.boolean_call(
            "SetFilePointerEx",
            &[handle as u64, offset as u64, OUTPUT, origin as u32 as u64],
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
    result(error, -1, |services| {
        if writing {
            services
                .memory
                .write(SCRATCH, unsafe { std::slice::from_raw_parts(data, length) })
                .map_err(|_| 998u32)?;
        }
        services.boolean_call(
            if writing { "WriteFile" } else { "ReadFile" },
            &[handle as u64, SCRATCH, length as u64, OUTPUT, 0],
        )?;
        let transferred = services.memory.read_int(OUTPUT, 4).map_err(|_| 998u32)? as usize;
        if transferred > length {
            return Err(998);
        }
        if !writing {
            let mut bytes = vec![0; transferred];
            services
                .memory
                .read(SCRATCH, &mut bytes)
                .map_err(|_| 998u32)?;
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), data, transferred);
            }
        }
        Ok(transferred as i32)
    })
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
