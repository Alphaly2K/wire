use super::{
    alloc, output, put, Backend, DSound, Format, Method, Object, SoundBuffer, ABI, INVALID,
    UNSUPPORTED,
};
impl DSound {
    fn create<B: Backend>(
        &mut self,
        kind: &'static str,
        buffer: Option<SoundBuffer>,
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        if self.objects.len() >= 256 {
            return Err(0x8007000e.into());
        }
        let table = if let Some(&p) = self.tables.get(kind) {
            p
        } else {
            mem.check_methods(kind)?;
            let mut bytes = Vec::new();
            for line in ABI.lines().filter(|l| l.starts_with(&format!("{kind}."))) {
                let method = Method::lookup(line.split('\t').next().unwrap()).unwrap();
                bytes.extend_from_slice(&mem.register(method)?.to_le_bytes());
            }
            let p = alloc(mem, bytes.len() as u32)?;
            mem.write_bytes(p, &bytes)?;
            self.tables.insert(kind, p);
            p
        };
        let p = alloc(mem, 4)?;
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
    fn release<B: Backend>(&mut self, handle: u32, mem: &mut B) -> Result<u32, B::Error> {
        let mut current = handle;
        loop {
            let o = &self.objects[&current];
            if o.refs != 1 {
                break;
            }
            mem.check_free(current)?;
            if let Some(b) = &o.buffer {
                if b.data != 0 {
                    mem.check_free(b.data)?;
                }
            }
            if o.owner == 0 {
                break;
            }
            current = o.owner;
        }
        self.release_checked(handle, mem)
    }
    fn release_checked<B: Backend>(&mut self, handle: u32, mem: &mut B) -> Result<u32, B::Error> {
        let o = &self.objects[&handle];
        let refs = o.refs - 1;
        if refs != 0 {
            self.objects.get_mut(&handle).unwrap().refs = refs;
            return Ok(refs);
        }
        let data = o.buffer.as_ref().map_or(0, |b| b.data);
        if data != 0 {
            mem.free(data)?;
        }
        mem.free(handle)?;
        let o = self.objects.remove(&handle).unwrap();
        if let Some(b) = o.buffer {
            if b.data != 0 {
                self.allocated -= u64::from(b.len);
            }
        }
        if o.owner != 0 {
            self.release_checked(o.owner, mem)?;
        }
        Ok(0)
    }
    fn query_interface<B: Backend>(
        &mut self,
        kind: &str,
        a: &[u32; 16],
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        let o = self.objects.get_mut(&a[0]).unwrap();
        let mut guid = [0; 16];
        mem.read_bytes(a[1], &mut guid)?;
        if kind == "IDirectSoundBuffer"
            && guid
                == [
                    0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9, 0x25,
                    0xcd, 0x16,
                ]
        {
            if o.buffer.as_ref().unwrap().flags & 0x100 == 0 {
                return Err(0x80004002.into());
            }
            if let Some((&h, _)) = self
                .objects
                .iter()
                .find(|(_, o)| o.owner == a[0] && o.kind == "IDirectSoundNotify")
            {
                let notify = self.objects.get_mut(&h).unwrap();
                notify.refs = notify
                    .refs
                    .checked_add(1)
                    .ok_or_else(|| B::Error::from(0x8007000e))?;
                return Ok(h);
            }
            let refs = self.objects[&a[0]]
                .refs
                .checked_add(1)
                .ok_or_else(|| B::Error::from(0x8007000e))?;
            let h = self.create("IDirectSoundNotify", None, mem)?;
            self.objects.get_mut(&h).unwrap().owner = a[0];
            self.objects.get_mut(&a[0]).unwrap().refs = refs;
            return Ok(h);
        }
        if kind == "IDirectSoundNotify"
            && (guid == [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
                || guid
                    == [
                        0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf,
                        0xb, 0xe5, 0x60,
                    ])
        {
            let owner = o.owner;
            let owner_object = self.objects.get_mut(&owner).unwrap();
            owner_object.refs = owner_object
                .refs
                .checked_add(1)
                .ok_or_else(|| B::Error::from(0x8007000e))?;
            return Ok(owner);
        }
        let expected = if kind == "IDirectSoundBuffer" {
            [
                0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf, 0xb,
                0xe5, 0x60,
            ]
        } else if kind == "IDirectSoundNotify" {
            [
                0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd,
                0x16,
            ]
        } else if kind == "IDirectSound8" {
            [
                0x93, 0x7e, 0x0a, 0xc5, 0x95, 0xf3, 0x34, 0x48, 0x9e, 0xf6, 0x7f, 0xa9, 0x9d, 0xe5,
                9, 0x66,
            ]
        } else {
            [
                0x83, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf, 0xb,
                0xe5, 0x60,
            ]
        };
        if guid != expected && guid != [0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46] {
            return Err(0x80004002.into());
        }
        o.refs = o
            .refs
            .checked_add(1)
            .ok_or_else(|| B::Error::from(0x8007000e))?;
        Ok(a[0])
    }
    fn create_buffer<B: Backend>(&mut self, a: &[u32; 16], mem: &mut B) -> Result<u32, B::Error> {
        if a[3] != 0 {
            return Err(0x80040110.into());
        }
        let size = mem.read_u32(a[1])?;
        if !matches!(size, 20 | 36) {
            return Err(INVALID.into());
        }
        mem.check(a[1], size, false)?;
        let flags = mem.read_u32(a[1] + 4)?;
        let len = mem.read_u32(a[1] + 8)?;
        let fp = mem.read_u32(a[1] + 16)?;
        if flags & !0x1c1eb != 0 {
            return Err(UNSUPPORTED.into());
        } // no hardware, 3D or FX
        let primary = flags & 1 != 0;
        let format = if primary {
            if len != 0 || fp != 0 {
                return Err(INVALID.into());
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
            return Err(INVALID.into());
        }
        if len > 16 * 1024 * 1024 || self.allocated + len as u64 > 64 * 1024 * 1024 {
            return Err(0x8007000e.into());
        }
        let data = if primary { 0 } else { alloc(mem, len)? };
        if format.bits == 8 && data != 0 {
            mem.write_bytes(data, &vec![128; len as usize])?;
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
        );
        let h = match result {
            Ok(h) => h,
            Err(e) => {
                if data != 0 {
                    mem.free(data)?;
                }
                return Err(e);
            }
        };
        self.allocated += len as u64;
        Ok(h)
    }
    pub(crate) fn call<B: Backend>(
        &mut self,
        name: &str,
        a: &[u32; 16],
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        if matches!(name, "DirectSoundCreate" | "DirectSoundCreate8") {
            mem.check(a[1], 4, true)?;
            let result = (|| {
                if a[2] != 0 {
                    return Err(0x80040110.into());
                }
                if a[0] != 0 {
                    let mut guid = [0; 16];
                    mem.read_bytes(a[0], &mut guid)?;
                    if guid != [0; 16] {
                        return Err(0x88780078.into());
                    }
                }
                if !mem.device() {
                    return Err(0x88780078.into());
                }
                self.create(
                    if name.ends_with('8') {
                        "IDirectSound8"
                    } else {
                        "IDirectSound"
                    },
                    None,
                    mem,
                )
            })();
            return output(mem, a[1], result);
        }
        let (kind, m) = name
            .split_once('.')
            .ok_or_else(|| B::Error::from(UNSUPPORTED))?;
        let o = self
            .objects
            .get_mut(&a[0])
            .ok_or_else(|| B::Error::from(INVALID))?;
        if o.kind != kind {
            return Err(INVALID.into());
        }
        match m {
            "QueryInterface" => {
                mem.check(a[2], 4, true)?;
                let result = self.query_interface(kind, a, mem);
                return output(mem, a[2], result);
            }
            "AddRef" => {
                o.refs = o
                    .refs
                    .checked_add(1)
                    .ok_or_else(|| B::Error::from(0x8007000e))?;
                return Ok(o.refs);
            }
            "Release" => {
                return self.release(a[0], mem);
            }
            _ => {}
        }
        if kind == "IDirectSoundNotify" {
            if m != "SetNotificationPositions" {
                return Err(UNSUPPORTED.into());
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
                return Err(INVALID.into());
            }
            let mut positions = Vec::new();
            for i in 0..a[1] {
                let p = a[2]
                    .checked_add(i * 8)
                    .ok_or_else(|| B::Error::from(INVALID))?;
                mem.check(p, 8, false)?;
                let offset = mem.read_u32(p)?;
                let event = mem.read_u32(p + 4)?;
                if (offset != u32::MAX && offset >= b.len) || !mem.event(event) {
                    return Err(INVALID.into());
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
                if !mem.window(a[1]) || !(1..=4).contains(&a[2]) {
                    Err(INVALID.into())
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
                    Err(UNSUPPORTED.into())
                }
            }
            "Compact" => Ok(0),
            "GetCaps" => {
                if mem.read_u32(a[1])? != 96 {
                    return Err(INVALID.into());
                }
                let mut b = [0u8; 96];
                for (i, n) in [(0, 96u32), (1, 0xf3f), (2, 100), (3, 200000), (4, 1)] {
                    b[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
                }
                mem.write_bytes(a[1], &b)?;
                Ok(0)
            }
            "CreateSoundBuffer" => {
                mem.check(a[2], 4, true)?;
                let result = self.create_buffer(a, mem);
                output(mem, a[2], result)
            }
            _ => Err(UNSUPPORTED.into()),
        }
    }
}
