use super::*;

pub(super) fn rect(mem: &GuestMem, p: u32, b: &Pixels) -> Result<[u32; 4], u32> {
    let r = if p == 0 {
        [0, 0, b.width, b.height]
    } else {
        words::<4>(mem, p)?
    };
    if r[0] >= r[2] || r[1] >= r[3] || r[2] > b.width || r[3] > b.height {
        return Err(INVALID);
    }
    Ok(r)
}
pub(super) fn read(mem: &GuestMem, b: &Pixels) -> Result<Vec<u32>, u32> {
    let mut data = vec![0; b.len() as usize];
    mem.read_bytes(b.bits, &mut data).map_err(|_| INVALID)?;
    Ok(data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| u32::from_le_bytes(*s))
        .collect())
}
pub(super) fn write(mem: &GuestMem, b: &Pixels, pixels: &[u32]) -> Result<(), u32> {
    mem.write_bytes(
        b.bits,
        &pixels
            .iter()
            .flat_map(|p| p.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .map_err(|_| INVALID)
}
impl D3D9 {
    pub(super) fn resource(&mut self, m: &str, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        let (device, container, surface, data, texture) =
            match &self.objects.get(&a[0]).ok_or(INVALID)?.payload {
                Payload::Texture {
                    device,
                    surface,
                    data,
                } => (*device, 0, *surface, data.clone(), true),
                Payload::Surface {
                    device,
                    container,
                    data,
                } => (*device, *container, 0, data.clone(), false),
                _ => return Err(INVALID),
            };
        match m {
            "GetDevice" => self.output_ref(device, mem, a[1]),
            "GetContainer" => {
                put(mem, a[2], &[0])?;
                if container == 0 {
                    return Err(0x80004002);
                }
                let iid = words::<4>(mem, a[1])?;
                if iid != [0x85c31227, 0x4f003de5, 0x1af13a9b, 0xb5188c3c]
                    && iid != [0, 0, 0xc0, 0x46000000]
                {
                    return Err(0x80004002);
                }
                self.output_ref(container, mem, a[2])
            }
            "GetType" => Ok(if texture { 3 } else { 1 }),
            "SetPriority" => {
                let o = self.objects.get_mut(&a[0]).unwrap();
                let old = o.priority;
                o.priority = a[1];
                Ok(old)
            }
            "GetPriority" => Ok(self.objects[&a[0]].priority),
            "PreLoad" => Ok(0), // all supported resources are permanently CPU resident
            "GetLevelCount" => Ok(1),
            "SetLOD" | "GetLOD" => Ok(0), // one-level texture clamps every LOD to zero
            "GetDesc" => {
                desc(mem, a[1], &data.borrow())?;
                Ok(0)
            }
            "GetLevelDesc" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                desc(mem, a[2], &data.borrow())?;
                Ok(0)
            }
            "GetSurfaceLevel" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                self.output_ref(surface, mem, a[2])
            }
            "LockRect" => {
                let (out, rp, flags) = if texture {
                    if a[1] != 0 {
                        return Err(INVALID);
                    }
                    (a[2], a[3], a[4])
                } else {
                    (a[1], a[2], a[3])
                };
                if flags & !0xf810 != 0 {
                    return Err(UNAVAILABLE);
                }
                let mut b = data.borrow_mut();
                if b.locked {
                    return Err(INVALID);
                }
                let r = rect(mem, rp, &b)?;
                mem.ptr(b.bits, b.len(), flags & 0x10 == 0)
                    .map_err(|_| INVALID)?;
                put(mem, out, &[b.pitch(), b.bits + r[1] * b.pitch() + r[0] * 4])?;
                b.locked = true;
                Ok(0)
            }
            "UnlockRect" => {
                if texture && a[1] != 0 {
                    return Err(INVALID);
                }
                let mut b = data.borrow_mut();
                if !b.locked {
                    return Err(INVALID);
                }
                b.locked = false;
                Ok(0)
            }
            "AddDirtyRect" => {
                rect(mem, a[1], &data.borrow())?;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn surface(
        &mut self,
        device: u32,
        data: Buffer,
        refs: u32,
        container: u32,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        let mut leases = vec![device];
        if container != 0 {
            leases.push(container);
        }
        self.object(
            "IDirect3DSurface9",
            Payload::Surface {
                device,
                container,
                data,
            },
            refs,
            vec![],
            leases,
            mem,
            api,
            state,
        )
    }
    pub(super) fn fill(&self, mem: &GuestMem, h: u32, rp: u32, color: u32) -> Result<(), u32> {
        let data = self.buffer(h)?;
        let b = data.borrow();
        if b.locked {
            return Err(INVALID);
        }
        let r = rect(mem, rp, &b)?;
        mem.ptr(b.bits, b.len(), true).map_err(|_| INVALID)?;
        let color = if b.format == 22 {
            color | 0xff000000
        } else {
            color
        };
        let row: Vec<_> = (r[0]..r[2]).flat_map(|_| color.to_le_bytes()).collect();
        for y in r[1]..r[3] {
            mem.write_bytes(b.bits + y * b.pitch() + r[0] * 4, &row)
                .map_err(|_| INVALID)?;
        }
        Ok(())
    }
    pub(super) fn copy(
        &self,
        mem: &GuestMem,
        src: u32,
        sp: u32,
        dst: u32,
        dp: u32,
        stretch: bool,
    ) -> Result<(), u32> {
        let sb = self.buffer(src)?;
        let db = self.buffer(dst)?;
        let s = sb.borrow();
        let d = db.borrow();
        if s.locked || d.locked {
            return Err(INVALID);
        }
        let sr = rect(mem, sp, &s)?;
        let dr = if stretch {
            rect(mem, dp, &d)?
        } else {
            let xy = if dp == 0 {
                [0, 0]
            } else {
                words::<2>(mem, dp)?
            };
            [
                xy[0],
                xy[1],
                xy[0].checked_add(sr[2] - sr[0]).ok_or(INVALID)?,
                xy[1].checked_add(sr[3] - sr[1]).ok_or(INVALID)?,
            ]
        };
        if dr[2] > d.width || dr[3] > d.height {
            return Err(INVALID);
        }
        mem.ptr(d.bits, d.len(), true).map_err(|_| INVALID)?;
        let source = read(mem, &s)?; // snapshot also makes same-buffer copies safe
        for y in dr[1]..dr[3] {
            let sy = sr[1] + (y - dr[1]) * (sr[3] - sr[1]) / (dr[3] - dr[1]);
            let row: Vec<_> = (dr[0]..dr[2])
                .flat_map(|x| {
                    let sx = sr[0] + (x - dr[0]) * (sr[2] - sr[0]) / (dr[2] - dr[0]);
                    let n = source[(sy * s.width + sx) as usize];
                    (if s.format == 22 || d.format == 22 {
                        n | 0xff000000
                    } else {
                        n
                    })
                    .to_le_bytes()
                })
                .collect();
            mem.write_bytes(d.bits + y * d.pitch() + dr[0] * 4, &row)
                .map_err(|_| INVALID)?;
        }
        Ok(())
    }
}
