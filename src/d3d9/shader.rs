//! Independently implemented, bounded PS 2.0 arithmetic interpreter.
//! Public token layout: pinned LGPL Wine d3d9types.h. No shader compiler/JIT.
use super::*;
pub(super) const MAX_WORDS: usize = 8192;
pub(super) const MAX_OPS: usize = 96;
#[derive(Clone)]
pub(super) struct Program {
    pub bytes: Vec<u8>,
    pub ops: Vec<[u32; 8]>,
    pub definitions: BTreeMap<usize, [f32; 4]>,
    pub texture_mask: u8,
    pub uv1: bool,
}
fn kind(t: u32) -> u32 {
    ((t >> 28) & 7) | ((t >> 8) & 0x18)
}
fn number(t: u32) -> usize {
    (t & 0x7ff) as usize
}
fn source(t: u32) -> Result<(), String> {
    if t & 0x80000000 == 0 || t & 0x0000e000 != 0 {
        return Err("relative/reserved source addressing".into());
    }
    let legal = match kind(t) {
        0 => number(t) < 12,
        1 => number(t) == 0,
        2 => number(t) < 32,
        3 => number(t) < 2,
        _ => false,
    };
    if !legal || !matches!((t >> 24) & 15, 0 | 1 | 11 | 12) {
        return Err(format!("unsupported source {t:#010x}"));
    }
    Ok(())
}
fn destination(t: u32, definition: bool) -> Result<(), String> {
    let legal = if definition {
        kind(t) == 2 && number(t) < 32 && (t >> 16) & 15 == 15
    } else {
        (kind(t) == 0 && number(t) < 12) || (kind(t) == 8 && number(t) == 0)
    };
    if !legal || t & 0x80000000 == 0 || t & 0x0fc0e000 != 0 || t & 0xf0000 == 0 {
        return Err(format!("unsupported destination {t:#010x}"));
    }
    Ok(())
}
impl Program {
    pub fn read(mem: &GuestMem, p: u32) -> Result<Self, String> {
        Self::parse(|i| {
            let addr = p
                .checked_add((i * 4) as u32)
                .ok_or("shader address overflow")?;
            mem.read_u32(addr).map_err(|e| e.to_string())
        })
    }
    pub(super) fn parse(
        mut read: impl FnMut(usize) -> Result<u32, String>,
    ) -> Result<Self, String> {
        let mut tokens = Vec::new();
        let mut get = |i| -> Result<u32, String> {
            if i >= MAX_WORDS {
                return Err("shader exceeds 8192 DWORDs".into());
            }
            let t = read(i)?;
            tokens.push(t);
            Ok(t)
        };
        if get(0)? != 0xffff0200 {
            return Err("only PS 2.0 is implemented".into());
        }
        let mut program = Self {
            bytes: Vec::new(),
            ops: Vec::new(),
            definitions: BTreeMap::new(),
            texture_mask: 0,
            uv1: false,
        };
        let mut at = 1;
        let mut output = 0;
        loop {
            let t = get(at)?;
            at += 1;
            if t == 0xffff {
                if output != 15 {
                    return Err("oC0 must be fully written".into());
                }
                program.bytes = tokens.iter().flat_map(|v| v.to_le_bytes()).collect();
                return Ok(program);
            }
            let op = t & 0xffff;
            if op == 0xfffe {
                let count = ((t >> 16) & 0x7fff) as usize;
                for _ in 0..count {
                    get(at)?;
                    at += 1;
                }
                continue;
            }
            let len = ((t >> 24) & 15) as usize;
            if t & 0xf0000000 != 0
                || (t & 0x00ff0000 != 0 && !(op == 66 && t & 0x00ff0000 == 0x10000))
            {
                return Err(format!("unsupported instruction controls {t:#010x}"));
            }
            let expected = match op {
                0 => 0,
                1 | 6 | 7 | 14 | 15 | 19 | 35 | 36 => 2,
                2 | 3 | 5 | 8..=13 | 32 => 3,
                4 | 18 | 37 | 88 | 90 => 4,
                31 => 2,
                65 => 1,
                66 => 3,
                81 => 5,
                _ => return Err(format!("unsupported PS2 opcode {op}")),
            };
            if len != expected {
                return Err(format!("PS2 opcode {op} length {len}, expected {expected}"));
            }
            let mut words = [0; 8];
            words[0] = t;
            for i in 0..len {
                words[i + 1] = get(at)?;
                at += 1;
            }
            if op == 0 {
                continue;
            }
            if op == 31 {
                let reg = words[2];
                if reg & 0x80000000 == 0
                    || reg & 0xf0000 == 0
                    || reg & 0x0ff0e000 != 0
                    || (kind(reg) == 1 && number(reg) != 0)
                    || (kind(reg) != 1 && number(reg) > 1)
                    || !matches!(kind(reg), 1 | 3 | 10)
                    || (kind(reg) == 10 && words[1] != 0x90000000)
                    || (kind(reg) != 10 && words[1] != 0x80000000)
                {
                    return Err(format!(
                        "unsupported declaration {:#010x} {reg:#010x}",
                        words[1]
                    ));
                }
                continue;
            }
            if op == 81 {
                destination(words[1], true)?;
                let value = [words[2], words[3], words[4], words[5]].map(f32::from_bits);
                if value.iter().any(|v| !v.is_finite()) {
                    return Err("nonfinite def".into());
                }
                program.definitions.insert(number(words[1]), value);
                continue;
            }
            if op == 65 {
                if kind(words[1]) != 0
                    || number(words[1]) >= 12
                    || words[1] & 0x80000000 == 0
                    || words[1] & 0x0ff0e000 != 0
                {
                    return Err("invalid texkill register".into());
                }
            } else {
                destination(words[1], false)?;
                if kind(words[1]) == 8 {
                    output |= (words[1] >> 16) & 15;
                }
                source(words[2])?;
                if op == 66 {
                    if !matches!(words[3], 0xa0e40800 | 0xa0e40801) {
                        return Err(format!("unsupported sampler {:#010x}", words[3]));
                    }
                    program.texture_mask |= 1 << number(words[3]);
                } else {
                    for &operand in words.iter().take(len + 1).skip(3) {
                        source(operand)?;
                    }
                }
            }
            if words
                .iter()
                .take(len + 1)
                .skip(2)
                .any(|&t| kind(t) == 3 && number(t) == 1)
            {
                program.uv1 = true;
            }
            if program.ops.len() == MAX_OPS {
                return Err("PS2 exceeds 96 executable instructions".into());
            }
            program.ops.push(words);
        }
    }
    pub fn constants(&self, device: &[[f32; 4]; 32]) -> [[f32; 4]; 32] {
        let mut values = *device;
        for (&i, &v) in &self.definitions {
            values[i] = v;
        }
        values
    }
    // GPU payload: count, then 32 float4 constants, then fixed 8-DWORD ops.
    pub fn gpu_words(&self, constants: &[[f32; 4]; 32], extra: [u32; 8]) -> Vec<u32> {
        let mut v = vec![self.ops.len() as u32];
        v.extend(extra);
        v.extend(constants.iter().flatten().map(|n| n.to_bits()));
        v.extend(self.ops.iter().flatten());
        v
    }
    pub fn execute(
        &self,
        constants: &[[f32; 4]; 32],
        diffuse: [f32; 4],
        uv: [[f32; 4]; 2],
        mut sample: impl FnMut(usize, [f32; 4]) -> [f32; 4],
    ) -> Option<[f32; 4]> {
        let mut temp = [[0.; 4]; 12];
        let mut color = [0.; 4];
        for &op in &self.ops {
            if op[0] & 0xffff == 65 {
                if temp[number(op[1])][..3].iter().any(|&n| n < 0.) {
                    return None;
                }
                continue;
            }
            let src = |t| {
                let original = match kind(t) {
                    0 => temp[number(t)],
                    1 => diffuse,
                    2 => constants[number(t)],
                    _ => uv[number(t)],
                };
                std::array::from_fn::<_, 4, _>(|i| {
                    let n = original[((t >> (16 + i * 2)) & 3) as usize];
                    match (t >> 24) & 15 {
                        1 => -n,
                        11 => n.abs(),
                        12 => -n.abs(),
                        _ => n,
                    }
                })
            };
            let a = src(op[2]);
            let b = src(op[3]);
            let c = src(op[4]);
            let code = op[0] & 0xffff;
            let scalar = match code {
                6 => 1. / a[0],
                7 => 1. / a[0].abs().sqrt(),
                8 => (0..3).map(|i| a[i] * b[i]).sum(),
                9 => (0..4).map(|i| a[i] * b[i]).sum(),
                14 => a[0].exp2(),
                15 => a[0].abs().log2(),
                32 => a[0].abs().powf(b[0]),
                90 => a[0] * b[0] + a[1] * b[1] + c[0],
                _ => 0.,
            };
            let mut value = if code == 66 {
                let coord = if op[0] & 0x10000 != 0 {
                    a.map(|n| n / a[3])
                } else {
                    a
                };
                sample(number(op[3]), coord)
            } else if code == 36 {
                let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
                [a[0] / l, a[1] / l, a[2] / l, a[3]]
            } else if code == 37 {
                [a[0].cos(), a[0].sin(), 0., 0.]
            } else {
                std::array::from_fn(|i| match code {
                    1 => a[i],
                    2 => a[i] + b[i],
                    3 => a[i] - b[i],
                    4 => a[i] * b[i] + c[i],
                    5 => a[i] * b[i],
                    10 => a[i].min(b[i]),
                    11 => a[i].max(b[i]),
                    12 => (a[i] < b[i]) as u32 as f32,
                    13 => (a[i] >= b[i]) as u32 as f32,
                    18 => a[i] * b[i] + (1. - a[i]) * c[i],
                    19 => a[i] - a[i].floor(),
                    35 => a[i].abs(),
                    88 => {
                        if a[i] >= 0. {
                            b[i]
                        } else {
                            c[i]
                        }
                    }
                    _ => scalar,
                })
            };
            if op[1] & 0x100000 != 0 {
                value = value.map(|n| n.clamp(0., 1.));
            }
            let dst = if kind(op[1]) == 8 {
                &mut color
            } else {
                &mut temp[number(op[1])]
            };
            for i in 0..4 {
                if op[1] & (1 << (16 + i)) != 0 {
                    dst[i] = value[i];
                }
            }
        }
        Some(color)
    }
}
impl D3D9 {
    pub(super) fn create_pixel_shader(
        &mut self,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        put(mem, a[2], &[0])?;
        self.shader_error = None;
        self.dump_shader(a, mem);
        let program = Program::read(mem, a[1]).map_err(|e| {
            self.shader_error = Some(e);
            UNAVAILABLE
        })?;
        let h = self.object(
            "IDirect3DPixelShader9",
            Payload::PixelShader {
                device: a[0],
                program,
            },
            1,
            vec![],
            vec![a[0]],
            mem,
            api,
            state,
        )?;
        put(mem, a[2], &[h])?;
        Ok(0)
    }
    pub(super) fn dump_shader(&self, a: &Args, mem: &GuestMem) {
        if let Some(dir) = &self.shader_dump_dir {
            // Probe-only diagnostic snapshot, bounded even for rejected shaders.
            let snapshot = (|| -> Result<Vec<u8>, String> {
                let mut out = Vec::new();
                let mut at = 0;
                while at < MAX_WORDS {
                    let t = mem
                        .read_u32(a[1].checked_add((at * 4) as u32).ok_or("overflow")?)
                        .map_err(|e| e.to_string())?;
                    out.extend(t.to_le_bytes());
                    at += 1;
                    if at == 1 {
                        continue;
                    }
                    if t == 0xffff {
                        return Ok(out);
                    }
                    let n = if t & 0xffff == 0xfffe {
                        ((t >> 16) & 0x7fff) as usize
                    } else {
                        ((t >> 24) & 15) as usize
                    };
                    if at + n > MAX_WORDS {
                        return Err("too long".into());
                    }
                    for _ in 0..n {
                        out.extend(
                            mem.read_u32(a[1].checked_add((at * 4) as u32).ok_or("overflow")?)
                                .map_err(|e| e.to_string())?
                                .to_le_bytes(),
                        );
                        at += 1;
                    }
                }
                Err("missing END".into())
            })();
            if let Ok(bytes) = snapshot {
                use sha2::{Digest, Sha256};
                let name = format!("{:x}.psbin", Sha256::digest(&bytes));
                if std::fs::create_dir_all(dir).is_ok() {
                    let _ = std::fs::write(dir.join(name), bytes);
                }
            }
        }
    }
    pub(super) fn set_pixel_shader(
        &mut self,
        h: u32,
        shader: u32,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        if shader != 0 {
            if !matches!(&self.objects.get(&shader).ok_or(INVALID)?.payload, Payload::PixelShader { device,.. } if *device == h)
            {
                return Err(INVALID);
            }
            self.pin(shader)?;
        }
        let old = std::mem::replace(&mut self.device_mut(h)?.pixel_shader, shader);
        let o = self.objects.get_mut(&h).unwrap();
        if shader != 0 {
            o.children.push(shader);
        }
        if old != 0 {
            let i = o.children.iter().position(|&n| n == old).unwrap();
            o.children.remove(i);
            self.unpin(old, state);
        }
        Ok(0)
    }
    pub(super) fn shader_call(&mut self, m: &str, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        let (device, bytes) = match &self.objects.get(&a[0]).ok_or(INVALID)?.payload {
            Payload::PixelShader { device, program } => (device, &program.bytes),
            Payload::VertexShader { device, program } => (device, &program.bytes),
            _ => return Err(INVALID),
        };
        match m {
            "GetDevice" => {
                let device = *device;
                self.output_ref(device, mem, a[1])
            }
            "GetFunction" => {
                let size = bytes.len() as u32;
                mem.ptr(a[2], 4, true).map_err(|_| INVALID)?;
                if a[1] != 0 {
                    let capacity = mem.read_u32(a[2]).map_err(|_| INVALID)?;
                    if capacity < size {
                        put(mem, a[2], &[size])?;
                        return Err(0x88760867);
                    }
                    mem.write_bytes(a[1], bytes).map_err(|_| INVALID)?;
                }
                put(mem, a[2], &[size])?;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(t: &[u32]) -> Result<Program, String> {
        Program::parse(|i| t.get(i).copied().ok_or("truncated".into()))
    }
    #[test]
    fn ps2_sampling_swizzles_aliases_saturation_and_discard() {
        // dcl t0; dcl_2d s0; texld r0,t0,s0; mul_sat r0,r0,c0;
        // mov oC0,r0.bgra; temp destination aliases its source.
        let t = [
            0xffff0200, 0x0200001f, 0x80000000, 0xb00f0000, 0x0200001f, 0x90000000, 0xa00f0800,
            0x03000042, 0x800f0000, 0xb0e40000, 0xa0e40800, 0x03000005, 0x801f0000, 0x80e40000,
            0xa0e40000, 0x02000001, 0x800f0800, 0x80c60000, 0xffff,
        ];
        let p = parse(&t).unwrap();
        let mut c = [[0.; 4]; 32];
        c[0] = [2., -1., 0.5, 1.];
        let v = p
            .execute(&c, [0.; 4], [[0.2, 0.3, 0., 1.]; 2], |_, uv| {
                assert_eq!(uv, [0.2, 0.3, 0., 1.]);
                [0.75, 0.4, 0.5, 0.8]
            })
            .unwrap();
        assert_eq!(v, [0.25, 0., 1., 0.8]);
        let discard = [
            0xffff0200, 0x02000001, 0x800f0000, 0xa0e40000, 0x01000041, 0x800f0000, 0x02000001,
            0x800f0800, 0x80e40000, 0xffff,
        ];
        let p = parse(&discard).unwrap();
        c[0] = [-1., 0., 0., 1.];
        assert!(p
            .execute(&c, [0.; 4], [[0.; 4]; 2], |_, _| [0.; 4])
            .is_none());
        c[0] = [0., 0., 0., -1.];
        assert!(p
            .execute(&c, [0.; 4], [[0.; 4]; 2], |_, _| [0.; 4])
            .is_some()); // texkill checks XYZ
    }
    #[test]
    fn ps2_rejects_unbounded_malformed_and_unsupported_records() {
        for t in [
            vec![0xffff0200, 0x02000001, 0x800f0800],
            vec![0xffff0200, 0x03000001, 0x800f0800, 0xa0e40000, 0, 0xffff],
            vec![0xffff0200, 0x02000001, 0x800f0800, 0xa0e42000, 0xffff],
            vec![0xffff0200, 0x02000001, 0x800f0800, 0xa0e40020, 0xffff],
            vec![0xffff0300, 0xffff],
            vec![0xffff0200, 0xffff],
            vec![0xffff0200, 0x7ffffffe],
        ] {
            assert!(parse(&t).is_err());
        }
        assert!(Program::parse(|i| Ok(if i == 0 { 0xffff0200 } else { 0 })).is_err());
    }
}
