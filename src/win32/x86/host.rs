//! Guest scheduling boundary for PE32 native presentation.
use crate::{api::ApiState, host::ui::Frame, mem::GuestMem, thread::Sched};

pub(crate) fn poll(mem: &GuestMem, state: &mut ApiState, sched: &mut Sched) {
    crate::dsound::poll(mem, state, sched);
    if !state.presenter.poll_due() {
        return;
    }
    let presenter = &mut state.presenter;
    let windows = state.user32.host_windows(presenter.active());
    presenter.sync_windows(windows);
    for _ in 0..512 {
        let Some(event) = presenter.next_event() else {
            break;
        };
        state.user32.host_message(
            &mut state.last_error,
            sched,
            state.start,
            [event.window, event.message, event.wparam, event.lparam],
        );
    }
    if !presenter.accept_frame(state.d3d9.frames) {
        return;
    }
    let hwnd = state.d3d9.last_presented;
    let Ok((width, height, pixels)) = state.graphics.window_snapshot(mem, hwnd) else {
        presenter.error = Some("presented window has no pixel surface".into());
        return;
    };
    let rgba = pixels
        .iter()
        .flat_map(|&c| [c as u8, (c >> 8) as u8, (c >> 16) as u8, 255])
        .collect();
    match Frame::new(hwnd, width, height, rgba) {
        Ok(frame) => presenter.submit(frame),
        Err(error) => presenter.error = Some(error),
    }
}
