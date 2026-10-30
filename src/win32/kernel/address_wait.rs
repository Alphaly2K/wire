//! Host-independent address-wait protocol. Adapters own clocks and contexts.
#[derive(Clone)]
pub(crate) struct AddressWait<D> {
    pub address: u64,
    pub ticket: u64,
    pub notified: bool,
    pub deadline: Option<D>,
}
impl<D: Copy + Ord> AddressWait<D> {
    pub fn expired(&self, now: D) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }
    pub fn ready(&self, now: D) -> bool {
        self.notified || self.expired(now)
    }
    pub fn wakeable(&self, address: u64, now: D) -> bool {
        self.address == address && !self.notified && !self.expired(now)
    }
}
pub(crate) enum Decision<D> {
    Return(u64),
    Pending,
    Enqueue(AddressWait<D>, u64),
}
pub(crate) fn first<'a, D: Copy + Ord + 'a>(
    waits: impl Iterator<Item = (u64, &'a AddressWait<D>)>,
    address: u64,
    now: D,
) -> Option<u64> {
    waits
        .filter(|(_, wait)| wait.wakeable(address, now))
        .map(|(thread, wait)| (wait.ticket, thread))
        .min()
        .map(|(_, thread)| thread)
}
