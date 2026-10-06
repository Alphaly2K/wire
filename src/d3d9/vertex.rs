//! Checked guest vertex storage and stream-zero fixed-function draws.
use super::*;
#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub position: u32,
    pub transformed: bool,
    pub color: Option<u32>,
    pub uv: u32,
    pub size: u32,
}
impl D3D9 {
    pub(super) fn layout(&self, d: &Device) -> Result<Layout, u32> {
        if d.declaration != 0 {
            return match &self.objects.get(&d.declaration).ok_or(INVALID)?.payload {
                Payload::Declaration { layout, .. } => Ok(*layout),
                _ => Err(INVALID),
            };
        }
        if !matches!(d.fvf, 0x102 | 0x142 | 0x104 | 0x144) {
            return Err(UNAVAILABLE);
        }
        let transformed = d.fvf & 0xe == 4;
        let color = d.fvf & 0x40 != 0;
        let position_size = if transformed { 16 } else { 12 };
        let uv = position_size + if color { 4 } else { 0 };
        Ok(Layout {
            position: 0,
            transformed,
            color: color.then_some(position_size),
            uv,
            size: uv + 8,
        })
    }
    pub(super) fn create_declaration(
        &mut self,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        put(mem, a[2], &[0])?;
        let mut elements = Vec::new();
        let (mut position, mut color, mut uv) = (None, None, None);
        let mut size = 0;
        for i in 0..=64 {
            let p = a[1].checked_add(i * 8).ok_or(INVALID)?;
            let mut bytes = [0; 8];
            mem.read_bytes(p, &mut bytes).map_err(|_| INVALID)?;
            elements.extend(bytes);
            let stream = u16::from_le_bytes(bytes[..2].try_into().unwrap());
            if stream == 255 && bytes[4] == 17 {
                let (position, transformed) = position.ok_or(INVALID)?;
                let layout = Layout {
                    position,
                    transformed,
                    color,
                    uv: uv.ok_or(INVALID)?,
                    size,
                };
                let h = self.object(
                    "IDirect3DVertexDeclaration9",
                    Payload::Declaration {
                        device: a[0],
                        elements,
                        layout,
                    },
                    1,
                    vec![],
                    vec![a[0]],
                    mem,
                    api,
                    state,
                )?;
                put(mem, a[2], &[h])?;
                return Ok(0);
            }
            if i == 64 || stream != 0 || bytes[5] != 0 || bytes[7] != 0 {
                return Err(UNAVAILABLE);
            }
            let offset = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as u32;
            let width = match (bytes[4], bytes[6]) {
                (2, 0) if position.is_none() => {
                    position = Some((offset, false));
                    12
                }
                (3, 9) if position.is_none() => {
                    position = Some((offset, true));
                    16
                }
                (4, 10) if color.is_none() => {
                    color = Some(offset);
                    4
                }
                (1, 5) if uv.is_none() => {
                    uv = Some(offset);
                    8
                }
                _ => return Err(UNAVAILABLE),
            };
            if !offset.is_multiple_of(4) || offset + width > 4096 {
                return Err(INVALID);
            }
            size = size.max(offset + width);
        }
        Err(INVALID)
    }
    pub(super) fn set_declaration(
        &mut self,
        h: u32,
        decl: u32,
        state: &mut ApiState,
    ) -> Result<(), u32> {
        if decl != 0 {
            if !matches!(&self.objects.get(&decl).ok_or(INVALID)?.payload,Payload::Declaration {device,..} if *device==h)
            {
                return Err(INVALID);
            }
            self.pin(decl)?;
        }
        let old = std::mem::replace(&mut self.device_mut(h)?.declaration, decl);
        let o = self.objects.get_mut(&h).unwrap();
        if decl != 0 {
            o.children.push(decl);
        }
        if old != 0 {
            let i = o.children.iter().position(|&p| p == old).unwrap();
            o.children.remove(i);
            self.unpin(old, state);
        }
        Ok(())
    }
    pub(super) fn declaration_call(
        &mut self,
        m: &str,
        a: &Args,
        mem: &GuestMem,
    ) -> Result<u32, u32> {
        let Payload::Declaration {
            device, elements, ..
        } = &self.objects.get(&a[0]).ok_or(INVALID)?.payload
        else {
            return Err(INVALID);
        };
        match m {
            "GetDevice" => {
                let h = *device;
                self.output_ref(h, mem, a[1])
            }
            "GetDeclaration" => {
                let count = elements.len() as u32 / 8;
                if a[1] == 0 {
                    put(mem, a[2], &[count])?;
                    return Ok(0);
                }
                let capacity = mem.read_u32(a[2]).map_err(|_| INVALID)?;
                mem.ptr(a[2], 4, true).map_err(|_| INVALID)?;
                if capacity < count {
                    put(mem, a[2], &[count])?;
                    return Err(0x88760867);
                }
                mem.write_bytes(a[1], elements).map_err(|_| INVALID)?;
                put(mem, a[2], &[count])?;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
    pub(super) fn create_vertices(
        &mut self,
        index: bool,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        put(mem, a[5], &[0])?;
        if a[1] == 0 {
            return Err(INVALID);
        }
        if a[2] & !0x208 != 0 || a[4] > 2 || a[6] != 0 {
            return Err(UNAVAILABLE);
        }
        if index && !matches!(a[3], 101 | 102) {
            return Err(UNAVAILABLE);
        }
        if a[1] > 64 * 1024 * 1024 || self.allocated + a[1] as u64 > 256 * 1024 * 1024 {
            return Err(NOMEM);
        }
        let bits = alloc(mem, state, a[1])?;
        let result = self.object(
            if index {
                "IDirect3DIndexBuffer9"
            } else {
                "IDirect3DVertexBuffer9"
            },
            Payload::Vertices {
                device: a[0],
                bits,
                len: a[1],
                usage: a[2],
                pool: a[4],
                fvf: a[3],
                locked: false,
            },
            1,
            vec![],
            vec![a[0]],
            mem,
            api,
            state,
        );
        let buffer = match result {
            Ok(h) => h,
            Err(e) => {
                state.heap.free(state.process_heap, bits);
                return Err(e);
            }
        };
        self.allocated += a[1] as u64;
        put(mem, a[5], &[buffer])?;
        Ok(0)
    }
    pub(super) fn vertex_call(&mut self, m: &str, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        let o = self.objects.get_mut(&a[0]).ok_or(INVALID)?;
        let index = o.kind == "IDirect3DIndexBuffer9";
        let Payload::Vertices {
            device,
            bits,
            len,
            usage,
            pool,
            fvf,
            locked,
        } = &mut o.payload
        else {
            return Err(INVALID);
        };
        match m {
            "GetDevice" => {
                let h = *device;
                self.output_ref(h, mem, a[1])
            }
            "GetType" => Ok(if index { 7 } else { 6 }),
            "SetPriority" => Ok(std::mem::replace(&mut o.priority, a[1])),
            "GetPriority" => Ok(o.priority),
            "PreLoad" => Ok(0),
            "GetDesc" => {
                if index {
                    put(mem, a[1], &[*fvf, 7, *usage, *pool, *len])?;
                } else {
                    put(mem, a[1], &[100, 6, *usage, *pool, *len, *fvf])?;
                }
                Ok(0)
            }
            "Lock" => {
                if *locked || a[1] > *len {
                    return Err(INVALID);
                }
                let size = if a[2] == 0 { *len - a[1] } else { a[2] };
                if size > *len - a[1] {
                    return Err(INVALID);
                }
                // Managed CPU storage does not need DISCARD/NOOVERWRITE
                // renaming; these hints leave its single checked copy usable.
                if a[4] & !0x3010 != 0
                    || (a[4] & 0x3000 != 0 && *usage & 0x200 == 0 && *pool != 1)
                    || (a[4] & 0x10 != 0 && *usage & 8 != 0)
                {
                    return Err(INVALID);
                }
                mem.ptr(*bits + a[1], size, a[4] & 0x10 == 0)
                    .map_err(|_| INVALID)?;
                put(mem, a[3], &[*bits + a[1]])?;
                *locked = true;
                Ok(0)
            }
            "Unlock" => {
                if !*locked {
                    return Err(INVALID);
                }
                *locked = false;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
    pub(super) fn set_stream(&mut self, a: &Args, state: &mut ApiState) -> Result<u32, u32> {
        if a[1] != 0 {
            return Err(UNAVAILABLE);
        }
        if a[2] != 0 {
            if self.objects.get(&a[2]).ok_or(INVALID)?.kind != "IDirect3DVertexBuffer9" {
                return Err(INVALID);
            }
            match &self.objects.get(&a[2]).ok_or(INVALID)?.payload {
                Payload::Vertices { device, len, .. }
                    if *device == a[0] && a[3] < *len && a[4] <= 4096 => {}
                _ => return Err(INVALID),
            }
            self.pin(a[2])?;
        }
        let old = std::mem::replace(&mut self.device_mut(a[0])?.stream, [a[2], a[3], a[4]])[0];
        let o = self.objects.get_mut(&a[0]).unwrap();
        if a[2] != 0 {
            o.children.push(a[2]);
        }
        if old != 0 {
            let i = o.children.iter().position(|&p| p == old).unwrap();
            o.children.remove(i);
            self.unpin(old, state);
        }
        Ok(0)
    }
    pub(super) fn set_indices(&mut self, a: &Args, state: &mut ApiState) -> Result<u32, u32> {
        if a[1] != 0 {
            let o = self.objects.get(&a[1]).ok_or(INVALID)?;
            if o.kind != "IDirect3DIndexBuffer9"
                || !matches!(o.payload,Payload::Vertices {device,..} if device==a[0])
            {
                return Err(INVALID);
            }
            self.pin(a[1])?;
        }
        let old = std::mem::replace(&mut self.device_mut(a[0])?.indices, a[1]);
        let o = self.objects.get_mut(&a[0]).unwrap();
        if a[1] != 0 {
            o.children.push(a[1]);
        }
        if old != 0 {
            let i = o.children.iter().position(|&p| p == old).unwrap();
            o.children.remove(i);
            self.unpin(old, state);
        }
        Ok(0)
    }
    pub(super) fn draw_indexed(&mut self, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        if a[6] > 65536 {
            return Err(UNAVAILABLE);
        }
        let count = match a[1] {
            4 => a[6] * 3,
            5 | 6 => a[6] + 2,
            _ => return Err(UNAVAILABLE),
        };
        let d = self.device(a[0])?;
        let (bits, len, format) = match &self.objects.get(&d.indices).ok_or(INVALID)?.payload {
            Payload::Vertices {
                bits,
                len,
                fvf,
                locked: false,
                ..
            } => (*bits, *len, *fvf),
            _ => return Err(INVALID),
        };
        let width = if format == 101 { 2 } else { 4 };
        let start = a[5].checked_mul(width).ok_or(INVALID)?;
        let end = start.checked_add(count * width).ok_or(INVALID)?;
        if end > len {
            return Err(INVALID);
        }
        let [vb, offset, stride] = d.stream;
        let (vertices, vertex_len) = match &self.objects.get(&vb).ok_or(INVALID)?.payload {
            Payload::Vertices {
                bits,
                len,
                locked: false,
                ..
            } => (*bits, *len),
            _ => return Err(INVALID),
        };
        let limit = a[3].checked_add(a[4]).ok_or(INVALID)?;
        let vertex_size = self.layout(d)?.size;
        let mut indices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let p = bits + start + i * width;
            let raw = if width == 2 {
                mem.read_u16(p).map_err(|_| INVALID)? as u32
            } else {
                mem.read_u32(p).map_err(|_| INVALID)?
            };
            if raw < a[3] || raw >= limit {
                return Err(INVALID);
            }
            let index = raw.checked_add_signed(a[2] as i32).ok_or(INVALID)?;
            let end = index
                .checked_mul(stride)
                .and_then(|n| n.checked_add(offset))
                .and_then(|n| n.checked_add(vertex_size))
                .ok_or(INVALID)?;
            if end > vertex_len {
                return Err(INVALID);
            }
            indices.push(index);
        }
        let mut up = [0; 16];
        up[..5].copy_from_slice(&[a[0], a[1], a[6], vertices + offset, stride]);
        self.draw_selected(a[0], &up, mem, Some(&indices))
    }
    pub(super) fn draw_stream(&mut self, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        let [h, offset, stride] = self.device(a[0])?.stream;
        let (bits, len) = match &self.objects.get(&h).ok_or(INVALID)?.payload {
            Payload::Vertices {
                bits,
                len,
                locked: false,
                ..
            } => (*bits, *len),
            _ => return Err(INVALID),
        };
        let count = match a[1] {
            4 => a[3].checked_mul(3),
            5 | 6 => a[3].checked_add(2),
            _ => return Err(UNAVAILABLE),
        }
        .ok_or(INVALID)?;
        let start = a[2]
            .checked_mul(stride)
            .and_then(|n| n.checked_add(offset))
            .ok_or(INVALID)?;
        let vertex_size = self.layout(self.device(a[0])?)?.size;
        let end = if a[3] == 0 {
            start
        } else {
            count
                .saturating_sub(1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(start))
                .and_then(|n| n.checked_add(vertex_size))
                .ok_or(INVALID)?
        };
        if end > len {
            return Err(INVALID);
        }
        let mut up = [0; 16];
        up[..5].copy_from_slice(&[a[0], a[1], a[3], bits + start, stride]);
        self.draw(a[0], &up, mem)
    }
}
