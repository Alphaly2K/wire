//! Portable DirectSound COM objects, PCM formats and software mixing.
//! Interface ABI: pinned LGPL Hangover Wine dsound.h; see third-party notices.
use std::collections::BTreeMap;
#[path = "dsound/calls.rs"]
mod calls;
pub(crate) trait Memory {
    type Error: From<u32>;
    fn check(&self, p: u32, n: u32, write: bool) -> Result<(), Self::Error>;
    fn read_bytes(&self, p: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write_bytes(&mut self, p: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn read_u32(&self, p: u32) -> Result<u32, Self::Error> {
        let mut bytes = [0; 4];
        self.read_bytes(p, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn read_u16(&self, p: u32) -> Result<u16, Self::Error> {
        let mut bytes = [0; 2];
        self.read_bytes(p, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }
}
pub(crate) trait Backend: Memory {
    fn allocate(&mut self, n: u32) -> Result<u32, Self::Error>;
    fn check_free(&self, p: u32) -> Result<(), Self::Error>;
    fn free(&mut self, p: u32) -> Result<(), Self::Error>;
    fn register(&mut self, method: Method) -> Result<u32, Self::Error>;
    fn check_methods(&self, kind: &str) -> Result<(), Self::Error>;
    fn event(&self, handle: u32) -> bool;
    fn window(&self, handle: u32) -> bool;
    fn device(&mut self) -> bool;
    fn status(error: &Self::Error) -> Option<u32>;
}
fn output<B: Backend>(mem: &mut B, p: u32, result: Result<u32, B::Error>) -> Result<u32, B::Error> {
    match result {
        Ok(value) => {
            put(mem, p, value)?;
            Ok(0)
        }
        Err(error) => {
            if B::status(&error).is_some() {
                put(mem, p, 0)?;
            }
            Err(error)
        }
    }
}
const INVALID: u32 = 0x80070057;
const UNSUPPORTED: u32 = 0x80004001;
const CONTROL: u32 = 0x8878001e;
pub(crate) const ABI: &str = include_str!("../../third_party/hangover/dsound-methods.tsv");
#[derive(Clone, Copy)]
pub(super) struct Format {
    pub(super) channels: u16,
    pub(super) rate: u32,
    pub(super) bits: u16,
}
impl Format {
    fn block(self) -> u32 {
        self.channels as u32 * self.bits as u32 / 8
    }
    fn read<M: Memory>(mem: &M, p: u32) -> Result<Self, M::Error> {
        mem.check(p, 16, false)?;
        let f = Self {
            channels: mem.read_u16(p + 2)?,
            rate: mem.read_u32(p + 4)?,
            bits: mem.read_u16(p + 14)?,
        };
        if mem.read_u16(p)? != 1
            || !matches!(f.channels, 1 | 2)
            || !matches!(f.bits, 8 | 16)
            || !(100..=200000).contains(&f.rate)
            || mem.read_u16(p + 12)? as u32 != f.block()
            || mem.read_u32(p + 8)? != f.rate * f.block()
        {
            return Err(INVALID.into());
        }
        Ok(f)
    }
    fn bytes(self) -> [u8; 18] {
        let mut b = [0; 18];
        b[..2].copy_from_slice(&1u16.to_le_bytes());
        b[2..4].copy_from_slice(&self.channels.to_le_bytes());
        b[4..8].copy_from_slice(&self.rate.to_le_bytes());
        b[8..12].copy_from_slice(&(self.rate * self.block()).to_le_bytes());
        b[12..14].copy_from_slice(&(self.block() as u16).to_le_bytes());
        b[14..16].copy_from_slice(&self.bits.to_le_bytes());
        b
    }
}
pub(super) struct SoundBuffer {
    pub(super) data: u32,
    pub(super) len: u32,
    pub(super) flags: u32,
    pub(super) format: Format,
    pub(super) frequency: u32,
    pub(super) volume: i32,
    pub(super) pan: i32,
    pub(super) position: u64,
    pub(super) playing: bool,
    pub(super) looping: bool,
    pub(super) lock: Option<[u32; 4]>,
    pub(super) notifications: Vec<(u32, u32)>,
}
pub(super) struct Object {
    pub(super) kind: &'static str,
    pub(super) refs: u32,
    pub(super) buffer: Option<SoundBuffer>,
    pub(super) owner: u32,
}
#[derive(Default)]
pub struct DSound {
    pub(super) objects: BTreeMap<u32, Object>,
    tables: BTreeMap<&'static str, u32>,
    allocated: u64,
    pub mixed_frames: u64,
    pub locked_playback_frames: u64,
    pub(super) pending: Vec<u32>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Method {
    pub name: &'static str,
    pub stack: u32,
}
impl Method {
    pub(crate) fn export(name: &str) -> Option<Self> {
        let ordinal = name.strip_prefix('#').and_then(|n| n.parse::<u16>().ok());
        include_str!("../../third_party/hangover/dsound.spec")
            .lines()
            .find_map(|line| {
                let (head, rest) = line.split_once('(')?;
                let fields = head.split_whitespace().collect::<Vec<_>>();
                let candidate = *fields.last()?;
                if fields.get(1) != Some(&"stdcall")
                    || (candidate != name
                        && (ordinal.is_none() || fields[0].parse::<u16>().ok() != ordinal))
                {
                    return None;
                }
                Some(Self {
                    name: candidate,
                    stack: rest.split(')').next()?.split_whitespace().count() as u32 * 4,
                })
            })
    }
    pub(crate) fn lookup(name: &str) -> Option<Self> {
        if let Some(method) = Self::export(name) {
            return Some(method);
        }
        ABI.lines().find_map(|line| {
            let mut fields = line.split('\t');
            let candidate = fields.next()?;
            (candidate == name)
                .then(|| {
                    Some(Self {
                        name: candidate,
                        stack: fields.nth(1)?.parse().ok()?,
                    })
                })
                .flatten()
        })
    }
}
fn put<M: Memory>(mem: &mut M, p: u32, n: u32) -> Result<(), M::Error> {
    mem.check(p, 4, true)?;
    mem.write_bytes(p, &n.to_le_bytes())
}
fn alloc<B: Backend>(mem: &mut B, n: u32) -> Result<u32, B::Error> {
    let p = mem.allocate(n)?;
    if p == 0 {
        Err(0x8007000e.into())
    } else {
        Ok(p)
    }
}
impl DSound {
    #[allow(dead_code)]
    pub(crate) fn playing(&self) -> bool {
        self.objects
            .values()
            .any(|o| o.buffer.as_ref().is_some_and(|b| b.playing && b.data != 0))
    }
    pub(crate) fn take_events(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.pending)
    }
    fn buffer_call<M: Memory>(
        m: &str,
        b: &mut SoundBuffer,
        a: &[u32; 16],
        mem: &mut M,
    ) -> Result<u32, M::Error> {
        match m {
            "GetCaps" => {
                if mem.read_u32(a[1])? != 20 {
                    return Err(INVALID.into());
                }
                mem.check(a[1], 20, true)?;
                let values = [20, b.flags | 8, b.len, 0, 0];
                for (i, n) in values.into_iter().enumerate() {
                    put(mem, a[1] + i as u32 * 4, n)?;
                }
                Ok(0)
            }
            "GetFormat" => {
                if a[1] != 0 {
                    if a[2] < 18 {
                        return Err(INVALID.into());
                    }
                    mem.check(a[1], 18, true)?;
                }
                if a[3] != 0 {
                    mem.check(a[3], 4, true)?;
                }
                if a[1] != 0 {
                    mem.write_bytes(a[1], &b.format.bytes())?;
                }
                if a[3] != 0 {
                    put(mem, a[3], 18)?;
                }
                Ok(0)
            }
            "SetFormat" => {
                if b.flags & 1 == 0 {
                    return Err(0x88780032.into());
                }
                b.format = Format::read(mem, a[1])?;
                b.frequency = b.format.rate;
                Ok(0)
            }
            "GetCurrentPosition" => {
                for p in [a[1], a[2]].into_iter().filter(|&p| p != 0) {
                    mem.check(p, 4, true)?;
                }
                let pos = (b.position >> 32) as u32 * b.format.block();
                if a[1] != 0 {
                    put(mem, a[1], pos)?;
                }
                if a[2] != 0 {
                    put(
                        mem,
                        a[2],
                        if b.len == 0 {
                            0
                        } else {
                            (pos + b.format.rate / 100 * b.format.block()) % b.len
                        },
                    )?;
                }
                Ok(0)
            }
            "GetStatus" => {
                put(
                    mem,
                    a[1],
                    b.playing as u32 | if b.playing && b.looping { 4 } else { 0 },
                )?;
                Ok(0)
            }
            "GetVolume" | "GetPan" | "GetFrequency" => {
                let (flag, value) = match m {
                    "GetVolume" => (0x80, b.volume as u32),
                    "GetPan" => (0x40, b.pan as u32),
                    _ => (0x20, b.frequency),
                };
                if b.flags & flag == 0 {
                    return Err(CONTROL.into());
                }
                put(mem, a[1], value)?;
                Ok(0)
            }
            "SetVolume" | "SetPan" | "SetFrequency" => {
                let n = a[1] as i32;
                let flag = match m {
                    "SetVolume" => 0x80,
                    "SetPan" => 0x40,
                    _ => 0x20,
                };
                if b.flags & flag == 0 {
                    return Err(CONTROL.into());
                }
                match m {
                    "SetVolume" if (-10000..=0).contains(&n) => b.volume = n,
                    "SetPan" if (-10000..=10000).contains(&n) => b.pan = n,
                    "SetFrequency" if a[1] == 0 => b.frequency = b.format.rate,
                    "SetFrequency" if (100..=200000).contains(&a[1]) => b.frequency = a[1],
                    _ => return Err(INVALID.into()),
                }
                Ok(0)
            }
            "SetCurrentPosition" => {
                if b.len == 0 || a[1] >= b.len {
                    return Err(INVALID.into());
                }
                b.position = ((a[1] / b.format.block()) as u64) << 32;
                Ok(0)
            }
            "Play" => {
                if a[1] != 0 || a[2] != 0 || a[3] & !1 != 0 {
                    return Err(UNSUPPORTED.into());
                }
                b.playing = true;
                b.looping = a[3] & 1 != 0;
                Ok(0)
            }
            "Stop" => {
                b.playing = false;
                Ok(0)
            }
            "Restore" => Ok(0), // software storage cannot be lost to driver focus changes
            "Lock" => {
                if b.data == 0 || b.lock.is_some() || a[7] & !3 != 0 || a[7] == 3 {
                    return Err(INVALID.into());
                }
                let offset = if a[7] & 1 != 0 {
                    (b.position >> 32) as u32 * b.format.block()
                } else {
                    a[1]
                };
                let len = if a[7] & 2 != 0 { b.len } else { a[2] };
                if offset >= b.len || len > b.len {
                    return Err(INVALID.into());
                }
                let first = len.min(b.len - offset);
                let second = len - first;
                if second != 0 && (a[5] == 0 || a[6] == 0) {
                    return Err(INVALID.into());
                }
                for p in [a[3], a[4]]
                    .into_iter()
                    .chain([a[5], a[6]].into_iter().filter(|&p| p != 0))
                {
                    mem.check(p, 4, true)?;
                }
                mem.check(b.data, b.len, true)?;
                put(mem, a[3], b.data + offset)?;
                put(mem, a[4], first)?;
                if a[5] != 0 {
                    put(mem, a[5], if second == 0 { 0 } else { b.data })?;
                }
                if a[6] != 0 {
                    put(mem, a[6], second)?;
                }
                b.lock = Some([
                    b.data + offset,
                    first,
                    if second == 0 { 0 } else { b.data },
                    second,
                ]);
                Ok(0)
            }
            "Unlock" => {
                if b.lock != Some([a[1], a[2], a[3], a[4]]) {
                    return Err(INVALID.into());
                }
                b.lock = None;
                Ok(0)
            }
            _ => Err(UNSUPPORTED.into()),
        }
    }
    pub(crate) fn mix<M: Memory>(&mut self, mem: &M, frames: u32) -> Result<Vec<i16>, M::Error> {
        for o in self.objects.values() {
            if let Some(b) = &o.buffer {
                if b.playing && b.data != 0 {
                    mem.check(b.data, b.len, false)?;
                }
            }
        }
        let mut out = vec![0i64; frames as usize * 2];
        for o in self.objects.values_mut() {
            let Some(b) = &mut o.buffer else { continue };
            if !b.playing || b.data == 0 {
                continue;
            }
            let mut data = vec![0; b.len as usize];
            mem.read_bytes(b.data, &mut data)?;
            let length = b.len / b.format.block();
            let gain = |db: i32| {
                if db <= -10000 {
                    0.
                } else {
                    10f64.powf(db as f64 / 2000.)
                }
            };
            let gains = [gain(b.volume - b.pan.max(0)), gain(b.volume + b.pan.min(0))];
            let step = ((b.frequency as u64) << 32) / 44100;
            let begin = b.position;
            let mut rendered = 0u64;
            for frame in 0..frames as usize {
                if b.position >> 32 >= length as u64 {
                    if b.looping {
                        b.position %= (length as u64) << 32;
                    } else {
                        b.playing = false;
                        b.position = 0;
                        break;
                    }
                }
                rendered += 1;
                let start = (b.position >> 32) as usize * b.format.block() as usize;
                if b.lock.is_some_and(|[p, n, p2, n2]| {
                    let at = b.data + start as u32;
                    (p <= at && at - p < n) || (p2 <= at && at - p2 < n2)
                }) {
                    self.locked_playback_frames += 1;
                }
                for ch in 0..2 {
                    let p = start
                        + if b.format.channels == 1 {
                            0
                        } else {
                            ch * b.format.bits as usize / 8
                        };
                    let n = if b.format.bits == 8 {
                        (data[p] as i32 - 128) * 256
                    } else {
                        i16::from_le_bytes([data[p], data[p + 1]]) as i32
                    };
                    out[frame * 2 + ch] += (n as f64 * gains[ch]).round() as i64;
                }
                b.position += step;
            }
            if b.position >> 32 >= length as u64 {
                if b.looping {
                    b.position %= (length as u64) << 32;
                } else {
                    b.playing = false;
                    b.position = 0;
                }
            }
            let end = begin + step * rendered;
            let ring = (length as u64) << 32;
            for &(offset, event) in &b.notifications {
                if offset == u32::MAX {
                    if !b.playing {
                        self.pending.push(event);
                    }
                    continue;
                }
                let phase = ((offset / b.format.block()) as u64) << 32;
                if end - begin >= ring
                    || (begin <= phase && phase < end)
                    || (end >= ring && phase < end - ring)
                {
                    self.pending.push(event);
                }
            }
        }
        self.mixed_frames += frames as u64;
        Ok(out
            .into_iter()
            .map(|n| n.clamp(-32768, 32767) as i16)
            .collect())
    }
}
