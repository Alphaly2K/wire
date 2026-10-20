//! USER owns sent-message selection, completion and sender wakeups.
use super::User32;
use crate::thread::{Sched, ThreadState, WaitTarget, Wake};

pub(crate) struct SentCall {
    pub id: u32,
    pub procedure: Option<u32>,
    pub args: [u32; 4],
}

impl User32 {
    pub(crate) fn next_sent(&mut self, tid: u32) -> Option<SentCall> {
        let (&id, msg) = self
            .sent
            .iter()
            .find(|(_, m)| m.receiver == tid && !m.processing && m.result.is_none())?;
        let args = msg.args;
        self.sent.get_mut(&id).unwrap().processing = true;
        Some(SentCall {
            id,
            args,
            procedure: self.windows.get(&args[0]).map(|w| w.wndproc),
        })
    }
    pub(crate) fn complete_sent(
        &mut self,
        id: u32,
        value: u32,
        sched: &mut Sched,
    ) -> Result<(), String> {
        if sched
            .threads
            .iter()
            .any(|t| t.tid == self.sent[&id].sender && matches!(t.state, ThreadState::Exited(_)))
        {
            self.sent.remove(&id);
            return Ok(());
        }
        let msg = self
            .sent
            .get_mut(&id)
            .ok_or("sent message removed during callback")?;
        msg.result = Some(value);
        let sender = msg.sender;
        if let Some(i) = sched.threads.iter().position(|t| {
            t.tid == sender
                && matches!(t.state,
            ThreadState::Waiting {target:WaitTarget::SentMessage(n),..} if n==id)
        }) {
            sched.wake_thread(i, Wake::Retest);
        }
        Ok(())
    }
}
