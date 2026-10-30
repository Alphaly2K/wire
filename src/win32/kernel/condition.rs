//! Condition wait identity and notification/deadline rules, independent of memory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Critical,
    Srw(bool),
}
#[derive(Clone)]
pub(crate) struct Wait<D> {
    pub condition: u64,
    pub lock: u64,
    pub mode: Mode,
    pub timeout: u32,
    pub ticket: u64,
    pub notified: bool,
    pub deadline: Option<D>,
}
impl<D: Copy + Ord> Wait<D> {
    pub fn expired(&self, now: D) -> bool {
        self.deadline.is_some_and(|d| now >= d)
    }
    pub fn wakeable(&self, condition: u64, now: D) -> bool {
        self.condition == condition && !self.notified && !self.expired(now)
    }
}
