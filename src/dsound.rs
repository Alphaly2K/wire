//! Bounded software PCM DirectSound device. The headless sink consumes the
//! same mixer output; visible macOS sessions use the public AudioQueue API.
//! Interface ABI: pinned LGPL Hangover Wine dsound.h, see third_party notices.
use crate::{
    api::{ApiState, ApiTable, Args},
    heap::GuestHeap,
    mem::GuestMem,
    win32::kernel::objects32::KernelObjects32,
};
use std::{collections::BTreeMap, time::Instant};

mod calls;
const INVALID: u32 = 0x80070057;
const UNSUPPORTED: u32 = 0x80004001;
const CONTROL: u32 = 0x8878001e;
const ABI: &str = include_str!("../third_party/hangover/dsound-methods.tsv");
#[derive(Clone, Copy)]
struct Format {
    channels: u16,
    rate: u32,
    bits: u16,
}
impl Format {
    fn block(self) -> u32 {
        self.channels as u32 * self.bits as u32 / 8
    }
    fn read(mem: &GuestMem, p: u32) -> Result<Self, u32> {
        mem.ptr(p, 16, false).map_err(|_| INVALID)?;
        let f = Self {
            channels: mem.read_u16(p + 2).map_err(|_| INVALID)?,
            rate: mem.read_u32(p + 4).map_err(|_| INVALID)?,
            bits: mem.read_u16(p + 14).map_err(|_| INVALID)?,
        };
        if mem.read_u16(p).map_err(|_| INVALID)? != 1
            || !matches!(f.channels, 1 | 2)
            || !matches!(f.bits, 8 | 16)
            || !(100..=200000).contains(&f.rate)
            || mem.read_u16(p + 12).map_err(|_| INVALID)? as u32 != f.block()
            || mem.read_u32(p + 8).map_err(|_| INVALID)? != f.rate * f.block()
        {
            return Err(INVALID);
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
struct SoundBuffer {
    data: u32,
    len: u32,
    flags: u32,
    format: Format,
    frequency: u32,
    volume: i32,
    pan: i32,
    position: u64,
    playing: bool,
    looping: bool,
    lock: Option<[u32; 4]>,
    notifications: Vec<(u32, u32)>,
}
struct Object {
    kind: &'static str,
    refs: u32,
    buffer: Option<SoundBuffer>,
    owner: u32,
}
pub struct DSound {
    objects: BTreeMap<u32, Object>,
    tables: BTreeMap<&'static str, u32>,
    allocated: u64,
    native: crate::host::audio::PcmOutput,
    last: Instant,
    fraction: u128,
    pub mixed_frames: u64,
    pub dropped_samples: u64,
    pub output_stats: [u64; 4], // consumed samples, underrun samples, callbacks, queued samples
    pub max_pump_gap_ms: u128,
    pub locked_playback_frames: u64,
    pub error: Option<String>,
    pending: Vec<u32>,
}
impl Default for DSound {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            tables: BTreeMap::new(),
            allocated: 0,
            native: Default::default(),
            last: Instant::now(),
            fraction: 0,
            mixed_frames: 0,
            dropped_samples: 0,
            output_stats: [0; 4],
            max_pump_gap_ms: 0,
            locked_playback_frames: 0,
            error: None,
            pending: Vec::new(),
        }
    }
}
impl Drop for DSound {
    fn drop(&mut self) {
        // Stop the sink before retiring guest-side object and buffer state.
        self.native.close();
    }
}
pub(crate) fn stack_bytes(name: &str) -> Option<u32> {
    ABI.lines().find_map(|l| {
        let mut f = l.split('\t');
        (f.next()? == name)
            .then(|| f.nth(1)?.parse().ok())
            .flatten()
    })
}
fn put(mem: &GuestMem, p: u32, n: u32) -> Result<(), u32> {
    mem.write_u32(p, n).map_err(|_| INVALID)
}
fn alloc(mem: &mut GuestMem, heap: &mut GuestHeap, process_heap: u32, n: u32) -> Result<u32, u32> {
    let p = heap
        .alloc(mem, process_heap, n, true)
        .map_err(|_| 0x8007000eu32)?;
    if p == 0 {
        Err(0x8007000e)
    } else {
        Ok(p)
    }
}
impl DSound {
    fn buffer_call(m: &str, b: &mut SoundBuffer, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        match m {
            "GetCaps" => {
                if mem.read_u32(a[1]).map_err(|_| INVALID)? != 20 {
                    return Err(INVALID);
                }
                let values = [20, b.flags | 8, b.len, 0, 0];
                for (i, n) in values.into_iter().enumerate() {
                    put(mem, a[1] + i as u32 * 4, n)?;
                }
                Ok(0)
            }
            "GetFormat" => {
                if a[1] != 0 {
                    if a[2] < 18 {
                        return Err(INVALID);
                    }
                    mem.write_bytes(a[1], &b.format.bytes())
                        .map_err(|_| INVALID)?;
                }
                if a[3] != 0 {
                    put(mem, a[3], 18)?;
                }
                Ok(0)
            }
            "SetFormat" => {
                if b.flags & 1 == 0 {
                    return Err(0x88780032);
                }
                b.format = Format::read(mem, a[1])?;
                b.frequency = b.format.rate;
                Ok(0)
            }
            "GetCurrentPosition" => {
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
                    return Err(CONTROL);
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
                    return Err(CONTROL);
                }
                match m {
                    "SetVolume" if (-10000..=0).contains(&n) => b.volume = n,
                    "SetPan" if (-10000..=10000).contains(&n) => b.pan = n,
                    "SetFrequency" if a[1] == 0 => b.frequency = b.format.rate,
                    "SetFrequency" if (100..=200000).contains(&a[1]) => b.frequency = a[1],
                    _ => return Err(INVALID),
                }
                Ok(0)
            }
            "SetCurrentPosition" => {
                if b.len == 0 || a[1] >= b.len {
                    return Err(INVALID);
                }
                b.position = ((a[1] / b.format.block()) as u64) << 32;
                Ok(0)
            }
            "Play" => {
                if a[1] != 0 || a[2] != 0 || a[3] & !1 != 0 {
                    return Err(UNSUPPORTED);
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
                    return Err(INVALID);
                }
                let offset = if a[7] & 1 != 0 {
                    (b.position >> 32) as u32 * b.format.block()
                } else {
                    a[1]
                };
                let len = if a[7] & 2 != 0 { b.len } else { a[2] };
                if offset >= b.len || len > b.len {
                    return Err(INVALID);
                }
                let first = len.min(b.len - offset);
                let second = len - first;
                if second != 0 && (a[5] == 0 || a[6] == 0) {
                    return Err(INVALID);
                }
                for p in [a[3], a[4]]
                    .into_iter()
                    .chain([a[5], a[6]].into_iter().filter(|&p| p != 0))
                {
                    mem.ptr(p, 4, true).map_err(|_| INVALID)?;
                }
                mem.ptr(b.data, b.len, true).map_err(|_| INVALID)?;
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
                    return Err(INVALID);
                }
                b.lock = None;
                Ok(0)
            }
            _ => Err(UNSUPPORTED),
        }
    }
    fn mix(&mut self, mem: &GuestMem, frames: u32) -> Result<Vec<i16>, u32> {
        let mut out = vec![0i64; frames as usize * 2];
        for o in self.objects.values_mut() {
            let Some(b) = &mut o.buffer else { continue };
            if !b.playing || b.data == 0 {
                continue;
            }
            let mut data = vec![0; b.len as usize];
            mem.read_bytes(b.data, &mut data).map_err(|_| INVALID)?;
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
    fn pump(&mut self, mem: &GuestMem) {
        if self.objects.is_empty() {
            self.last = Instant::now();
            return;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        if elapsed.as_millis() < 5 {
            return;
        }
        self.last = now;
        self.max_pump_gap_ms = self.max_pump_gap_ms.max(elapsed.as_millis());
        let total = elapsed.as_nanos() * 44100 + self.fraction;
        self.fraction = total % 1_000_000_000;
        let frames = (total / 1_000_000_000).min(44100) as u32;
        match self.mix(mem, frames) {
            Ok(samples) => {
                if self.native.active() {
                    let n = self.native.push(&samples);
                    self.dropped_samples += samples.len() as u64 - n as u64;
                    self.native.stats(&mut self.output_stats);
                }
            }
            Err(e) => {
                self.error = Some(format!("PCM guest read failed: {e:#x}"));
                for o in self.objects.values_mut() {
                    if let Some(b) = &mut o.buffer {
                        b.playing = false;
                    }
                }
            }
        }
    }
}
fn signal(sound: &mut DSound, handles: &mut KernelObjects32, sched: &mut crate::thread::Sched) {
    for event in std::mem::take(&mut sound.pending) {
        crate::win32::x86::object_waits::signal_audio_event(handles, sched, event);
    }
}
pub(crate) fn poll(mem: &GuestMem, state: &mut ApiState, sched: &mut crate::thread::Sched) {
    state.dsound.pump(mem);
    signal(&mut state.dsound, &mut state.handles, sched);
}
pub(crate) fn dispatch(
    name: &str,
    a: &Args,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    sched: &mut crate::thread::Sched,
) -> (u32, String, bool) {
    let sound = &mut state.dsound;
    sound.pump(mem);
    let result = sound.call(
        name,
        a,
        mem,
        api,
        &mut calls::Resources {
            heap: &mut state.heap,
            process_heap: state.process_heap,
            events: &state.handles,
            windows: &state.user32.windows,
            presenter: &state.presenter,
        },
    );
    let note = format!(
        "software PCM; frames={}; native={}; error={:?}",
        sound.mixed_frames,
        sound.native.active(),
        result.as_ref().err()
    );
    let note = if name.ends_with(".CreateSoundBuffer") {
        format!(
            "{note}; desc={:?}",
            (0..5)
                .map(|i| mem.read_u32(a[1].wrapping_add(i * 4)))
                .collect::<Vec<_>>()
        )
    } else {
        note
    };
    let note = if name.ends_with(".SetNotificationPositions") {
        format!(
            "{note}; notifications={:?}",
            (0..a[1].min(1024))
                .map(|i| (
                    mem.read_u32(a[2].wrapping_add(i * 8)),
                    mem.read_u32(a[2].wrapping_add(i * 8 + 4))
                ))
                .collect::<Vec<_>>()
        )
    } else {
        note
    };
    signal(&mut state.dsound, &mut state.handles, sched);
    (
        result.unwrap_or_else(|e| e),
        note,
        result != Err(UNSUPPORTED),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_mixer_resamples_loops_clips_and_stops_at_buffer_end() {
        let mut mem = GuestMem::new().unwrap();
        mem.commit(0x20000, 4096).unwrap();
        mem.write_bytes(0x20000, &[0, 0, 0xff, 0x7f, 0, 0x80, 0, 0x40])
            .unwrap();
        let mut s = DSound::default();
        s.objects.insert(
            1,
            Object {
                kind: "IDirectSoundBuffer",
                refs: 1,
                owner: 0,
                buffer: Some(SoundBuffer {
                    data: 0x20000,
                    len: 8,
                    flags: 0xe0,
                    format: Format {
                        channels: 1,
                        rate: 44100,
                        bits: 16,
                    },
                    frequency: 44100,
                    volume: 0,
                    pan: 0,
                    position: 0,
                    playing: true,
                    looping: false,
                    lock: None,
                    notifications: vec![(2, 10), (u32::MAX, 11)],
                }),
            },
        );
        assert_eq!(
            s.mix(&mem, 5).unwrap(),
            [0, 0, 32767, 32767, -32768, -32768, 16384, 16384, 0, 0]
        );
        assert_eq!(s.pending, [10, 11]);
        s.pending.clear();
        let b = s.objects.get_mut(&1).unwrap().buffer.as_mut().unwrap();
        assert!(!b.playing);
        assert_eq!(b.position, 0);
        b.playing = true;
        b.looping = true;
        b.frequency = 22050;
        b.pan = 10000;
        assert_eq!(s.mix(&mem, 4).unwrap(), [0, 0, 0, 0, 0, 32767, 0, 32767]);
        let b = s.objects.get_mut(&1).unwrap().buffer.as_mut().unwrap();
        b.frequency = 44100;
        b.position = 0;
        b.pan = 0;
        s.objects.insert(
            2,
            Object {
                kind: "IDirectSoundBuffer",
                refs: 1,
                owner: 0,
                buffer: Some(SoundBuffer {
                    data: 0x20006,
                    len: 2,
                    flags: 0,
                    format: Format {
                        channels: 1,
                        rate: 44100,
                        bits: 16,
                    },
                    frequency: 44100,
                    volume: 0,
                    pan: 0,
                    position: 0,
                    playing: true,
                    looping: true,
                    lock: None,
                    notifications: vec![],
                }),
            },
        );
        assert_eq!(
            s.mix(&mem, 3).unwrap(),
            [16384, 16384, 32767, 32767, -16384, -16384]
        );
    }
}
