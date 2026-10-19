//! Win64 allocation markers and FLS callback identities; values remain in each thread's TEB.
pub(crate) struct LocalSlots64 {
    tls: [Option<u64>; 64],
    fls: [Option<(u64, u64)>; 128],
}

impl Default for LocalSlots64 {
    fn default() -> Self {
        Self {
            tls: [None; 64],
            fls: [None; 128],
        }
    }
}

impl LocalSlots64 {
    pub(crate) fn vacant_tls(&self) -> Option<usize> {
        self.tls.iter().position(Option::is_none)
    }

    pub(crate) fn vacant_fls(&self) -> Option<usize> {
        self.fls.iter().position(Option::is_none)
    }

    #[inline]
    pub(crate) fn tls(&self, index: usize) -> Option<u64> {
        self.tls.get(index).copied().flatten()
    }

    pub(crate) fn fls(&self, index: usize) -> Option<(u64, u64)> {
        self.fls.get(index).copied().flatten()
    }

    #[inline]
    pub(crate) fn commit_tls(&mut self, index: usize, value: Option<u64>) {
        self.tls[index] = value;
    }

    pub(crate) fn commit_fls(&mut self, index: usize, value: Option<(u64, u64)>) {
        self.fls[index] = value;
    }

    pub(crate) fn active_fls(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.fls
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.map(|(_, callback)| (index, callback)))
    }
}
