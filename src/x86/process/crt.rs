//! CRT CPU/FPU results and initializer execution belong to Process32.
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
};

/// Minimal CRT (UCRT/msvcr) shims. All cdecl. Ok(None) = not covered (caller
/// logs it as unimplemented).
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    args: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<Option<crate::cpu::GuestCallOutcome>, String> {
    if !(dll.starts_with("api-ms-win-crt-")
        || dll == "ucrtbase.dll"
        || dll == "msvcrt.dll"
        || (dll.starts_with("msvcr") && dll.ends_with(".dll")))
    {
        return Ok(None);
    }
    let r = match name {
        "_errno" => state.errno_address(mem, sched.current_tid())?,
        "_ftol" | "_ftol2" | "_ftol2_sse" => {
            let result = cpu.fpu.crt_ftol();
            cpu.regs[crate::cpu::EDX] = (result >> 32) as u32;
            result as u32
        }
        "floor" => {
            cpu.fpu.crt_floor(args[0] as u64 | ((args[1] as u64) << 32));
            0
        }
        "ldexp" => {
            if cpu
                .fpu
                .crt_ldexp(args[0] as u64 | ((args[1] as u64) << 32), args[2] as i32)
            {
                let errno = state.errno_address(mem, sched.current_tid())?;
                mem.write_u32(errno, 34).map_err(|e| e.to_string())?;
            }
            0
        }
        "_CIpow" | "pow" => {
            let values = (name == "pow").then_some((
                args[0] as u64 | ((args[1] as u64) << 32),
                args[2] as u64 | ((args[3] as u64) << 32),
            ));
            if let Some(error) = cpu.fpu.crt_pow(values) {
                let errno = state.errno_address(mem, sched.current_tid())?;
                mem.write_u32(errno, error).map_err(|e| e.to_string())?;
            }
            0
        }
        "qsort" => {
            return super::crt_sort::qsort(
                args[0], args[1], args[2], args[3], cpu, mem, api, state, log, sched, max_instr,
            )
            .map(Some)
        }
        "__p__fmode" | "__p__commode" | "__p___argc" | "__p__timezone" | "__p__daylight"
        | "__p__acmdln" | "__p__wcmdln" => api
            .data_export(mem, dll, &name[4..], state.cmdline_a, state.cmdline_w)?
            .ok_or("unknown CRT data accessor")?,
        "_initterm" | "_initterm_e" => {
            // Walk an array of guest function pointers and call each
            // non-null initializer (C++ static ctors / C initializers).
            let mut a = args[0];
            let end = args[1];
            let mut first_err = 0u32;
            while a < end {
                let f = mem.read_u32(a).unwrap_or(0);
                if f != 0 {
                    let v = match crate::x86::process::call_guest(
                        cpu,
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        f,
                        &[],
                        max_instr,
                    )? {
                        crate::cpu::GuestCallOutcome::Returned(value) => value,
                        transfer => return Ok(Some(transfer)),
                    };
                    if name.ends_with("_e") && v != 0 && first_err == 0 {
                        first_err = v;
                    }
                }
                a = a.wrapping_add(4);
            }
            first_err
        }
        "strtoul" => state.crt_strtoul(args, mem, sched.current_tid())?,
        "exit" | "_exit" | "_cexit" | "quick_exit" => {
            cpu.halted = true;
            cpu.exit_code = Some(args[0]);
            0
        }
        _ => match state.crt_scalar(name, args, mem)? {
            Some(value) => value,
            None => return Ok(None),
        },
    };
    Ok(Some(crate::cpu::GuestCallOutcome::Returned(r)))
}
