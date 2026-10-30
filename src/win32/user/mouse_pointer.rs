//! Mouse promotion and owned pointer-message snapshots; no guest memory.
pub(crate) const MOUSE: u64 = 0x57010001;
#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    pub window: u64,
    pub frame: u32,
    pub flags: u32,
    pub point: [i32; 2],
    pub time: u32,
    pub input: [u32; 3],
    pub change: u32,
}

impl Snapshot {
    #[allow(dead_code)]
    pub fn info32(self) -> [u8; 88] {
        let wide = self.info64();
        let mut bytes = [0; 88];
        bytes[..16].copy_from_slice(&wide[..16]);
        bytes[16..20].copy_from_slice(&(MOUSE as u32).to_le_bytes());
        bytes[20..24].copy_from_slice(&(self.window as u32).to_le_bytes());
        bytes[24..].copy_from_slice(&wide[32..]);
        bytes
    }
    pub fn info64(self) -> [u8; 96] {
        let mut bytes = [0; 96];
        for (offset, value) in [
            (0, 4u32), // PT_MOUSE
            (4, 1),
            (8, self.frame),
            (12, self.flags),
            (64, self.time),
            (68, 1),
            (76, self.input[1] & 0xc),
            (88, self.change),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[16..24].copy_from_slice(&MOUSE.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.window.to_le_bytes());
        for offset in [32, 48] {
            for (n, value) in self.point.into_iter().enumerate() {
                bytes[offset + n * 4..offset + n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        for offset in [40, 56] {
            for (n, value) in self.point.into_iter().enumerate() {
                let value = (i64::from(value) * 2540 / 96) as i32;
                bytes[offset + n * 4..offset + n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }
}

#[derive(Default)]
pub(crate) struct MousePointer {
    mode: Option<bool>,
    buttons: u32,
    frame: u32,
    seen: bool,
    pub current: std::collections::BTreeMap<u64, Snapshot>,
}

impl MousePointer {
    pub fn enabled(&self) -> bool {
        self.mode == Some(true)
    }

    pub fn enable(&mut self, enabled: bool) -> bool {
        if self.mode.is_some() {
            return false;
        }
        self.mode = Some(enabled);
        true
    }

    pub fn remove_window(&mut self, window: u64) {
        self.current.retain(|_, p| p.window != window);
    }

    pub fn promote(
        &mut self,
        window: u64,
        input: [u32; 3],
        point: [i32; 2],
        time: u32,
    ) -> Option<(u32, u64, Snapshot)> {
        if !self.enabled() {
            return None;
        }
        let [message, wp, _] = input;
        let change = match message {
            0x200 | 0x20a | 0x20e => 0,
            0x201 | 0x203 => 1,
            0x202 => 2,
            0x204 | 0x206 => 3,
            0x205 => 4,
            0x207 | 0x209 => 5,
            0x208 => 6,
            0x20b | 0x20d if wp >> 16 == 1 => 7,
            0x20c if wp >> 16 == 1 => 8,
            0x20b | 0x20d if wp >> 16 == 2 => 9,
            0x20c if wp >> 16 == 2 => 10,
            _ => return None,
        };
        let buttons = wp & 0x73;
        let mut flags = 0x2002 | u32::from(!self.seen);
        for (mk, pointer) in [(1, 0x10), (2, 0x20), (16, 0x40), (32, 0x80), (64, 0x100)] {
            if buttons & mk != 0 {
                flags |= pointer;
            }
        }
        if buttons != 0 {
            flags |= 4;
        }
        let pointer_message = match message {
            0x20a => {
                flags |= 0x80000;
                0x24e
            }
            0x20e => {
                flags |= 0x100000;
                0x24f
            }
            _ if self.buttons == 0 && buttons != 0 => {
                flags |= 0x10000;
                0x246
            }
            _ if self.buttons != 0 && buttons == 0 => {
                flags |= 0x40000;
                0x247
            }
            _ => {
                flags |= 0x20000;
                0x245
            }
        };
        self.buttons = buttons;
        self.seen = true;
        self.frame = self.frame.wrapping_add(1);
        let high = if matches!(message, 0x20a | 0x20e) {
            wp & 0xffff0000
        } else {
            (flags & 0xffff) << 16
        };
        Some((
            pointer_message,
            u64::from(high | 1),
            Snapshot {
                window,
                frame: self.frame,
                flags,
                point,
                time,
                input,
                change,
            },
        ))
    }
}
