//! Bounded D3D9 state recording. Setters validate against a private snapshot;
//! getters/draws keep using live state. Recorded resources own balanced pins.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Key {
    Render(u32),
    Sampler(u32),
    Stage(u32),
    Transform(u32),
    VertexConstant(usize),
    PixelConstant(usize),
    Viewport,
    Scissor,
    Texture(u32),
    VertexFormat,
    VertexShader,
    PixelShader,
    Stream,
    Indices,
}
pub(super) struct Recording {
    snapshot: Box<Device>,
    pub children: Vec<u32>,
    keys: BTreeSet<Key>,
}
fn setter(m: &str) -> bool {
    matches!(
        m,
        "SetRenderState"
            | "SetSamplerState"
            | "SetTextureStageState"
            | "SetTransform"
            | "MultiplyTransform"
            | "SetVertexShaderConstantF"
            | "SetPixelShaderConstantF"
            | "SetViewport"
            | "SetScissorRect"
            | "SetTexture"
            | "SetFVF"
            | "SetVertexDeclaration"
            | "SetVertexShader"
            | "SetPixelShader"
            | "SetStreamSource"
            | "SetIndices"
    )
}
fn keys(m: &str, a: &Args) -> Vec<Key> {
    match m {
        "SetRenderState" => vec![Key::Render(a[1])],
        "SetSamplerState" => vec![Key::Sampler(a[1] * 16 + a[2])],
        "SetTextureStageState" => vec![Key::Stage(a[1] * 32 + a[2])],
        "SetTransform" | "MultiplyTransform" => vec![Key::Transform(a[1])],
        "SetVertexShaderConstantF" => (a[1]..a[1] + a[3])
            .map(|i| Key::VertexConstant(i as usize))
            .collect(),
        "SetPixelShaderConstantF" => (a[1]..a[1] + a[3])
            .map(|i| Key::PixelConstant(i as usize))
            .collect(),
        "SetViewport" => vec![Key::Viewport],
        "SetScissorRect" => vec![Key::Scissor],
        "SetTexture" => vec![Key::Texture(a[1])],
        "SetFVF" | "SetVertexDeclaration" => vec![Key::VertexFormat],
        "SetVertexShader" => vec![Key::VertexShader],
        "SetPixelShader" => vec![Key::PixelShader],
        "SetStreamSource" => vec![Key::Stream],
        "SetIndices" => vec![Key::Indices],
        _ => Vec::new(),
    }
}
fn resources(d: &Device, keys: &BTreeSet<Key>) -> Vec<u32> {
    keys.iter()
        .filter_map(|k| match k {
            Key::Texture(0) => Some(d.texture),
            Key::Texture(1) => Some(d.texture1),
            Key::VertexFormat => Some(d.declaration),
            Key::VertexShader => Some(d.vertex_shader),
            Key::PixelShader => Some(d.pixel_shader),
            Key::Stream => Some(d.stream[0]),
            Key::Indices => Some(d.indices),
            _ => None,
        })
        .filter(|&h| h != 0)
        .collect()
}
fn capture(d: &Device, keys: &BTreeSet<Key>) -> Result<Box<Device>, u32> {
    let mut snapshot = Box::new(d.clone());
    // Store resolved defaults: ZENABLE's initial value depends on the initial
    // depth surface and must not change when a block is applied to another target.
    for k in keys {
        match *k {
            Key::Render(n) => {
                snapshot.render.insert(n, raster::render(d, n)?);
            }
            Key::Sampler(n) => {
                snapshot
                    .sampler
                    .insert(n, raster::sampler_slot(d, n / 16, n % 16)?);
            }
            Key::Stage(n) => {
                snapshot.stage.insert(n, raster::stage(d, n / 32, n % 32)?);
            }
            _ => {}
        }
    }
    Ok(snapshot)
}
impl<G: RasterBackend> D3D9<G> {
    fn check_pins(&self, handles: &[u32]) -> Result<(), u32> {
        let mut counts = BTreeMap::<u32, u32>::new();
        for &h in handles {
            *counts.entry(h).or_default() += 1;
        }
        for (h, count) in counts {
            self.objects
                .get(&h)
                .ok_or(INVALID)?
                .pins
                .checked_add(count)
                .ok_or(NOMEM)?;
        }
        Ok(())
    }
    pub(super) fn begin_stateblock(&mut self, h: u32) -> Result<u32, u32> {
        if self.recording.contains_key(&h) {
            return Err(INVALID);
        }
        let snapshot = Box::new(self.device(h)?.clone());
        let children = self.objects[&h].children.clone();
        self.check_pins(&children)?;
        for &p in &children {
            self.pin(p)?;
        }
        self.recording.insert(
            h,
            Recording {
                snapshot,
                children,
                keys: BTreeSet::new(),
            },
        );
        Ok(0)
    }
    pub(super) fn recorded_device_call<B: Backend>(
        &mut self,
        m: &str,
        a: &Args,
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        if !setter(m) || !self.recording.contains_key(&a[0]) {
            return self.device_call(m, a, mem);
        }
        let mut r = self.recording.remove(&a[0]).unwrap();
        // Existing checked setters and binding ownership are reused against
        // the recording snapshot, then live state is restored even on failure.
        let swap = |this: &mut Self, r: &mut Recording| {
            let o = this.objects.get_mut(&a[0]).unwrap();
            let Payload::Device(d) = &mut o.payload else {
                unreachable!()
            };
            std::mem::swap(d, &mut r.snapshot);
            std::mem::swap(&mut o.children, &mut r.children);
        };
        swap(self, &mut r);
        let result = self.device_call(m, a, mem);
        swap(self, &mut r);
        if result.is_ok() {
            r.keys.extend(keys(m, a));
        }
        self.recording.insert(a[0], r);
        result
    }
    pub(super) fn end_stateblock<B: Backend>(
        &mut self,
        a: &Args,
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        mem.check(a[1], 4, true)?;
        mem.check(a[1], 4, true)?;
        let r = self.recording.get(&a[0]).ok_or(INVALID)?;
        let mut parents = vec![a[0]];
        parents.extend(resources(&r.snapshot, &r.keys));
        let result = self.object(
            "IDirect3DStateBlock9",
            Payload::StateBlock {
                device: a[0],
                snapshot: r.snapshot.clone(),
                keys: r.keys.clone(),
            },
            1,
            parents,
            vec![],
            mem,
        )?;
        let r = self.recording.remove(&a[0]).unwrap();
        for p in r.children {
            self.unpin(p, mem)?;
        }
        put(mem, a[1], &[result])?;
        Ok(0)
    }
    pub(super) fn create_stateblock<B: Backend>(
        &mut self,
        a: &Args,
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        mem.check(a[2], 4, true)?;
        mem.check(a[2], 4, true)?;
        if !(1..=3).contains(&a[1]) || self.recording.contains_key(&a[0]) {
            return Err(INVALID.into());
        }
        // Typed pixel/vertex blocks await complete state membership support.
        if a[1] != 1 {
            return Err(UNAVAILABLE.into());
        }
        let d = self.device(a[0])?;
        let mut keys = BTreeSet::from([
            Key::Viewport,
            Key::Scissor,
            Key::Texture(0),
            Key::Texture(1),
            Key::VertexFormat,
            Key::VertexShader,
            Key::PixelShader,
            Key::Stream,
            Key::Indices,
        ]);
        keys.extend(
            (0..256)
                .filter(|&n| raster::render(d, n).is_ok())
                .map(Key::Render),
        );
        for slot in 0..2 {
            keys.extend(
                (1..16)
                    .filter(|&n| raster::sampler_slot(d, slot, n).is_ok())
                    .map(|n| Key::Sampler(slot * 16 + n)),
            );
            keys.extend(
                (1..32)
                    .filter(|&n| raster::stage(d, slot, n).is_ok())
                    .map(|n| Key::Stage(slot * 32 + n)),
            );
        }
        keys.extend([2, 3, 256].map(Key::Transform));
        keys.extend((0..256).map(Key::VertexConstant));
        keys.extend((0..32).map(Key::PixelConstant));
        let snapshot = capture(d, &keys)?;
        let mut parents = vec![a[0]];
        parents.extend(resources(&snapshot, &keys));
        let h = self.object(
            "IDirect3DStateBlock9",
            Payload::StateBlock {
                device: a[0],
                snapshot,
                keys,
            },
            1,
            parents,
            vec![],
            mem,
        )?;
        put(mem, a[2], &[h])?;
        Ok(0)
    }
    pub(super) fn stateblock_call<B: Backend>(
        &mut self,
        m: &str,
        a: &Args,
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        let Payload::StateBlock {
            device,
            snapshot,
            keys,
        } = &self.objects[&a[0]].payload
        else {
            return Err(INVALID.into());
        };
        let h = *device;
        if m == "GetDevice" {
            return self.output_ref(h, mem, a[1]);
        }
        if self.recording.contains_key(&h) {
            return Err(INVALID.into());
        }
        let keys = keys.clone();
        if m == "Capture" {
            let snapshot = capture(self.device(h)?, &keys)?;
            let mut parents = vec![h];
            parents.extend(resources(&snapshot, &keys));
            self.check_pins(&parents)?;
            for &p in &parents {
                self.pin(p)?;
            }
            let o = self.objects.get_mut(&a[0]).unwrap();
            let old = std::mem::replace(&mut o.parents, parents);
            o.payload = Payload::StateBlock {
                device: h,
                snapshot,
                keys,
            };
            for p in old {
                self.unpin(p, mem)?;
            }
            return Ok(0);
        }
        if m != "Apply" {
            return Err(UNAVAILABLE.into());
        }
        let snapshot = snapshot.clone();
        self.check_pins(&resources(&snapshot, &keys))?;
        for k in keys {
            match k {
                Key::Texture(slot) => self.bind_slot(
                    h,
                    if slot == 0 {
                        snapshot.texture
                    } else {
                        snapshot.texture1
                    },
                    true,
                    slot,
                    mem,
                )?,
                Key::VertexFormat => {
                    self.set_declaration(h, snapshot.declaration, mem)?;
                    self.device_mut(h)?.fvf = snapshot.fvf;
                }
                Key::VertexShader => {
                    self.set_vertex_shader(h, snapshot.vertex_shader, mem)?;
                }
                Key::PixelShader => {
                    self.set_pixel_shader(h, snapshot.pixel_shader, mem)?;
                }
                Key::Stream => {
                    let mut args = [0; 16];
                    args[..5].copy_from_slice(&[
                        h,
                        0,
                        snapshot.stream[0],
                        snapshot.stream[1],
                        snapshot.stream[2],
                    ]);
                    self.set_stream(&args, mem)?;
                }
                Key::Indices => {
                    let mut args = [0; 16];
                    args[..2].copy_from_slice(&[h, snapshot.indices]);
                    self.set_indices(&args, mem)?;
                }
                _ => {
                    let d = self.device_mut(h)?;
                    match k {
                        Key::Render(n) => {
                            copy_entry(&mut d.render, &snapshot.render, n);
                        }
                        Key::Sampler(n) => {
                            copy_entry(&mut d.sampler, &snapshot.sampler, n);
                        }
                        Key::Stage(n) => {
                            copy_entry(&mut d.stage, &snapshot.stage, n);
                        }
                        Key::Transform(n) => {
                            copy_entry(&mut d.transforms, &snapshot.transforms, n);
                        }
                        Key::VertexConstant(i) => {
                            d.vertex_constants[i] = snapshot.vertex_constants[i]
                        }
                        Key::PixelConstant(i) => d.pixel_constants[i] = snapshot.pixel_constants[i],
                        Key::Viewport => d.viewport = snapshot.viewport,
                        Key::Scissor => d.scissor = snapshot.scissor,
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(0)
    }
}
fn copy_entry<T: Copy>(dst: &mut BTreeMap<u32, T>, src: &BTreeMap<u32, T>, key: u32) {
    if let Some(&v) = src.get(&key) {
        dst.insert(key, v);
    } else {
        dst.remove(&key);
    }
}
