//! Host key state and resolved Unicode text stay data until TranslateMessage.
use std::collections::VecDeque;

fn transition(message: u32, key: u32, lparam: u32) -> Option<(usize, bool)> {
    let (key, down) = match message {
        0x100 | 0x101 | 0x104 | 0x105 => {
            let key = match key {
                0x10 => {
                    if (lparam >> 16) & 0xff == 0x36 {
                        0xa1
                    } else {
                        0xa0
                    }
                }
                0x11 => {
                    if lparam & (1 << 24) != 0 {
                        0xa3
                    } else {
                        0xa2
                    }
                }
                0x12 => {
                    if lparam & (1 << 24) != 0 {
                        0xa5
                    } else {
                        0xa4
                    }
                }
                key => key,
            };
            (key, matches!(message, 0x100 | 0x104))
        }
        0x201..=0x203 => (1, message != 0x202),
        0x204..=0x206 => (2, message != 0x205),
        0x207..=0x209 => (4, message != 0x208),
        0x20b..=0x20d => {
            let key = match key >> 16 {
                1 => 5,
                2 => 6,
                _ => return None,
            };
            (key, message != 0x20c)
        }
        _ => return None,
    };
    (key < 256).then_some((key as usize, down))
}

fn aggregate(state: &mut [u8; 256], index: usize, pressed: bool) {
    let generic = match index {
        0xa0 | 0xa1 => 0x10,
        0xa2 | 0xa3 => 0x11,
        0xa4 | 0xa5 => 0x12,
        _ => return,
    };
    let down = (state[index & !1] | state[index | 1]) & 0x80;
    if pressed {
        state[generic] |= 1;
    }
    state[generic] = (state[generic] & 1) | down;
}
pub(crate) struct Keyboard {
    asynchronous: [u8; 256],
    queued: std::collections::BTreeMap<u64, [u8; 256]>,
    text: VecDeque<([u64; 5], Vec<u16>)>,
}
impl Default for Keyboard {
    fn default() -> Self {
        Self {
            asynchronous: [0; 256],
            queued: Default::default(),
            text: Default::default(),
        }
    }
}
impl Keyboard {
    pub(crate) fn host(&mut self, words: [u32; 5]) -> bool {
        self.host64(words.map(u64::from))
    }
    pub(crate) fn host64(&mut self, words: [u64; 5]) -> bool {
        let [hwnd, message, key, lparam, _] = words;
        if let Some((index, down)) = transition(message as u32, key as u32, lparam as u32) {
            let pressed = down && self.asynchronous[index] & 0x80 == 0;
            if down {
                if self.asynchronous[index] & 0x80 == 0 {
                    self.asynchronous[index] |= 1;
                }
                self.asynchronous[index] |= 0x80;
            } else {
                self.asynchronous[index] &= 1;
            }
            aggregate(&mut self.asynchronous, index, pressed);
        }
        if matches!(message, 0x100 | 0x104) {
            if self.text.len() >= 10000 {
                self.text.pop_front();
            }
            self.text.push_back((words, Vec::new()));
        } else if matches!(message, 0x102 | 0x106) {
            if let Some((last, text)) = self.text.back_mut().filter(|(last, _)| last[0] == hwnd) {
                if last[1] == 0x100 || last[1] == 0x104 {
                    if text.len() < 32 {
                        text.push(key as u16);
                    }
                    return false;
                }
            }
        }
        true
    }
    pub(crate) fn consumed(&mut self, tid: u32, message: u32, key: u32, lparam: u32) {
        self.consumed64(tid as u64, message, key, lparam);
    }
    pub(crate) fn consumed64(&mut self, tid: u64, message: u32, key: u32, lparam: u32) {
        if let Some((index, down)) = transition(message, key, lparam) {
            let state = self.queued.entry(tid).or_insert([0; 256]);
            if down {
                if matches!(index, 0x14 | 0x90 | 0x91) && lparam & (1 << 30) == 0 {
                    state[index] ^= 1;
                }
                state[index] |= 0x80;
            } else {
                state[index] &= 1;
            }
            aggregate(state, index, false);
        }
    }
    pub(crate) fn translate(&mut self, words: [u32; 5]) -> Vec<u16> {
        self.translate64(words.map(u64::from))
    }
    pub(crate) fn translate64(&mut self, words: [u64; 5]) -> Vec<u16> {
        if let Some(i) = self.text.iter().position(|(key, _)| *key == words) {
            return self.text.remove(i).unwrap().1;
        }
        // Culture-independent controls for posted key-down messages. Other
        // text comes from the host's active layout; no guessed US keymap.
        match words[2] {
            8 | 9 | 13 | 27 => vec![words[2] as u16],
            _ => Vec::new(),
        }
    }
    pub(crate) fn asynchronous(&mut self, key: u32) -> u32 {
        let Some(value) = self.asynchronous.get_mut(key as usize) else {
            return 0;
        };
        let result = (((*value as u16 & 0x80) << 8) | (*value as u16 & 1)) as i16 as i32 as u32;
        *value &= 0x80;
        result
    }
    pub(crate) fn state(&self, tid: u32) -> [u8; 256] {
        self.state64(tid as u64)
    }
    pub(crate) fn state64(&self, tid: u64) -> [u8; 256] {
        self.queued.get(&tid).copied().unwrap_or([0; 256])
    }
    pub(crate) fn exit(&mut self, tid: u32) {
        self.exit64(tid as u64);
    }
    pub(crate) fn exit64(&mut self, tid: u64) {
        self.queued.remove(&tid);
    }
    pub(crate) fn remove_window(&mut self, hwnd: u32) {
        self.remove_window64(hwnd as u64);
    }
    pub(crate) fn remove_window64(&mut self, hwnd: u64) {
        self.text.retain(|(key, _)| key[0] != hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::Keyboard;

    #[test]
    fn modifier_state_keeps_full_thread_ids_and_independent_press_latches() {
        let mut k = Keyboard::default();
        let tid = 1u64 << 40 | 1;
        for (lp, side) in [(0x2a0001, 0xa0), (0x360001, 0xa1)] {
            k.host64([1u64 << 42, 0x100, 16, lp, 7]);
            assert_eq!(k.asynchronous(16), 0xffff8001);
            assert_eq!(k.asynchronous(side), 0xffff8001);
            k.consumed64(tid, 0x100, 16, lp as u32);
        }
        assert_eq!(k.state64(1)[16], 0);
        k.consumed64(tid, 0x101, 16, 0xc02a0001);
        assert_eq!(k.state64(tid)[16], 0x80);
        assert_eq!(k.state64(tid)[0xa0], 0);
        assert_eq!(k.state64(tid)[0xa1], 0x80);
        k.exit64(1);
        assert_eq!(k.state64(tid)[16], 0x80);
        k.exit64(tid);
        assert_eq!(k.state64(tid)[16], 0);
    }

    #[test]
    fn invalid_keys_and_xbuttons_do_not_alias_real_keys() {
        let mut k = Keyboard::default();
        for (message, key) in [
            (0x100, 256),
            (0x100, u32::MAX),
            (0x20b, 0),
            (0x20b, 3 << 16),
        ] {
            k.host([1, message, key, 0, 0]);
            k.consumed(1, message, key, 0);
        }
        assert_eq!(k.state(1), [0; 256]);
        for key in 0..256 {
            assert_eq!(k.asynchronous(key), 0);
        }
    }
}
