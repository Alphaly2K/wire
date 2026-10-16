//! Guest timer-resolution requests; these do not alter host timer resolution.
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct PeriodRequests {
    requests: BTreeMap<u32, u32>,
}

impl PeriodRequests {
    pub(crate) fn begin(&mut self, period: u32) -> Result<u64, String> {
        if period == 0 || (self.requests.len() >= 256 && !self.requests.contains_key(&period)) {
            return Ok(97);
        }
        let refs = self.requests.entry(period).or_default();
        *refs = refs
            .checked_add(1)
            .ok_or("multimedia timer reference capacity")?;
        Ok(0)
    }

    pub(crate) fn end(&mut self, period: u32) -> u64 {
        if let Some(refs) = self.requests.get_mut(&period) {
            *refs -= 1;
            if *refs == 0 {
                self.requests.remove(&period);
            }
            0
        } else {
            97
        }
    }
}
