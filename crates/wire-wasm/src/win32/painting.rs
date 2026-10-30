//! Rectangular dirty regions; callbacks and PAINTSTRUCT output remain in Process.
use super::{
    catalog::PaintingApi as P,
    user::{self, geometry, User},
    ServiceError,
};
use crate::{machine::State, memory::Memory};
#[derive(Default)]
pub(crate) struct Paint {
    pub area: Option<[i32; 4]>,
    pub erase: bool,
    pub version: u64,
}

impl User {
    pub fn invalidate(
        &mut self,
        window: u64,
        area: Option<[i32; 4]>,
        erase: bool,
    ) -> Result<(), u32> {
        let w = self.windows.get(&window).ok_or(1400u32)?;
        let full = [0, 0, w.client[2] - w.client[0], w.client[3] - w.client[1]];
        let (area, valid) = geometry::intersection(full, area.unwrap_or(full));
        if !valid {
            return Ok(());
        }
        let p = self.paints.entry(window).or_default();
        p.area = Some(p.area.map_or(area, |old| geometry::union(old, area).0));
        p.erase |= erase;
        p.version = p.version.wrapping_add(1);
        self.messages.clear_paint(window);
        Ok(())
    }
    pub fn validate(&mut self, window: u64, area: Option<[i32; 4]>) {
        if let Some(p) = self.paints.get_mut(&window) {
            if area.is_none()
                || p.area.zip(area).is_some_and(|(old, r)| {
                    r[0] <= old[0] && r[1] <= old[1] && r[2] >= old[2] && r[3] >= old[3]
                })
            {
                p.area = None;
                p.erase = false;
                p.version = p.version.wrapping_add(1);
                self.messages.clear_paint(window);
            }
        }
    }
    pub fn paint_pending(&self, tid: u32) -> bool {
        self.paints.iter().any(|(h, p)| {
            p.area.is_some()
                && self
                    .windows
                    .get(h)
                    .is_some_and(|w| w.tid == u64::from(tid) && w.style & 0x10000000 != 0)
        })
    }
    pub fn message_clock(&self, tid: u32, mask: u32) -> bool {
        mask & 0x10 != 0 && self.messages.needs_time(tid)
            || mask & 0x20 != 0 && self.paint_pending(tid)
    }
    pub fn refresh_messages(&mut self, tid: u32, sample: u64, mask: u32) {
        if mask & 0x10 != 0 {
            self.messages.refresh(tid, sample, self.cursor.position());
        }
        if mask & 0x20 == 0 {
            return;
        }
        let windows = self
            .paints
            .iter()
            .filter_map(|(&h, p)| {
                (p.area.is_some()
                    && self
                        .windows
                        .get(&h)
                        .is_some_and(|w| w.tid == u64::from(tid) && w.style & 0x10000000 != 0))
                .then_some(h)
            })
            .collect::<Vec<_>>();
        self.messages
            .refresh_paint(tid, &windows, self.cursor.position(), sample);
    }
    pub fn painting_value(
        &mut self,
        id: P,
        a: &[u64; 16],
        state: &State,
        memory: &mut Memory,
        tid: u32,
    ) -> Result<u64, ServiceError> {
        let Some(w) = self.windows.get(&a[0]) else {
            return user::error(memory, state, 1400);
        };
        if w.tid != u64::from(tid) {
            return user::error(memory, state, 50);
        }
        match id {
            P::Invalidate | P::Validate => {
                let area = if a[1] == 0 {
                    None
                } else {
                    Some(user::rect(memory, a[1])?)
                };
                if id == P::Invalidate {
                    self.invalidate(a[0], area, a[2] != 0).map_err(|_| {
                        ServiceError::Unavailable("window disappeared during invalidation")
                    })?;
                } else {
                    self.validate(a[0], area);
                }
                Ok(1)
            }
            P::End => {
                let mut bytes = [0; 72];
                let size = if memory.mode() == crate::machine::Mode::X86 {
                    64
                } else {
                    72
                };
                memory.read(a[1], &mut bytes[..size])?;
                let ptr = memory.mode() as usize / 8;
                let mut handle = [0; 8];
                handle[..ptr].copy_from_slice(&bytes[..ptr]);
                match self
                    .gdi
                    .release(a[0], u64::from_le_bytes(handle), tid, true)
                {
                    Ok(v) => Ok(v),
                    Err(e) => user::error(memory, state, e),
                }
            }
            _ => unreachable!(),
        }
    }
}
