//! Guest scheduling boundary for PE32 native presentation.
use crate::{
    api::ApiState,
    host::ui::{Frame, Presenter, WindowRequest},
    mem::GuestMem,
    thread::Sched,
};

pub(crate) fn poll(mem: &GuestMem, state: &mut ApiState, sched: &mut Sched) {
    crate::dsound::poll(mem, state, sched);
    if !state.presenter.poll_due() {
        return;
    }
    let mut presenter = std::mem::take(&mut state.presenter);
    poll_ready(&mut presenter, mem, state, sched);
    state.presenter = presenter;
}

fn poll_ready(presenter: &mut Presenter, mem: &GuestMem, state: &mut ApiState, sched: &mut Sched) {
    let windows = state
        .user32
        .windows
        .iter()
        .filter(|(_, w)| {
            w.tid != 0
                && w.parent == 0
                && w.style & 0x40000000 == 0
                && w.client_size().iter().all(|&n| n > 1)
        })
        .map(|(&id, window)| {
            let [width, height] = window.client_size();
            WindowRequest {
                id,
                title: if presenter.active() {
                    String::from_utf16_lossy(&window.text)
                } else {
                    String::new()
                },
                width: width as u32,
                height: height as u32,
                visible: window.style & 0x10000000 != 0,
            }
        })
        .collect();
    presenter.sync_windows(windows);
    for _ in 0..512 {
        let Some(event) = presenter.next_event() else {
            break;
        };
        crate::user32::host_message(
            state,
            sched,
            event.window,
            event.message,
            event.wparam,
            event.lparam,
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
