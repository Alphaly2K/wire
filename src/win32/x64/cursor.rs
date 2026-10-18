//! Win64 virtual pointer owner and checked memory, independent of Runtime64.
use super::geometry::{read_rect, write_rect};
use crate::{
    win32::{
        catalog::CursorApi,
        user::cursor::{self, Cursor},
    },
    x64::memory::Memory,
};

pub(crate) struct Cursor64 {
    pub(crate) state: Cursor<u64, u64>,
    clip: [i32; 4],
    screen: [i32; 4],
}

pub(crate) struct CursorResult {
    pub value: u64,
    pub error: Option<u32>,
}

impl CursorResult {
    fn value(value: u64) -> Self {
        Self { value, error: None }
    }
    fn error(error: u32) -> Self {
        Self {
            value: 0,
            error: Some(error),
        }
    }
}

impl Cursor64 {
    pub(crate) fn new(screen: [i32; 4]) -> Self {
        Self {
            state: Default::default(),
            clip: screen,
            screen,
        }
    }

    pub(crate) fn position(&self) -> [i32; 2] {
        self.state.position()
    }

    pub(crate) fn host_position(&mut self, point: [i32; 2]) -> [i32; 2] {
        let point = cursor::clamp(point, self.clip);
        self.state.set_position(point);
        point
    }

    pub(crate) fn call(
        &mut self,
        mem: &mut Memory,
        id: CursorApi,
        a: &[u64],
        tid: u64,
        valid_image: bool,
    ) -> Result<CursorResult, String> {
        use CursorApi::*;
        let value = match id {
            Show => self.state.show(tid, a[0] != 0) as u32 as u64,
            Get => self.state.current(),
            Set => {
                if !valid_image {
                    return Ok(CursorResult::error(6));
                }
                self.state.replace(a[0])
            }
            GetPosition => {
                mem.write(
                    a[0],
                    &self
                        .position()
                        .into_iter()
                        .flat_map(i32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
                1
            }
            GetClip => {
                write_rect(mem, a[0], self.clip)?;
                1
            }
            Clip => {
                let rect = if a[0] == 0 {
                    self.screen
                } else {
                    read_rect(mem, a[0])?
                };
                let Some(clip) = cursor::clipped(rect, self.screen) else {
                    return Ok(CursorResult::error(87));
                };
                self.clip = clip;
                self.state
                    .set_position(cursor::clamp(self.position(), clip));
                1
            }
            PointerEnabled => 0,
            EnablePointer => {
                if a[0] != 0 {
                    return Ok(CursorResult::error(50));
                }
                1
            }
            Track | SetPosition => {
                unreachable!("tracking has a separate memory/window adapter; setter is absent")
            }
        };
        Ok(CursorResult::value(value))
    }
}

pub(crate) fn track(
    mem: &mut Memory,
    pointer: u64,
    has_window: impl FnOnce(u64) -> bool,
) -> Result<CursorResult, String> {
    // Win64 TRACKMOUSEEVENT is 24 bytes including tail padding. The host
    // bridge has no hover/leave source; only query/cancel the empty state.
    let mut data = [0; 24];
    mem.read(pointer, &mut data)?;
    let size = u32::from_le_bytes(data[..4].try_into().unwrap());
    let flags = u32::from_le_bytes(data[4..8].try_into().unwrap());
    if size != 24 {
        return Ok(CursorResult::error(87));
    }
    if flags & 0x40000000 != 0 {
        let mut output = [0; 24];
        output[..4].copy_from_slice(&24u32.to_le_bytes());
        mem.write(pointer, &output)?;
        return Ok(CursorResult::value(1));
    }
    let window = u64::from_le_bytes(data[8..16].try_into().unwrap());
    if !has_window(window) {
        return Ok(CursorResult::error(1400));
    }
    if flags & !(0x80000000 | 0x10 | 3) != 0 {
        return Ok(CursorResult::error(87));
    }
    if flags & 0x80000000 != 0 || flags & 3 == 0 {
        return Ok(CursorResult::value(1));
    }
    Ok(CursorResult::error(50))
}
