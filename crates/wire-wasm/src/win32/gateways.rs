//! Logical Windows call and return addresses have no guest instruction bytes.
use crate::{jit::Exit, machine::Mode};
use std::ops::Range;
#[path = "../../../../src/win32/gateway_layout.rs"]
mod x86;

#[derive(Clone, Debug)]
pub struct Layout {
    pub services: Range<u64>,
    pub returned: u64,
    pub callback: u64,
    pub thread_returned: u64,
}

pub const fn layout(mode: Mode) -> Layout {
    let base32 = x86::STUB_BASE as u64;
    let end32 = base32 + x86::STUB_CAPACITY as u64 * x86::STUB_STRIDE as u64;
    match mode {
        Mode::X86 => Layout {
            services: base32..end32,
            returned: base32,
            callback: end32,
            thread_returned: base32 + x86::STUB_STRIDE as u64,
        },
        Mode::X64 => Layout {
            services: 0x7ffe_0000_0000..0x7ffe_0010_0000,
            returned: 0x7ffe_1000_0000,
            callback: 0x7ffe_1000_0010,
            thread_returned: 0x7ffe_1000_0020,
        },
    }
}

pub(crate) fn provider_area(mode: Mode) -> Range<u64> {
    let start = if mode == Mode::X86 {
        0x7100_0000
    } else {
        0x7ffe_2000_0000
    };
    start..start + PROVIDER_CAPACITY as u64 * 65536
}

pub(crate) const PROVIDER_CAPACITY: usize = 128;

pub(crate) fn delay_area(mode: Mode) -> Range<u64> {
    let start = if mode == Mode::X86 {
        0x7050_0000
    } else {
        0x7ffe_2100_0000
    };
    start..start + 4096 * 16
}

impl Layout {
    fn delays(&self) -> Range<u64> {
        delay_area(if self.services.start < 1 << 32 {
            Mode::X86
        } else {
            Mode::X64
        })
    }
    pub const fn words(&self) -> [u64; 5] {
        [
            self.services.start,
            self.services.end - self.services.start,
            self.returned,
            self.callback,
            self.thread_returned,
        ]
    }

    pub fn exit(&self, pc: u64) -> Option<Exit> {
        if pc == self.returned {
            Some(Exit::Returned)
        } else if pc == self.callback {
            Some(Exit::Callback)
        } else if pc == self.thread_returned {
            Some(Exit::ThreadReturned)
        } else if self.services.contains(&pc) || self.delays().contains(&pc) {
            Some(Exit::Service)
        } else {
            None
        }
    }

    /// First reserved byte touched by an instruction's fetch interval.
    pub fn first(&self, start: u64, length: usize) -> Option<u64> {
        let end = start.saturating_add(length as u64);
        [
            self.services.clone(),
            self.delays(),
            self.returned..self.returned + 1,
            self.callback..self.callback + 1,
            self.thread_returned..self.thread_returned + 1,
        ]
        .into_iter()
        .filter_map(|range| {
            let first = start.max(range.start);
            (first < end && first < range.end).then_some(first)
        })
        .min()
    }
}
