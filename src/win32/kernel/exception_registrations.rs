//! Process-owned handler ordering and identities, independent of guest memory.
pub(crate) struct Registrations {
    handlers: Vec<(u64, u64)>,
    next: u64,
    filter: u64,
}
impl Default for Registrations {
    fn default() -> Self {
        Self {
            handlers: Vec::new(),
            next: 0x7fff8000,
            filter: 0,
        }
    }
}
impl Registrations {
    pub(crate) fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
    pub(crate) fn targets(&self) -> Vec<u64> {
        self.handlers.iter().map(|&(_, target)| target).collect()
    }
    pub(crate) fn add(&mut self, first: bool, target: u64) -> Result<u64, u32> {
        if target == 0 {
            return Err(87);
        }
        if self.handlers.len() >= 64 || self.next >= 0x7fff8100 {
            return Err(8);
        }
        let token = self.next;
        self.next += 1;
        if first {
            self.handlers.insert(0, (token, target));
        } else {
            self.handlers.push((token, target));
        }
        Ok(token)
    }
    pub(crate) fn remove(&mut self, token: u64) -> u64 {
        if let Some(index) = self.handlers.iter().position(|&(id, _)| id == token) {
            self.handlers.remove(index);
            1
        } else {
            0
        }
    }
    pub(crate) fn set_filter(&mut self, target: u64) -> u64 {
        std::mem::replace(&mut self.filter, target)
    }
    #[allow(dead_code)]
    pub(crate) fn filter(&self) -> u64 {
        self.filter
    }
}
