//! Public PE32 USER compatibility entry points; state lives in win32::user::pe32.
pub(crate) use crate::win32::user::pe32::{
    dialog_integer, ControlRequest, CreationRequest, DefaultRequest, DialogRequest, MessageRequest,
    PaintRequest, PaintingOperation, WindowRequest, WindowStateRequest,
};
pub use crate::win32::user::pe32::{Timer, User32, Window, WindowClass};
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::{Cpu, GuestCallOutcome},
    mem::GuestMem,
    thread::Sched,
};

pub fn find_class(
    mem: &GuestMem,
    state: &ApiState,
    id: u32,
    instance: u32,
    wide: bool,
) -> Result<Option<u16>, String> {
    state.user32.find_class(mem, id, instance, wide)
}
/// Queue a host input event at a guest scheduling boundary.
pub fn host_message(
    state: &mut ApiState,
    sched: &mut Sched,
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
) {
    state.user32.host_message(
        &mut state.last_error,
        sched,
        state.start,
        [hwnd, message, wparam, lparam],
    );
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Option<Result<(GuestCallOutcome, String), String>> {
    if dll != "user32.dll" {
        return None;
    }
    if name == "RedrawWindow" {
        return Some(crate::x86::process::painting::redraw(
            a, cpu, mem, api, state, log, sched, max_instr,
        ));
    }
    // Compatibility facade for callers with names. Bound API gateways use
    // the typed ID directly and never resolve the export on the hot path.
    use crate::win32::catalog::{resolve, ApiId, Architecture, Resolution};
    if let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) {
        return match contract.id {
            ApiId::Dialog(id) if !id.message_box() => Some(crate::x86::process::dialog::call(
                id, a, cpu, mem, api, state, log, sched, max_instr,
            )),
            ApiId::Painting(id) => Some(crate::x86::process::painting::call(
                id, a, cpu, mem, api, state, log, sched, max_instr,
            )),
            _ => None,
        };
    }
    None
}
