//! Win64 MSGs and per-thread posted/native messages. Guest pointers remain u64.
use super::{Memory, Runtime64};
use crate::win32::catalog::MessageApi as M;
use crate::x64::{
    memory::WRITE,
    runtime::{threads::Block, WindowEvent},
};
#[derive(Clone, Copy)]
pub(in crate::x64::runtime) struct Message {
    pub event: WindowEvent,
    pub tid: u64,
    pub time: u32,
    pub point: [i32; 2],
    pub hardware: bool,
}
pub(in crate::x64::runtime) fn read(mem: &Memory, pointer: u64) -> Result<[u64; 5], String> {
    let mut data = [0; 48];
    mem.read(pointer, &mut data)?;
    Ok([
        u64::from_le_bytes(data[..8].try_into().unwrap()),
        u32::from_le_bytes(data[8..12].try_into().unwrap()) as u64,
        u64::from_le_bytes(data[16..24].try_into().unwrap()),
        u64::from_le_bytes(data[24..32].try_into().unwrap()),
        u32::from_le_bytes(data[32..36].try_into().unwrap()) as u64,
    ])
}
impl Runtime64 {
    fn message_index(&self, tid: u64, window: u64, min: u32, max: u32) -> Option<usize> {
        self.user.events.iter().position(|m| {
            m.tid == tid
                && (m.event.message == 0x12
                    || ((window == 0
                        || window == m.event.window
                        || (window == u64::MAX && m.event.window == 0))
                        && ((min == 0 && max == 0) || (min..=max).contains(&m.event.message))))
        })
    }
    pub(in crate::x64::runtime) fn message_ready(
        &self,
        tid: u64,
        window: u64,
        min: u32,
        max: u32,
    ) -> bool {
        self.message_index(tid, window, min, max).is_some()
    }
    pub(in crate::x64::runtime) fn message_call(
        &mut self,
        mem: &mut Memory,
        id: M,
        a: &[u64],
    ) -> Result<Option<u64>, String> {
        let tid = self.threads.active;
        let value = match id {
            M::Extra | M::Time | M::Position => {
                let state = self.user.message_state.get(&tid).copied().unwrap_or([0; 3]);
                state[match id {
                    M::Extra => 0,
                    M::Time => 1,
                    _ => 2,
                }]
            }
            M::SetExtra => {
                let state = self.user.message_state.entry(tid).or_default();
                std::mem::replace(&mut state[0], a[0])
            }
            M::Quit | M::PostA | M::PostW | M::PostThreadA | M::PostThreadW => {
                let quit = id == M::Quit;
                let thread = matches!(id, M::PostThreadA | M::PostThreadW);
                let window = if quit || thread { 0 } else { a[0] };
                let owner = if thread {
                    if !self
                        .threads
                        .entries
                        .get(&a[0])
                        .is_some_and(|t| t.exit.is_none())
                    {
                        self.set_error(mem, 1444)?;
                        return Ok(Some(0));
                    }
                    a[0]
                } else if window == 0 {
                    tid
                } else if let Some(w) = self.user.windows.get(&window) {
                    w.tid
                } else {
                    self.set_error(mem, if window == 65535 { 50 } else { 1400 })?;
                    return Ok(Some(0));
                };
                let message = if quit { 0x12 } else { a[1] as u32 };
                if !quit && matches!(message, 1 | 0xc | 0xd | 0x24 | 0x46 | 0x47 | 0x4a | 0x83) {
                    self.set_error(mem, 1159)?; // ERROR_MESSAGE_SYNC_ONLY
                    return Ok(Some(0));
                }
                if self.user.events.len() >= 4096 {
                    self.set_error(mem, 1816)?;
                    return Ok(Some(0));
                }
                self.user.events.push_back(Message {
                    event: WindowEvent {
                        window,
                        message,
                        wparam: if quit { a[0] } else { a[2] },
                        lparam: if quit { 0 } else { a[3] },
                    },
                    tid: owner,
                    time: self.start.elapsed().as_millis() as u32,
                    point: self.user.cursor.position(),
                    hardware: false,
                });
                (!quit) as u64
            }
            M::PeekA | M::PeekW | M::GetA | M::GetW => {
                let get = id.get();
                if (a[1] != 0 && a[1] != u64::MAX && !self.user.windows.contains_key(&a[1]))
                    || ((a[2] != 0 || a[3] != 0) && a[2] > a[3])
                {
                    self.set_error(
                        mem,
                        if a[1] != 0 && a[1] != u64::MAX && !self.user.windows.contains_key(&a[1]) {
                            1400
                        } else {
                            87
                        },
                    )?;
                    return Ok(Some(if get { u32::MAX as u64 } else { 0 }));
                }
                if !get && a[4] & !3 != 0 {
                    self.set_error(mem, 50)?;
                    return Ok(Some(0));
                }
                let Some(index) = self.message_index(tid, a[1], a[2] as u32, a[3] as u32) else {
                    if !get {
                        return Ok(Some(0));
                    }
                    mem.check(a[0], 48, WRITE)?;
                    self.block(Block::Messages {
                        window: a[1],
                        min: a[2] as u32,
                        max: a[3] as u32,
                    })?;
                    return Ok(None);
                };
                let m = self.user.events[index];
                let mut bytes = [0; 48];
                bytes[..8].copy_from_slice(&m.event.window.to_le_bytes());
                bytes[8..12].copy_from_slice(&m.event.message.to_le_bytes());
                bytes[16..24].copy_from_slice(&m.event.wparam.to_le_bytes());
                bytes[24..32].copy_from_slice(&m.event.lparam.to_le_bytes());
                bytes[32..36].copy_from_slice(&m.time.to_le_bytes());
                for (n, p) in m.point.into_iter().enumerate() {
                    bytes[36 + n * 4..40 + n * 4].copy_from_slice(&p.to_le_bytes());
                }
                mem.write(a[0], &bytes)?;
                // Native and posted messages in this bridge have no injected
                // extra-info tag. Publish metadata only after checked output.
                self.user.message_state.insert(
                    tid,
                    [
                        0,
                        m.time as u64,
                        (m.point[0] as u16 as u32 | ((m.point[1] as u16 as u32) << 16)) as u64,
                    ],
                );
                if get || a[4] & 1 != 0 {
                    self.user.events.remove(index);
                    if m.hardware {
                        self.user.keyboard.consumed64(
                            tid,
                            m.event.message,
                            m.event.wparam as u32,
                            m.event.lparam as u32,
                        );
                    }
                }
                if get && m.event.message == 0x12 {
                    0
                } else {
                    1
                }
            }
            M::Translate => {
                let words = read(mem, a[0])?;
                if !matches!(words[1], 0x100 | 0x101 | 0x104 | 0x105) {
                    return Ok(Some(0));
                }
                if matches!(words[1], 0x100 | 0x104) {
                    // Reserve enough capacity before consuming cached host text.
                    if self.user.events.len() > 4096 - 64 {
                        return Err("Win64 translated message capacity exceeded".into());
                    }
                    let text = self.user.keyboard.translate64(words);
                    let wide = self.user.windows.get(&words[0]).is_none_or(|w| w.wide);
                    let chars: Vec<_> = if wide {
                        text.into_iter().map(u32::from).collect()
                    } else {
                        crate::codepage::encode(&String::from_utf16_lossy(&text), 932)
                            .map_err(|e| format!("Win64 keyboard encoding error {e}"))?
                            .0
                            .into_iter()
                            .map(u32::from)
                            .collect()
                    };
                    for c in chars {
                        self.user.events.push_back(Message {
                            event: WindowEvent {
                                window: words[0],
                                message: if words[1] == 0x104 { 0x106 } else { 0x102 },
                                wparam: c as u64,
                                lparam: words[3],
                            },
                            tid,
                            time: self.start.elapsed().as_millis() as u32,
                            point: self.user.cursor.position(),
                            hardware: false,
                        });
                    }
                }
                1
            }
            _ => unreachable!(),
        };
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::{
        cpu::Cpu64,
        memory::{READ, WRITE},
        runtime::{Control, WindowStep},
    };
    const STACK: u64 = 0x700000000;
    const HWND: u64 = 0x7ffd00630000;
    fn fixture() -> (Runtime64, Memory) {
        let mut mem = Memory::default();
        mem.map(STACK, 8192, READ | WRITE).unwrap();
        let mut runtime = Runtime64::new(&mut mem, 0x140000000, STACK, 8192, "probe.exe").unwrap();
        runtime.threads.enabled = true;
        let procedure = runtime.register("user32.dll", "DefWindowProcW");
        runtime.user.windows.insert(
            HWND,
            super::super::Window {
                class: 0,
                procedure,
                instance: 0x140000000,
                parent: 0,
                menu: 0,
                style: 0x10000000,
                ex_style: 0,
                rect: [100, 200, 420, 440],
                client: [100, 200, 420, 440],
                data: 0,
                extra: Vec::new(),
                tid: 1,
                wide: true,
                title: "test".into(),
                ready: true,
            },
        );
        (runtime, mem)
    }
    #[test]
    fn mouse_tracking_reports_empty_state_and_real_absent_delivery() {
        use crate::win32::catalog::CursorApi::Track;
        let (mut r, mut m) = fixture();
        let data = STACK + 128;
        m.write_int(data, 4, 24).unwrap();
        m.write_int(data + 8, 8, HWND).unwrap();
        for flags in [1, 2, 3, 0x11, 0x12] {
            m.write_int(data + 4, 4, flags).unwrap();
            assert_eq!(r.cursor_call(&mut m, Track, &[data]).unwrap(), 0);
            assert_eq!(m.read_int(crate::x64::runtime::TEB + 0x68, 4).unwrap(), 50);
            assert!(r.user.events.is_empty());
        }
        m.write_int(data + 4, 4, 0x80000003).unwrap();
        assert_eq!(r.cursor_call(&mut m, Track, &[data]).unwrap(), 1);
        m.write_int(data + 4, 4, 0x40000000).unwrap();
        m.write_int(data + 8, 8, u64::MAX).unwrap();
        assert_eq!(r.cursor_call(&mut m, Track, &[data]).unwrap(), 1);
        assert_eq!(m.read_int(data, 4).unwrap(), 24);
        assert_eq!(m.read_int(data + 4, 4).unwrap(), 0);
        assert_eq!(m.read_int(data + 8, 8).unwrap(), 0);
        assert_eq!(m.read_int(data + 16, 8).unwrap(), 0);
        m.write_int(data + 4, 4, 2).unwrap();
        m.write_int(data + 8, 8, HWND ^ (1 << 40)).unwrap();
        assert_eq!(r.cursor_call(&mut m, Track, &[data]).unwrap(), 0);
        assert_eq!(
            m.read_int(crate::x64::runtime::TEB + 0x68, 4).unwrap(),
            1400
        );
        m.write_int(data, 4, 16).unwrap();
        assert_eq!(r.cursor_call(&mut m, Track, &[data]).unwrap(), 0);
        assert_eq!(m.read_int(crate::x64::runtime::TEB + 0x68, 4).unwrap(), 87);
        // QUERY writes the complete Win64 structure atomically.
        let crossing = STACK + 4090;
        m.write(crossing, &[0xaa; 24]).unwrap();
        m.write_int(crossing, 4, 24).unwrap();
        m.write_int(crossing + 4, 4, 0x40000000).unwrap();
        let mut before = [0; 24];
        m.read(crossing, &mut before).unwrap();
        m.protect(STACK + 4096, 4096, READ).unwrap();
        assert!(r.cursor_call(&mut m, Track, &[crossing]).is_err());
        let mut after = [0; 24];
        m.read(crossing, &mut after).unwrap();
        assert_eq!(after, before);
        assert!(crate::x64::runtime::import_supported(
            "user32.dll",
            "TrackMouseEvent"
        ));
        assert!(!crate::x64::runtime::import_supported(
            "kernel32.dll",
            "TrackMouseEvent"
        ));
    }
    #[test]
    fn message_metadata_is_per_thread_full_width_and_commits_after_checked_output() {
        let (mut r, mut m) = fixture();
        let extra = 0xfedcba9876543210;
        assert_eq!(
            r.message_call(&mut m, M::SetExtra, &[extra]).unwrap(),
            Some(0)
        );
        assert_eq!(r.message_call(&mut m, M::Extra, &[]).unwrap(), Some(extra));
        r.threads.active = 2;
        assert_eq!(r.message_call(&mut m, M::Extra, &[]).unwrap(), Some(0));
        r.threads.active = 1;
        r.user.cursor.state.set_position([-10, 240]);
        r.message_call(&mut m, M::PostW, &[HWND, 0x8001, 0, 0])
            .unwrap();
        let time = r.user.events[0].time;
        assert!(r
            .message_call(&mut m, M::PeekW, &[STACK + 8180, HWND, 0, 0, 1])
            .is_err());
        assert_eq!(r.user.events.len(), 1);
        assert_eq!(r.message_call(&mut m, M::Extra, &[]).unwrap(), Some(extra));
        assert_eq!(r.message_call(&mut m, M::Time, &[]).unwrap(), Some(0));
        r.message_call(&mut m, M::PeekW, &[STACK + 128, HWND, 0, 0, 1])
            .unwrap();
        assert_eq!(r.message_call(&mut m, M::Extra, &[]).unwrap(), Some(0));
        assert_eq!(
            r.message_call(&mut m, M::Time, &[]).unwrap(),
            Some(time as u64)
        );
        assert_eq!(
            r.message_call(&mut m, M::Position, &[]).unwrap(),
            Some(0x00f0fff6)
        );
        assert_eq!(m.read_int(STACK + 128 + 32, 4).unwrap(), time as u64);
    }
    #[test]
    fn device_notifications_own_high_handles_validate_filters_and_cleanup_window() {
        let (mut r, mut m) = fixture();
        let filter = STACK + 128;
        m.write_int(filter, 4, 32).unwrap();
        m.write_int(filter + 4, 4, 5).unwrap();
        m.write(filter + 12, &crate::x64::runtime::devices::HID_GUID)
            .unwrap();
        let token = r
            .device_call(&mut m, "RegisterDeviceNotificationW", &[HWND, filter, 0])
            .unwrap();
        assert!(token > u32::MAX as u64 && token != HWND);
        assert_eq!(
            r.device_call(&mut m, "RegisterDeviceNotificationA", &[0, filter, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            r.device_call(&mut m, "RegisterDeviceNotificationW", &[HWND, filter, 2])
                .unwrap(),
            0
        );
        assert_eq!(
            r.device_call(&mut m, "RegisterDeviceNotificationW", &[HWND, filter, 1])
                .unwrap(),
            0
        );
        let fault = STACK + 8180;
        m.write_int(fault, 4, 32).unwrap();
        m.write_int(fault + 4, 4, 5).unwrap();
        assert!(r
            .device_call(&mut m, "RegisterDeviceNotificationW", &[HWND, fault, 0])
            .is_err());
        let other = r
            .device_call(&mut m, "RegisterDeviceNotificationA", &[HWND, 0, 4])
            .unwrap();
        assert_eq!(other, token + 16); // Failed registrations did not acquire ownership.
        assert_eq!(
            r.device_call(&mut m, "UnregisterDeviceNotification", &[token])
                .unwrap(),
            1
        );
        assert_eq!(
            r.device_call(&mut m, "UnregisterDeviceNotification", &[token])
                .unwrap(),
            0
        );
        r.remove_device_notifications(HWND);
        assert_eq!(
            r.device_call(&mut m, "UnregisterDeviceNotification", &[other])
                .unwrap(),
            0
        );
        assert!(r.user.events.is_empty());
        assert!(crate::x64::runtime::import_supported(
            "user32.dll",
            "RegisterDeviceNotificationW"
        ));
        assert!(!crate::x64::runtime::import_supported(
            "kernel32.dll",
            "RegisterDeviceNotificationW"
        ));
    }
    #[test]
    fn msg_layout_filters_and_faults_preserve_full_width_posted_data() {
        let (mut r, mut m) = fixture();
        let out = STACK + 128;
        let wparam = 0xfedc112233445566;
        let lparam = 0xff00010000000001;
        assert_eq!(
            r.message_call(&mut m, M::PostW, &[HWND, 0x8001, wparam, lparam])
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            r.message_call(&mut m, M::PeekW, &[out, HWND, 0x8001, 0x8001, 0])
                .unwrap(),
            Some(1)
        );
        assert_eq!(read(&m, out).unwrap()[..4], [HWND, 0x8001, wparam, lparam]);
        assert_eq!(r.user.events.len(), 1);
        m.write(out, &[0x55; 48]).unwrap();
        assert_eq!(
            r.message_call(&mut m, M::PeekW, &[out, 0, 0x8002, 0x8002, 1])
                .unwrap(),
            Some(0)
        );
        assert_eq!(m.read_int(out, 8).unwrap(), 0x5555555555555555);
        let bad = STACK + 4092;
        m.write(bad, &[0x55; 48]).unwrap();
        m.protect(STACK + 4096, 4096, READ).unwrap();
        assert!(r
            .message_call(&mut m, M::PeekW, &[bad, 0, 0, 0, 1])
            .is_err());
        assert_eq!(r.user.events.len(), 1);
        assert_eq!(m.read_int(bad, 4).unwrap(), 0x55555555);
        assert_eq!(
            r.message_call(&mut m, M::PeekW, &[out, 0, 0, 0, 1])
                .unwrap(),
            Some(1)
        );
        assert!(r.user.events.is_empty());
        r.message_call(&mut m, M::Quit, &[0x1234]).unwrap();
        assert_eq!(
            r.message_call(&mut m, M::GetW, &[out, u64::MAX, 0x8001, 0x8001])
                .unwrap(),
            Some(0)
        );
        assert_eq!(read(&m, out).unwrap()[..4], [0, 0x12, 0x1234, 0]);
    }
    #[test]
    fn message_wait_retries_same_gateway_and_dispatch_resumes_real_wndproc() {
        let (mut r, mut m) = fixture();
        let out = STACK + 128;
        let stack = STACK + 0x1f88;
        m.write_int(stack, 8, 0x140001234).unwrap();
        let gateway = r.register("user32.dll", "GetMessageW");
        let mut cpu = Cpu64::new(gateway, stack);
        cpu.regs[1] = out;
        cpu.regs[2] = HWND;
        cpu.regs[8] = 0x8001;
        cpu.regs[9] = 0x8001;
        let before = cpu.clone();
        assert_eq!(
            r.dispatch("user32.dll", "GetMessageW", &mut cpu, &mut m)
                .unwrap(),
            Control::Blocked
        );
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, gateway);
        assert!(!r.thread_runnable(1, &m).unwrap());
        r.message_call(&mut m, M::PostW, &[HWND, 0x8002, 1, 2])
            .unwrap();
        assert!(!r.thread_runnable(1, &m).unwrap());
        r.message_call(
            &mut m,
            M::PostW,
            &[HWND, 0x8001, 0x1234567812345678, 0xfedcba9876543210],
        )
        .unwrap();
        assert!(r.thread_runnable(1, &m).unwrap());
        assert_eq!(
            r.dispatch("user32.dll", "GetMessageW", &mut cpu, &mut m)
                .unwrap(),
            Control::Returned
        );
        assert_eq!(cpu.rip, 0x140001234);
        assert_eq!(cpu.regs[4], stack + 8);
        cpu.regs[4] = stack;
        cpu.regs[1] = out;
        match r
            .dispatch("user32.dll", "DispatchMessageW", &mut cpu, &mut m)
            .unwrap()
        {
            Control::Callbacks {
                calls,
                window_call: Some(mut call),
                ..
            } => {
                assert_eq!(
                    calls[0].1,
                    [HWND, 0x8001, 0x1234567812345678, 0xfedcba9876543210]
                );
                assert!(matches!(
                    r.window_resume(&mut m, &mut call, 0x1122334455667788)
                        .unwrap(),
                    WindowStep::Returned(0x1122334455667788)
                ));
                assert!(r.trace.back().unwrap().contains("DispatchMessageW"));
            }
            other => panic!("expected WNDPROC callback: {other:?}"),
        }
    }
    #[test]
    fn native_key_state_waits_for_removal_and_host_unicode_waits_for_translation() {
        use crate::win32::catalog::KeyboardApi::{AsyncState, State};
        let (mut r, mut m) = fixture();
        let out = STACK + 128;
        r.queue_window_event(WindowEvent {
            window: HWND,
            message: 0x100,
            wparam: 0x41,
            lparam: 1,
        })
        .unwrap();
        r.queue_window_event(WindowEvent {
            window: HWND,
            message: 0x102,
            wparam: 0x65e5,
            lparam: 1,
        })
        .unwrap();
        assert_eq!(r.user.events.len(), 1);
        assert_eq!(
            r.keyboard_call(&mut m, AsyncState, &[0x41]).unwrap(),
            0xffff8001
        );
        assert_eq!(
            r.keyboard_call(&mut m, AsyncState, &[0x41]).unwrap(),
            0xffff8000
        );
        assert_eq!(r.keyboard_call(&mut m, State, &[0x41]).unwrap(), 0);
        r.message_call(&mut m, M::PeekW, &[out, 0, 0, 0, 0])
            .unwrap();
        assert_eq!(r.keyboard_call(&mut m, State, &[0x41]).unwrap(), 0);
        r.message_call(&mut m, M::PeekW, &[out, 0, 0, 0, 1])
            .unwrap();
        assert_eq!(r.keyboard_call(&mut m, State, &[0x41]).unwrap(), 0xffff8000);
        assert_eq!(
            r.message_call(&mut m, M::Translate, &[out]).unwrap(),
            Some(1)
        );
        r.message_call(&mut m, M::PeekW, &[out, 0, 0, 0, 1])
            .unwrap();
        assert_eq!(read(&m, out).unwrap()[..3], [HWND, 0x102, 0x65e5]);
        r.queue_window_event(WindowEvent {
            window: HWND,
            message: 0x101,
            wparam: 0x41,
            lparam: 0xc0000001,
        })
        .unwrap();
        assert_eq!(r.keyboard_call(&mut m, AsyncState, &[0x41]).unwrap(), 0);
        r.message_call(&mut m, M::PeekW, &[out, 0, 0, 0, 1])
            .unwrap();
        assert_eq!(r.keyboard_call(&mut m, State, &[0x41]).unwrap(), 0);
    }
}
