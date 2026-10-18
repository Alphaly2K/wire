//! Cursor values and per-thread counts; no guest memory, image table or process.
use std::collections::BTreeMap;

// Preserve each guest's handle and thread-key widths without exposing the map.
pub(crate) struct Cursor<Handle, Thread> {
    current: Handle,
    position: [i32; 2],
    counts: BTreeMap<Thread, i32>,
}

impl<Handle: Default, Thread> Default for Cursor<Handle, Thread> {
    fn default() -> Self {
        Self {
            current: Handle::default(),
            position: [0; 2],
            counts: BTreeMap::new(),
        }
    }
}

impl<Handle: Copy, Thread: Ord> Cursor<Handle, Thread> {
    pub(crate) fn current(&self) -> Handle {
        self.current
    }

    // The architecture adapter validates the image before replacing it.
    pub(crate) fn replace(&mut self, handle: Handle) -> Handle {
        std::mem::replace(&mut self.current, handle)
    }

    pub(crate) fn position(&self) -> [i32; 2] {
        self.position
    }

    pub(crate) fn set_position(&mut self, position: [i32; 2]) {
        self.position = position;
    }

    pub(crate) fn show(&mut self, thread: Thread, show: bool) -> i32 {
        let count = self.counts.entry(thread).or_default();
        *count = count.saturating_add(if show { 1 } else { -1 });
        *count
    }

    pub(crate) fn exit(&mut self, thread: Thread) {
        self.counts.remove(&thread);
    }
}

pub(crate) fn clipped(rect: [i32; 4], screen: [i32; 4]) -> Option<[i32; 4]> {
    let clip = [
        rect[0].max(screen[0]),
        rect[1].max(screen[1]),
        rect[2].min(screen[2]),
        rect[3].min(screen[3]),
    ];
    (clip[0] < clip[2] && clip[1] < clip[3]).then_some(clip)
}

pub(crate) fn clamp(point: [i32; 2], clip: [i32; 4]) -> [i32; 2] {
    std::array::from_fn(|n| point[n].clamp(clip[n], clip[n + 2] - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_counts_saturate_and_retire_only_the_full_width_thread() {
        let mut cursor = Cursor::<u64, u64>::default();
        let other = (1u64 << 32) | 1;
        cursor.counts.insert(1, i32::MAX);
        cursor.counts.insert(other, i32::MIN);
        assert_eq!(cursor.show(1, true), i32::MAX);
        assert_eq!(cursor.show(other, false), i32::MIN);
        assert_eq!(cursor.show(1, false), i32::MAX - 1);
        cursor.replace(1u64 << 40);
        cursor.set_position([-3, 5]);
        cursor.exit(1);
        assert_eq!(cursor.show(1, false), -1);
        assert_eq!(cursor.show(other, true), i32::MIN + 1);
        assert_eq!(cursor.current(), 1u64 << 40);
        assert_eq!(cursor.position(), [-3, 5]);
    }
}
