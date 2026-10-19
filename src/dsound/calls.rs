//! DirectSound object operations borrow only their heap and read-only device prerequisites.
#[cfg(target_os = "macos")]
use super::wire_audio_create;
use super::{alloc, put, DSound, Format, Object, SoundBuffer, ABI, INVALID, UNSUPPORTED};
use crate::{
    api::{ApiTable, Args},
    heap::GuestHeap,
    host::ui::Presenter,
    mem::GuestMem,
    user32::Window,
    win32::kernel::objects32::KernelObjects32,
};
use std::collections::BTreeMap;

pub(super) struct Resources<'a> {
    pub heap: &'a mut GuestHeap,
    pub process_heap: u32,
    pub events: &'a KernelObjects32,
    pub windows: &'a BTreeMap<u32, Window>,
    pub presenter: &'a Presenter,
}

impl DSound {
    fn create(
        &mut self,
        kind: &'static str,
        buffer: Option<SoundBuffer>,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        heap: &mut GuestHeap,
        process_heap: u32,
    ) -> Result<u32, u32> {
        if self.objects.len() >= 256 {
            return Err(0x8007000e);
        }
        let table = if let Some(&p) = self.tables.get(kind) {
            p
        } else {
            let entries: Vec<_> = ABI
                .lines()
                .filter(|l| l.starts_with(&format!("{kind}.")))
                .map(|l| api.register("dsound.dll", Some(l.split('\t').next().unwrap()), None))
                .collect();
            let p = alloc(mem, heap, process_heap, entries.len() as u32 * 4)?;
            for (i, n) in entries.iter().enumerate() {
                put(mem, p + i as u32 * 4, *n)?;
            }
            self.tables.insert(kind, p);
            p
        };
        let p = alloc(mem, heap, process_heap, 4)?;
        put(mem, p, table)?;
        self.objects.insert(
            p,
            Object {
                kind,
                refs: 1,
                buffer,
                owner: 0,
            },
        );
        Ok(p)
    }
    pub(super) fn call(
        &mut self,
        name: &str,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        resources: &mut Resources<'_>,
    ) -> Result<u32, u32> {
        if matches!(name, "DirectSoundCreate" | "DirectSoundCreate8") {
            put(mem, a[1], 0)?;
            if a[2] != 0 {
                return Err(0x80040110);
            }
            if a[0] != 0 {
                let mut guid = [0; 16];
                mem.read_bytes(a[0], &mut guid).map_err(|_| INVALID)?;
                if guid != [0; 16] {
                    return Err(0x88780078);
                }
            }
            if self.native == 0 && resources.presenter.active() {
                #[cfg(target_os = "macos")]
                {
                    self.native = unsafe { wire_audio_create() } as usize;
                    if self.native == 0 {
                        return Err(0x88780078);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                return Err(0x88780078);
            }
            let kind = if name.ends_with('8') {
                "IDirectSound8"
            } else {
                "IDirectSound"
            };
            let h = self.create(kind, None, mem, api, resources.heap, resources.process_heap)?;
            put(mem, a[1], h)?;
            return Ok(0);
        }
        let (kind, m) = name.split_once('.').ok_or(UNSUPPORTED)?;
        let o = self.objects.get_mut(&a[0]).ok_or(INVALID)?;
        if o.kind != kind {
            return Err(INVALID);
        }
        match m {
            "QueryInterface" => {
                put(mem, a[2], 0)?;
                let mut guid = [0; 16];
                mem.read_bytes(a[1], &mut guid).map_err(|_| INVALID)?;
                if kind == "IDirectSoundBuffer"
                    && guid
                        == [
                            0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9,
                            0x25, 0xcd, 0x16,
                        ]
                {
                    if o.buffer.as_ref().unwrap().flags & 0x100 == 0 {
                        return Err(0x80004002);
                    }
                    if let Some((&h, _)) = self
                        .objects
                        .iter()
                        .find(|(_, o)| o.owner == a[0] && o.kind == "IDirectSoundNotify")
                    {
                        let notify = self.objects.get_mut(&h).unwrap();
                        notify.refs = notify.refs.checked_add(1).ok_or(0x8007000eu32)?;
                        put(mem, a[2], h)?;
                        return Ok(0);
                    }
                    let owner = self.objects.get_mut(&a[0]).unwrap();
                    owner.refs = owner.refs.checked_add(1).ok_or(0x8007000eu32)?;
                    let result = self.create(
                        "IDirectSoundNotify",
                        None,
                        mem,
                        api,
                        resources.heap,
                        resources.process_heap,
                    );
                    let h = match result {
                        Ok(h) => h,
                        Err(e) => {
                            self.objects.get_mut(&a[0]).unwrap().refs -= 1;
                            return Err(e);
                        }
                    };
                    self.objects.get_mut(&h).unwrap().owner = a[0];
                    put(mem, a[2], h)?;
                    return Ok(0);
                }
                if kind == "IDirectSoundNotify"
                    && (guid == [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
                        || guid
                            == [
                                0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0,
                                0x20, 0xaf, 0xb, 0xe5, 0x60,
                            ])
                {
                    let owner = o.owner;
                    let owner_object = self.objects.get_mut(&owner).unwrap();
                    owner_object.refs = owner_object.refs.checked_add(1).ok_or(0x8007000eu32)?;
                    put(mem, a[2], owner)?;
                    return Ok(0);
                }
                let expected = if kind == "IDirectSoundBuffer" {
                    [
                        0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf,
                        0xb, 0xe5, 0x60,
                    ]
                } else if kind == "IDirectSoundNotify" {
                    [
                        0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9, 0x25,
                        0xcd, 0x16,
                    ]
                } else if kind == "IDirectSound8" {
                    [
                        0x93, 0x7e, 0x0a, 0xc5, 0x95, 0xf3, 0x34, 0x48, 0x9e, 0xf6, 0x7f, 0xa9,
                        0x9d, 0xe5, 9, 0x66,
                    ]
                } else {
                    [
                        0x83, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf,
                        0xb, 0xe5, 0x60,
                    ]
                };
                if guid != expected
                    && guid != [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
                {
                    return Err(0x80004002);
                }
                o.refs = o.refs.checked_add(1).ok_or(0x8007000eu32)?;
                put(mem, a[2], a[0])?;
                return Ok(0);
            }
            "AddRef" => {
                o.refs = o.refs.checked_add(1).ok_or(0x8007000eu32)?;
                return Ok(o.refs);
            }
            "Release" => {
                o.refs -= 1;
                let n = o.refs;
                if n == 0 {
                    let o = self.objects.remove(&a[0]).unwrap();
                    if let Some(b) = o.buffer {
                        if b.data != 0 {
                            resources.heap.free(resources.process_heap, b.data);
                            self.allocated -= b.len as u64;
                        }
                    }
                    resources.heap.free(resources.process_heap, a[0]);
                    if o.owner != 0 {
                        let mut args = [0; 16];
                        args[0] = o.owner;
                        self.call("IDirectSoundBuffer.Release", &args, mem, api, resources)?;
                    }
                }
                return Ok(n);
            }
            _ => {}
        }
        if kind == "IDirectSoundNotify" {
            if m != "SetNotificationPositions" {
                return Err(UNSUPPORTED);
            }
            let owner = o.owner;
            let b = self
                .objects
                .get_mut(&owner)
                .unwrap()
                .buffer
                .as_mut()
                .unwrap();
            if b.playing || a[1] > 1024 {
                return Err(INVALID);
            }
            let mut positions = Vec::new();
            for i in 0..a[1] {
                let p = a[2].checked_add(i * 8).ok_or(INVALID)?;
                mem.ptr(p, 8, false).map_err(|_| INVALID)?;
                let offset = mem.read_u32(p).map_err(|_| INVALID)?;
                let event = mem.read_u32(p + 4).map_err(|_| INVALID)?;
                if (offset != u32::MAX && offset >= b.len)
                    || !matches!(
                        resources.events.get(&event),
                        Some(crate::api::HandleObj::Event { .. })
                    )
                {
                    return Err(INVALID);
                }
                positions.push((offset, event));
            }
            b.notifications = positions;
            return Ok(0);
        }
        if let Some(b) = o.buffer.as_mut() {
            if m == "Stop" && b.playing {
                self.pending.extend(
                    b.notifications
                        .iter()
                        .filter_map(|&(offset, event)| (offset == u32::MAX).then_some(event)),
                );
            }
            return Self::buffer_call(m, b, a, mem);
        }
        match m {
            "SetCooperativeLevel" => {
                if !resources.windows.contains_key(&a[1]) || !(1..=4).contains(&a[2]) {
                    Err(INVALID)
                } else {
                    Ok(0)
                }
            }
            "GetSpeakerConfig" => {
                put(mem, a[1], 4)?;
                Ok(0)
            }
            "SetSpeakerConfig" => {
                if a[1] & 0xff == 4 {
                    Ok(0)
                } else {
                    Err(UNSUPPORTED)
                }
            }
            "Compact" => Ok(0),
            "GetCaps" => {
                if mem.read_u32(a[1]).map_err(|_| INVALID)? != 96 {
                    return Err(INVALID);
                }
                let mut b = [0u8; 96];
                for (i, n) in [(0, 96u32), (1, 0xf3f), (2, 100), (3, 200000), (4, 1)] {
                    b[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
                }
                mem.write_bytes(a[1], &b).map_err(|_| INVALID)?;
                Ok(0)
            }
            "CreateSoundBuffer" => {
                put(mem, a[2], 0)?;
                if a[3] != 0 {
                    return Err(0x80040110);
                }
                let size = mem.read_u32(a[1]).map_err(|_| INVALID)?;
                if !matches!(size, 20 | 36) {
                    return Err(INVALID);
                }
                mem.ptr(a[1], size, false).map_err(|_| INVALID)?;
                let flags = mem.read_u32(a[1] + 4).map_err(|_| INVALID)?;
                let len = mem.read_u32(a[1] + 8).map_err(|_| INVALID)?;
                let fp = mem.read_u32(a[1] + 16).map_err(|_| INVALID)?;
                if flags & !0x1c1eb != 0 {
                    return Err(UNSUPPORTED);
                } // no hardware, 3D or FX
                let primary = flags & 1 != 0;
                let format = if primary {
                    if len != 0 || fp != 0 {
                        return Err(INVALID);
                    }
                    Format {
                        channels: 2,
                        rate: 44100,
                        bits: 16,
                    }
                } else {
                    Format::read(mem, fp)?
                };
                if !primary && (len == 0 || len % format.block() != 0) {
                    return Err(INVALID);
                }
                if len > 16 * 1024 * 1024 || self.allocated + len as u64 > 64 * 1024 * 1024 {
                    return Err(0x8007000e);
                }
                let data = if primary {
                    0
                } else {
                    alloc(mem, resources.heap, resources.process_heap, len)?
                };
                if format.bits == 8 && data != 0 {
                    mem.write_bytes(data, &vec![128; len as usize])
                        .map_err(|_| INVALID)?;
                }
                let result = self.create(
                    "IDirectSoundBuffer",
                    Some(SoundBuffer {
                        data,
                        len,
                        flags,
                        format,
                        frequency: format.rate,
                        volume: 0,
                        pan: 0,
                        position: 0,
                        playing: false,
                        looping: false,
                        lock: None,
                        notifications: Vec::new(),
                    }),
                    mem,
                    api,
                    resources.heap,
                    resources.process_heap,
                );
                let h = match result {
                    Ok(h) => h,
                    Err(e) => {
                        if data != 0 {
                            resources.heap.free(resources.process_heap, data);
                        }
                        return Err(e);
                    }
                };
                self.allocated += len as u64;
                put(mem, a[2], h)?;
                Ok(0)
            }
            _ => Err(UNSUPPORTED),
        }
    }
}
