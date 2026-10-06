//! Bounded VS2 MOV/DP4 execution for the verified Siglus vertex programs.
//! Executed once per vertex before clipping; pixels remain on Metal.
use super::*;
pub(super) struct VertexProgram {
    pub bytes: Vec<u8>,
    ops: Vec<[u32; 4]>,
    inputs: BTreeMap<usize, (u32, u32)>, // vreg -> public declaration usage/index
}
fn kind(t: u32) -> u32 {
    ((t >> 28) & 7) | ((t >> 8) & 24)
}
fn number(t: u32) -> usize {
    (t & 2047) as usize
}
pub(super) struct Output {
    pub position: [f32; 4],
    pub color: [f32; 4],
    pub uv: [[f32; 2]; 2],
}
impl VertexProgram {
    pub fn read(mem: &GuestMem, p: u32) -> Result<Self, String> {
        let mut bytes = Vec::new();
        let mut at = 0;
        let mut get = || -> Result<u32, String> {
            if at >= shader::MAX_WORDS {
                return Err("VS2 exceeds 8192 words".into());
            }
            let t = mem
                .read_u32(
                    p.checked_add((at * 4) as u32)
                        .ok_or("VS2 pointer overflow")?,
                )
                .map_err(|e| e.to_string())?;
            bytes.extend(t.to_le_bytes());
            at += 1;
            Ok(t)
        };
        if get()? != 0xfffe0200 {
            return Err("only VS2 is implemented".into());
        }
        let mut ops = Vec::new();
        let mut inputs = BTreeMap::new();
        let mut output = 0;
        let mut color_mask = 0;
        loop {
            let t = get()?;
            if t == 0xffff {
                break;
            }
            let code = t & 65535;
            if code == 65534 {
                for _ in 0..((t >> 16) & 32767) {
                    get()?;
                }
                continue;
            }
            if t & 0xf0ff0000 != 0 {
                return Err("VS2 instruction controls".into());
            }
            let len = (t >> 24) & 15;
            let expected = match code {
                31 | 1 => 2,
                9 => 3,
                _ => return Err(format!("unsupported VS2 opcode {code}")),
            };
            if len != expected {
                return Err("VS2 instruction length".into());
            }
            let mut op = [0; 4];
            op[0] = t;
            for i in 0..len as usize {
                op[i + 1] = get()?;
            }
            if code == 31 {
                let (usage, index) = (op[1] & 15, (op[1] >> 16) & 15);
                let d = op[2];
                if op[1] & !0x800f000f != 0
                    || op[1] & 0x80000000 == 0
                    || kind(d) != 1
                    || number(d) >= 16
                    || d & 0x8fff0000 != 0x800f0000
                    || d & 0xe000 != 0
                    || !matches!((usage, index), (0, 0) | (5, 0) | (5, 1) | (10, 0))
                    || inputs.insert(number(d), (usage, index)).is_some()
                {
                    return Err("unsupported VS2 declaration".into());
                }
                continue;
            }
            let d = op[1];
            let legal = match kind(d) {
                0 => number(d) < 12,
                4 => number(d) == 0,
                5 => number(d) == 0,
                6 => number(d) < 2,
                _ => false,
            };
            if !legal || d & 0x80000000 == 0 || d & 0x0fc0e000 != 0 || d & 0xf0000 == 0 {
                return Err("unsupported VS2 destination".into());
            }
            for &s in op.iter().take(len as usize + 1).skip(2) {
                let legal = match kind(s) {
                    0 => number(s) < 12,
                    1 => inputs.contains_key(&number(s)),
                    2 => number(s) < 256,
                    _ => false,
                };
                if !legal
                    || s & 0x80000000 == 0
                    || s & 0xe000 != 0
                    || !matches!((s >> 24) & 15, 0 | 1 | 11 | 12)
                {
                    return Err("unsupported VS2 source".into());
                }
            }
            if kind(d) == 4 {
                output |= (d >> 16) & 15;
            }
            if kind(d) == 5 {
                color_mask |= (d >> 16) & 15;
            }
            if ops.len() >= 256 {
                return Err("VS2 exceeds 256 ops".into());
            }
            ops.push(op);
        }
        if output != 15 || color_mask != 15 {
            return Err("VS2 must write oPos and oD0".into());
        }
        Ok(Self { bytes, ops, inputs })
    }
    pub fn execute(
        &self,
        constants: &[[f32; 4]; 256],
        position: [f32; 4],
        color: [f32; 4],
        uv: [[f32; 2]; 2],
    ) -> Result<Output, u32> {
        let mut temp = [[0.; 4]; 12];
        let mut output = Output {
            position: [0.; 4],
            color: [0.; 4],
            uv: [[0.; 2]; 2],
        };
        for op in &self.ops {
            let source = |t: u32| {
                let v = match kind(t) {
                    0 => temp[number(t)],
                    2 => constants[number(t)],
                    _ => match self.inputs[&number(t)] {
                        (0, _) => position,
                        (10, _) => color,
                        (_, i) => [uv[i as usize][0], uv[i as usize][1], 0., 1.],
                    },
                };
                std::array::from_fn::<_, 4, _>(|i| {
                    let n = v[((t >> (16 + i * 2)) & 3) as usize];
                    match (t >> 24) & 15 {
                        1 => -n,
                        11 => n.abs(),
                        12 => -n.abs(),
                        _ => n,
                    }
                })
            };
            let a = source(op[2]);
            let b = if op[0] & 65535 == 9 {
                source(op[3])
            } else {
                [0.; 4]
            };
            let mut v = if op[0] & 65535 == 9 {
                [(0..4).map(|i| a[i] * b[i]).sum(); 4]
            } else {
                a
            };
            if op[1] & 0x100000 != 0 {
                v = v.map(|v| v.clamp(0., 1.));
            }
            for (i, &value) in v.iter().enumerate() {
                if op[1] & (1 << (16 + i)) == 0 {
                    continue;
                }
                match kind(op[1]) {
                    0 => temp[number(op[1])][i] = value,
                    4 => output.position[i] = value,
                    5 => output.color[i] = value,
                    _ => {
                        if i >= 2 && value != if i == 3 { 1. } else { 0. } {
                            return Err(UNAVAILABLE);
                        }
                        if i < 2 {
                            output.uv[number(op[1])][i] = value;
                        }
                    }
                }
            }
        }
        if output
            .position
            .iter()
            .chain(&output.color)
            .chain(output.uv.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return Err(INVALID);
        }
        Ok(output)
    }
}
impl D3D9 {
    pub(super) fn create_vertex_shader(
        &mut self,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        put(mem, a[2], &[0])?;
        self.dump_shader(a, mem);
        self.shader_error = None;
        let program = VertexProgram::read(mem, a[1]).map_err(|e| {
            self.shader_error = Some(e);
            UNAVAILABLE
        })?;
        let h = self.object(
            "IDirect3DVertexShader9",
            Payload::VertexShader {
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
    pub(super) fn set_vertex_shader(
        &mut self,
        h: u32,
        shader: u32,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        if shader != 0 {
            if !matches!(&self.objects.get(&shader).ok_or(INVALID)?.payload,Payload::VertexShader {device,..} if *device==h)
            {
                return Err(INVALID);
            }
            self.pin(shader)?;
        }
        let old = std::mem::replace(&mut self.device_mut(h)?.vertex_shader, shader);
        let d = self.objects.get_mut(&h).unwrap();
        if shader != 0 {
            d.children.push(shader);
        }
        if old != 0 {
            let i = d.children.iter().position(|&n| n == old).unwrap();
            d.children.remove(i);
            self.unpin(old, state);
        }
        Ok(0)
    }
}
