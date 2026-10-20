//! Window and thread retirement borrow only USER state, graphics, menus and scheduler.
use super::User32;
use crate::{
    graphics::Graphics,
    thread::{Sched, ThreadState, WaitTarget, Wake},
    win32::user::menus32::Menus,
};

fn wake_sender(sched: &mut Sched, id: u32, sender: u32) {
    if let Some(i)=sched.threads.iter().position(|t|t.tid==sender && matches!(t.state,ThreadState::Waiting {target:WaitTarget::SentMessage(n),..} if n==id)) {
        sched.wake_thread(i,Wake::Retest);
    }
}
impl User32 {
    pub(crate) fn remove_window(
        &mut self,
        graphics: &mut Graphics,
        menus: &mut Menus,
        sched: &mut Sched,
        hwnd: u32,
    ) {
        self.keyboard.remove_window(hwnd);
        let pending: Vec<_> = self
            .sent
            .iter()
            .filter(|(_, m)| m.args[0] == hwnd && !m.processing && m.result.is_none())
            .map(|(&id, m)| (id, m.sender))
            .collect();
        for (id, sender) in pending {
            self.sent.get_mut(&id).unwrap().result = Some(0);
            wake_sender(sched, id, sender);
        }
        graphics.remove_window(hwnd);
        for owner in [
            &mut self.focus,
            &mut self.active,
            &mut self.foreground,
            &mut self.capture,
        ] {
            if *owner == hwnd {
                *owner = 0;
            }
        }
        if let Some(window) = self.windows.remove(&hwnd) {
            if window.style & 0x40000000 == 0 && window.menu != 0 {
                menus.destroy(window.menu);
            }
        }
        self.timers.retain(|&(_, owner, _), _| owner != hwnd);
        for queue in self.queues.values_mut() {
            queue.retain(|msg| msg.hwnd != hwnd);
        }
    }

    pub(crate) fn exit_thread(
        &mut self,
        graphics: &mut Graphics,
        menus: &mut Menus,
        sched: &mut Sched,
        tid: u32,
    ) {
        let windows: Vec<_> = self
            .windows
            .iter()
            .filter_map(|(&h, w)| (w.tid == tid).then_some(h))
            .collect();
        for h in windows {
            self.remove_window(graphics, menus, sched, h);
        }
        let pending: Vec<_> = self
            .sent
            .iter()
            .filter(|(_, m)| m.receiver == tid && m.result.is_none())
            .map(|(&id, m)| (id, m.sender))
            .collect();
        for (id, sender) in pending {
            self.sent.get_mut(&id).unwrap().result = Some(0);
            wake_sender(sched, id, sender);
        }
        self.sent.retain(|_, m| m.sender != tid || m.processing);
        self.queues.remove(&tid);
        self.queue_changed.remove(&tid);
        self.cursor.exit(tid);
        self.keyboard.exit(tid);
        self.timers.retain(|&(owner, _, _), _| owner != tid);
    }
}
