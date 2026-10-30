//! PE32 CRT scalar services use shared byte/parsing and x87 semantics.
use super::{crt_memory, ServiceError};
use crate::{
    machine::State,
    memory::{Memory, READ, WRITE},
    process::Loaded,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Export {
    pub name: &'static str,
    pub slots: u32,
}
impl Export {
    pub(super) fn lookup(name: &str) -> Option<Self> {
        let (name, slots) = match name {
            "__acrt_iob_func" => ("__acrt_iob_func", 1),
            "__getmainargs" => ("__getmainargs", 5),
            "__p___argc" => ("__p___argc", 0),
            "__p___argv" => ("__p___argv", 0),
            "__p___initenv" => ("__p___initenv", 0),
            "__p___wargv" => ("__p___wargv", 0),
            "__p___winitenv" => ("__p___winitenv", 0),
            "__p__acmdln" => ("__p__acmdln", 0),
            "__p__commode" => ("__p__commode", 0),
            "__p__daylight" => ("__p__daylight", 0),
            "__p__environ" => ("__p__environ", 0),
            "__p__fmode" => ("__p__fmode", 0),
            "__p__pgmptr" => ("__p__pgmptr", 0),
            "__p__timezone" => ("__p__timezone", 0),
            "__p__wcmdln" => ("__p__wcmdln", 0),
            "__p__wenviron" => ("__p__wenviron", 0),
            "__p__wpgmptr" => ("__p__wpgmptr", 0),
            "__set_app_type" => ("__set_app_type", 1),
            "__setusermatherr" => ("__setusermatherr", 1),
            "_cexit" => ("_cexit", 0),
            "_configure_narrow_argv" => ("_configure_narrow_argv", 1),
            "_configure_wide_argv" => ("_configure_wide_argv", 1),
            "_controlfp_s" => ("_controlfp_s", 3),
            "_crt_at_quick_exit" => ("_crt_at_quick_exit", 1),
            "_crt_atexit" => ("_crt_atexit", 1),
            "_crt_debugger_hook" => ("_crt_debugger_hook", 1),
            "_exit" => ("_exit", 1),
            "_initialize_narrow_environment" => ("_initialize_narrow_environment", 0),
            "_initialize_onexit_table" => ("_initialize_onexit_table", 1),
            "_initialize_wide_environment" => ("_initialize_wide_environment", 0),
            "_initterm" => ("_initterm", 2),
            "_initterm_e" => ("_initterm_e", 2),
            "_lock" => ("_lock", 1),
            "_register_onexit_function" => ("_register_onexit_function", 2),
            "_register_thread_local_exe_atexit_callback" => {
                ("_register_thread_local_exe_atexit_callback", 1)
            }
            "_set_app_type" => ("_set_app_type", 1),
            "_set_fmode" => ("_set_fmode", 1),
            "_set_invalid_parameter_handler" => ("_set_invalid_parameter_handler", 1),
            "_set_new_mode" => ("_set_new_mode", 1),
            "_unlock" => ("_unlock", 1),
            "exit" => ("exit", 1),
            "qsort" => ("qsort", 4),
            "quick_exit" => ("quick_exit", 1),
            "setvbuf" => ("setvbuf", 4),

            "_errno" => ("_errno", 0),
            "_ftol" => ("_ftol", 0),
            "_ftol2" => ("_ftol2", 0),
            "_ftol2_sse" => ("_ftol2_sse", 0),
            "_CIpow" => ("_CIpow", 0),
            "floor" => ("floor", 2),
            "ldexp" => ("ldexp", 3),
            "pow" => ("pow", 4),
            "time" => ("time", 1),
            "_time32" => ("_time32", 1),
            "_mbsinc" => ("_mbsinc", 1),
            "_mbsnbcpy" => ("_mbsnbcpy", 3),
            "memcpy" => ("memcpy", 3),
            "memmove" => ("memmove", 3),
            "memset" => ("memset", 3),
            "memcmp" => ("memcmp", 3),
            "toupper" => ("toupper", 1),
            "tolower" => ("tolower", 1),
            "strcmp" => ("strcmp", 2),
            "strncmp" => ("strncmp", 3),
            "strlen" => ("strlen", 1),
            "strtoul" => ("strtoul", 3),
            _ => return None,
        };
        Some(Self { name, slots })
    }
    pub(super) fn callback(self) -> bool {
        matches!(
            self.name,
            "qsort" | "_initterm" | "_initterm_e" | "exit" | "_exit" | "_cexit" | "quick_exit"
        )
    }
    pub(super) fn wall(self) -> bool {
        matches!(self.name, "time" | "_time32")
    }
}
pub(crate) fn provider(dll: &str) -> Option<String> {
    let dll = crate::pe::linker::module_name(dll).ok()?;
    (dll == "msvcrt.dll"
        || dll == "ucrtbase.dll"
        || dll.starts_with("api-ms-win-crt-")
        || dll.starts_with("msvcr") && dll.ends_with(".dll"))
    .then_some(dll)
}
struct Access<'a>(&'a mut Memory);
impl crt_memory::Memory for Access<'_> {
    type Error = ServiceError;
    fn invalid(message: &'static str) -> ServiceError {
        ServiceError::Unavailable(message)
    }
    fn check(&self, p: u32, length: usize) -> Result<(), ServiceError> {
        Ok(self.0.check(u64::from(p), length, READ)?)
    }
    fn read_bytes(&self, p: u32, bytes: &mut [u8]) -> Result<(), ServiceError> {
        Ok(self.0.read(u64::from(p), bytes)?)
    }
    fn write_bytes(&mut self, p: u32, bytes: &[u8]) -> Result<(), ServiceError> {
        Ok(self.0.write(u64::from(p), bytes)?)
    }
}
#[derive(Default)]
pub(crate) struct Crt {
    pub(super) app_type: BTreeMap<String, u32>,
    pub(super) locks: BTreeMap<(String, u32), (u32, u32)>,
    pub(crate) normal: BTreeMap<String, Vec<u64>>,
    pub(crate) quick: BTreeMap<String, Vec<u64>>,
    pub(super) startup: super::crt_startup::State,
    errno: BTreeMap<u32, u64>,
}
impl Crt {
    pub(crate) fn new(command: &str) -> Self {
        Self {
            startup: super::crt_startup::State::new(command),
            ..Self::default()
        }
    }
}
pub(super) fn errno(image: &mut Loaded, memory: &mut Memory) -> Result<u64, ServiceError> {
    let tid = image.threads.active;
    if let Some(&p) = image.crt.errno.get(&tid) {
        return Ok(p);
    }
    if image.crt.errno.len() >= 4096 {
        return Err(ServiceError::Unavailable("CRT errno capacity exhausted"));
    }
    let p = image.data_pages.allocate(memory, 4);
    if p == 0 {
        return Err(ServiceError::Unavailable("CRT errno allocation failed"));
    }
    image.crt.errno.insert(tid, p);
    Ok(p)
}
pub(super) fn failure(
    image: &mut Loaded,
    memory: &mut Memory,
    error: u32,
    value: u32,
) -> Result<u64, ServiceError> {
    let p = errno(image, memory)?;
    memory.write_int(p, 4, u64::from(error))?;
    Ok(u64::from(value))
}
pub(super) fn dispatch(
    export: Export,
    arguments: &[u64; 16],
    state: &mut State,
    memory: &mut Memory,
    image: &mut Loaded,
    wall: Option<u64>,
) -> Result<u64, ServiceError> {
    let a = arguments.map(|n| n as u32);
    if export.name == "_controlfp_s" {
        return controlfp(&a, state, memory, image);
    }
    if export.name == "_errno" {
        return errno(image, memory);
    }
    if export.name == "strtoul" {
        let parsed = crt_memory::strtoul(&Access(memory), a[0], a[2])?;
        if a[1] != 0 {
            memory.check(u64::from(a[1]), 4, WRITE)?;
        }
        let error = if parsed.error.is_some() {
            let p = errno(image, memory)?;
            memory.check(p, 4, WRITE)?;
            Some(p)
        } else {
            None
        };
        if a[1] != 0 {
            memory.write_int(u64::from(a[1]), 4, u64::from(parsed.end))?;
        }
        if let Some(p) = error {
            memory.write_int(p, 4, u64::from(parsed.error.unwrap()))?;
        }
        return Ok(u64::from(parsed.value));
    }
    let mut fpu = state.fpu;
    let mut error = None;
    let value = match export.name {
        "_ftol" | "_ftol2" | "_ftol2_sse" => fpu.crt_ftol(),
        "floor" => {
            fpu.crt_floor(u64::from(a[0]) | (u64::from(a[1]) << 32));
            0
        }
        "ldexp" => {
            if fpu.crt_ldexp(u64::from(a[0]) | (u64::from(a[1]) << 32), a[2] as i32) {
                error = Some(34);
            }
            0
        }
        "pow" | "_CIpow" => {
            error = fpu.crt_pow((export.name == "pow").then_some((
                u64::from(a[0]) | (u64::from(a[1]) << 32),
                u64::from(a[2]) | (u64::from(a[3]) << 32),
            )));
            0
        }
        _ => {
            let seconds = if export.wall() {
                wall.ok_or(ServiceError::Unavailable(
                    "CRT time requires a FILETIME sample",
                ))?
                .checked_sub(116_444_736_000_000_000)
                .map(|ticks| ticks / 10_000_000)
                .unwrap_or(u64::MAX)
            } else {
                0
            };
            return crt_memory::call(export.name, &a, &mut Access(memory), seconds)?
                .map(u64::from)
                .ok_or(ServiceError::Unavailable("unknown CRT scalar export"));
        }
    };
    if let Some(error) = error {
        let p = errno(image, memory)?;
        memory.write_int(p, 4, u64::from(error))?;
    }
    state.fpu = fpu;
    if matches!(export.name, "_ftol" | "_ftol2" | "_ftol2_sse") {
        state.regs[2] = value >> 32;
    }
    Ok(value)
}

