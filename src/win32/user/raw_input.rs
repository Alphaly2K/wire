//! Logical host devices, registrations and bounded raw packets; no guest memory.
use std::collections::BTreeMap;

pub(crate) use super::mouse_pointer::MOUSE;
pub(crate) const KEYBOARD: u64 = 0x57010002;
pub(crate) const CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Registration {
    pub usage: u16,
    pub flags: u32,
    pub target: u64,
}

pub(crate) struct Packet {
    pub window: u64,
    pub tid: u64,
    pub input: [u32; 3],
    pub no_legacy: bool,
    pub queued: bool,
    kind: u32,
    payload: Vec<u8>,
}
impl Packet {
    pub fn size(&self, width: usize) -> usize {
        8 + width * 2 + self.payload.len()
    }
    pub fn bytes(&self, width: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.size(width));
        bytes.extend(self.kind.to_le_bytes());
        bytes.extend((self.size(width) as u32).to_le_bytes());
        let device = if self.kind == 0 { MOUSE } else { KEYBOARD };
        bytes.extend_from_slice(&device.to_le_bytes()[..width]);
        bytes.extend_from_slice(&0u64.to_le_bytes()[..width]); // RIM_INPUT: foreground delivery.
        bytes.extend(&self.payload);
        bytes
    }
}
pub(crate) struct RawInput {
    pub registrations: BTreeMap<u16, Registration>,
    pub packets: BTreeMap<u64, Packet>,
    next: u64,
}
impl Default for RawInput {
    fn default() -> Self {
        Self {
            registrations: BTreeMap::new(),
            packets: BTreeMap::new(),
            next: 0x57020000,
        }
    }
}
impl RawInput {
    pub fn check_capacity(&self, additional: usize, handle_end: u64) -> Result<(), String> {
        if self
            .packets
            .len()
            .checked_add(additional)
            .is_none_or(|n| n > CAPACITY)
        {
            return Err("raw input packet capacity exceeded".into());
        }
        if self
            .next
            .checked_add(additional as u64)
            .is_none_or(|n| n > handle_end)
        {
            return Err("raw input handle exhaustion".into());
        }
        Ok(())
    }
    pub fn remove_window(&mut self, window: u64) {
        self.registrations.retain(|_, r| r.target != window);
        self.packets.retain(|_, p| p.window != window);
    }
    pub fn host(
        &mut self,
        window: u64,
        input: [u32; 3],
        point: [i32; 2],
        screen: [u32; 2],
        owner: impl FnOnce(u64) -> Option<u64>,
    ) -> Result<Option<(u64, bool)>, String> {
        let [message, key, lp] = input;
        let mouse = (0x200..=0x20e).contains(&message);
        let keyboard = matches!(message, 0x100 | 0x101 | 0x104 | 0x105);
        if !mouse && !keyboard {
            return Ok(None);
        }
        let Some(r) = self.registrations.get(&(if mouse { 2 } else { 6 })) else {
            return Ok(None);
        };
        let target = if r.target == 0 { window } else { r.target };
        let Some(tid) = owner(target) else {
            return Ok(None);
        };
        let no_legacy = r.flags & 0x30 == 0x30;
        let mut payload = vec![0; if mouse { 24 } else { 16 }];
        if mouse {
            // The host supplies positions, not physical relative device counts.
            payload[..2].copy_from_slice(&3u16.to_le_bytes());
            for n in 0..2 {
                let size = screen[n].max(1) as u64;
                let pixel = (point[n] as i64).clamp(0, size as i64 - 1) as u64;
                let normalized = (pixel * 65535).div_ceil(size) as u32;
                payload[12 + n * 4..16 + n * 4].copy_from_slice(&normalized.to_le_bytes());
            }
            let button: u16 = match message {
                0x201 | 0x203 => 1,
                0x202 => 2,
                0x204 | 0x206 => 4,
                0x205 => 8,
                0x207 | 0x209 => 16,
                0x208 => 32,
                0x20b | 0x20d if key >> 16 == 1 => 64,
                0x20c if key >> 16 == 1 => 128,
                0x20b | 0x20d if key >> 16 == 2 => 256,
                0x20c if key >> 16 == 2 => 512,
                0x20a => 0x400,
                0x20e => 0x800,
                _ => 0,
            };
            payload[4..6].copy_from_slice(&button.to_le_bytes());
            if matches!(message, 0x20a | 0x20e) {
                payload[6..8].copy_from_slice(&((key >> 16) as u16).to_le_bytes());
            }
        } else {
            let flags: u16 = (matches!(message, 0x101 | 0x105) as u16)
                | (if lp & (1 << 24) != 0 { 2 } else { 0 });
            payload[..2].copy_from_slice(&((lp >> 16 & 0xff) as u16).to_le_bytes());
            payload[2..4].copy_from_slice(&flags.to_le_bytes());
            payload[6..8].copy_from_slice(&(key as u16).to_le_bytes());
            payload[8..12].copy_from_slice(&message.to_le_bytes());
        }
        self.check_capacity(1, u64::MAX)?;
        let handle = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or("raw input handle exhaustion")?;
        self.packets.insert(
            handle,
            Packet {
                window: target,
                tid,
                input,
                no_legacy,
                queued: true,
                kind: if mouse { 0 } else { 1 },
                payload,
            },
        );
        Ok(Some((handle, no_legacy)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pe32_packet_identity_limit_preflights_the_whole_batch() {
        let mut input = RawInput {
            next: u32::MAX.into(),
            ..Default::default()
        };
        input.registrations.insert(
            2,
            Registration {
                usage: 2,
                flags: 0,
                target: 0,
            },
        );
        assert!(input.check_capacity(2, 1 << 32).is_err());
        assert!(input.packets.is_empty());
        input.check_capacity(1, 1 << 32).unwrap();
        assert_eq!(
            input
                .host(1, [0x200, 0, 0], [0, 0], [1024, 768], |_| Some(1))
                .unwrap(),
            Some((u32::MAX.into(), false))
        );
        assert!(input.check_capacity(1, 1 << 32).is_err());
        assert_eq!(input.packets.len(), 1);
    }
}
