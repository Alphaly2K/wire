//! PE32 codec retains the scheduler's historical two-state suspension policy.
use crate::{mem::GuestMem, thread::Sched, win32::catalog::ThreadPropertyApi};

pub(crate) fn call(
    id: ThreadPropertyApi,
    args: &[u32],
    mem: &mut GuestMem,
    threads: &mut Sched,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    use ThreadPropertyApi::*;
    let mut note = String::new();
    let value = match id {
        ExitCode => {
            let (code, running) = threads.exit_code(args[0]);
            mem.write_u32(args[1], code).map_err(|e| e.to_string())?;
            note = if running {
                "STILL_ACTIVE".into()
            } else {
                format!("exited, code {code:#x}")
            };
            1
        }
        Suspend | Resume => match threads.set_suspended(args[0], id == Suspend) {
            Ok(()) => 0,
            Err(error) => {
                *last_error = error;
                u32::MAX
            }
        },
        SetPriority => {
            note = "no-op (cooperative scheduler)".into();
            1
        }
        GetPriority => 0,
        SetAffinity => {
            let error =
                if args[0] != u32::MAX - 1 && threads.thread_index_by_handle(args[0]).is_none() {
                    Some(6)
                } else if args[1] != 1 {
                    Some(87)
                } else {
                    None
                };
            match error {
                Some(error) => {
                    *last_error = error;
                    note = format!("common API error {error:#x}");
                    0
                }
                None => {
                    note = "common process/string/COM allocation contract".into();
                    1
                }
            }
        }
        GetErrorMode | SetErrorMode | Id => unreachable!("PE32 thread property remains ABI-only"),
    };
    Ok((value, note))
}
