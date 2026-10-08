//! Host key state and resolved Unicode text stay data until TranslateMessage.
use std::collections::VecDeque;
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
        let [hwnd, message, key, _, _] = words;
        let index = match message {
            0x100 | 0x101 | 0x104 | 0x105 => Some(key as usize),
            0x201 | 0x202 => Some(1),
            0x204 | 0x205 => Some(2),
            _ => None,
        };
        if let Some(index) = index.filter(|&i| i < 256) {
            if matches!(message, 0x100 | 0x104 | 0x201 | 0x204) {
                self.asynchronous[index] |= 0x81;
            } else {
                self.asynchronous[index] &= 1;
            }
        }
        if matches!(message, 0x100 | 0x104) {
            if self.text.len() >= 10000 {
                self.text.pop_front();
            }
            self.text.push_back((words, Vec::new()));
        } else if message == 0x102 {
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
        let index = match message {
            0x100 | 0x101 | 0x104 | 0x105 => Some(key as usize),
            0x201 | 0x202 => Some(1),
            0x204 | 0x205 => Some(2),
            _ => None,
        };
        if let Some(index) = index.filter(|&i| i < 256) {
            let state = self.queued.entry(tid).or_insert([0; 256]);
            if matches!(message, 0x100 | 0x104 | 0x201 | 0x204) {
                if matches!(index, 0x14 | 0x90 | 0x91) && lparam & (1 << 30) == 0 {
                    state[index] ^= 1;
                }
                state[index] |= 0x80;
            } else {
                state[index] &= 1;
            }
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