fn fp_control(state: &State) -> u32 {
    let mut flags = 0;
    for (bit, flag) in [(0, 0x10), (1, 0x80000), (2, 8), (3, 4), (4, 2), (5, 1)] {
        if state.fpu.cw & (1 << bit) != 0 {
            flags |= flag;
        }
    }
    flags |= u32::from((state.fpu.cw >> 10) & 3) << 8;
    flags |= match (state.fpu.cw >> 8) & 3 {
        0 => 0x20000,
        2 => 0x10000,
        _ => 0,
    };
    flags |= u32::from(state.fpu.cw & 0x1000) << 6;
    flags |= match (state.mxcsr & 0x40 != 0, state.mxcsr & 0x8000 != 0) {
        (false, false) => 0,
        (true, true) => 0x1000000,
        (true, false) => 0x2000000,
        (false, true) => 0x3000000,
    };
    if (u32::from(state.fpu.cw) & 0x3f) != (state.mxcsr >> 7) & 0x3f
        || u32::from((state.fpu.cw >> 10) & 3) != (state.mxcsr >> 13) & 3
    {
        flags |= 0x80000000;
    }
    flags
}
fn controlfp(
    a: &[u32; 16],
    state: &mut State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    const VALID: u32 = 0x030f031f;
    let (value, mask) = (a[1], a[2]);
    let combined = (fp_control(state) & !mask) | (value & mask);
    if a[0] == 0
        || value & mask & !VALID != 0
        || mask & 0x30000 != 0 && combined & 0x30000 == 0x30000
    {
        return failure(image, memory, 22, 22);
    }
    memory.check(u64::from(a[0]), 4, WRITE)?;
    let mut next = state.clone();
    for (bit, flag) in [(0, 0x10), (2, 8), (3, 4), (4, 2), (5, 1)] {
        if mask & flag != 0 {
            next.fpu.cw = (next.fpu.cw & !(1 << bit)) | u16::from(value & flag != 0) << bit;
            next.mxcsr =
                (next.mxcsr & !(1 << (bit + 7))) | u32::from(value & flag != 0) << (bit + 7);
        }
    }
    if mask & 0x300 != 0 {
        let rc = ((fp_control(state) & !mask) | (value & mask)) >> 8 & 3;
        next.fpu.cw = (next.fpu.cw & !0xc00) | ((rc as u16) << 10);
        next.mxcsr = (next.mxcsr & !0x6000) | (rc << 13);
    }
    if mask & 0x30000 != 0 {
        let pc = match combined & 0x30000 {
            0 => 3,
            0x10000 => 2,
            _ => 0,
        };
        next.fpu.cw = (next.fpu.cw & !0x300) | (pc << 8);
    }
    if mask & 0x40000 != 0 {
        next.fpu.cw = (next.fpu.cw & !0x1000) | u16::from(value & 0x40000 != 0) << 12;
    }
    if mask & 0x3000000 != 0 {
        let dn = ((fp_control(state) & !mask) | (value & mask)) & 0x3000000;
        next.mxcsr = (next.mxcsr & !0x8040)
            | match dn {
                0 => 0,
                0x1000000 => 0x8040,
                0x2000000 => 0x40,
                _ => 0x8000,
            };
    }
    next.fpu.sw = if next.fpu.sw & !next.fpu.cw & 0x3f != 0 {
        next.fpu.sw | 0x8080
    } else {
        next.fpu.sw & !0x8080
    };
    memory.write_int(u64::from(a[0]), 4, u64::from(fp_control(&next)))?;
    state.fpu = next.fpu;
    state.mxcsr = next.mxcsr;
    Ok(0)
}
