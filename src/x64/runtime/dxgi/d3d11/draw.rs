//! Translate owned context bindings into typed native draw inputs.
use super::*;
use crate::gpu::draw::{Attribute, Descriptor, Draw, Resource as BindingResource, Vertex};
fn words<const N: usize>(bytes: &[u8], name: &str) -> Result<[u32; N], String> {
    if bytes.len() != N * 4 {
        return Err(format!("D3D11 {name} descriptor size"));
    }
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
impl Runtime64 {
    pub fn graphics_draws(&self) -> u64 {
        self.graphics.drawn
    }
    pub(super) fn d3d_draw(&mut self, method: &str, a: &[u64]) -> Result<u64, String> {
        let indexed = method.contains("Indexed");
        let instanced = method.contains("Instanced");
        let count = a[1] as u32;
        let instances = if instanced { a[2] as u32 } else { 1 };
        if count == 0 || instances == 0 {
            return Ok(0);
        }
        let context = &self.graphics.contexts[&a[0]];
        let bound = |name: &str, slot: usize| {
            context
                .bindings
                .get(name)
                .and_then(|b| b.get(slot))
                .copied()
                .unwrap_or(0)
        };
        let state = |name: &str| -> Result<&[u8], String> {
            match self.graphics.resources.get(&bound(name, 0)) {
                Some(Resource::State(s)) => Ok(s),
                _ => Err(format!("D3D11 missing {name} state")),
            }
        };
        let value = |name: &str| context.values.get(name).map(Vec::as_slice).unwrap_or(&[]);
        let buffer = |p: u64| -> Result<&crate::gpu::Buffer, String> {
            match self.graphics.resources.get(&p) {
                Some(Resource::Buffer {
                    native,
                    mapped: None,
                    ..
                }) => Ok(native),
                _ => Err(format!("D3D11 draw buffer {p:#x} missing or mapped")),
            }
        };
        let view = |p: u64| -> Result<&crate::gpu::ImageView, String> {
            match self.graphics.resources.get(&p) {
                Some(Resource::View {
                    native: NativeView::Image(v),
                    resource,
                    ..
                }) => {
                    if !matches!(self.graphics.resources.get(resource), Some(Resource::Texture {mapped, ..}) if mapped.is_empty())
                    {
                        return Err("D3D11 draw image is mapped".into());
                    }
                    Ok(v)
                }
                _ => Err(format!("D3D11 draw image view {p:#x} missing")),
            }
        };
        for stage in ["GSSetShader", "HSSetShader", "DSSetShader"] {
            if bound(stage, 0) != 0 {
                return Err(format!("D3D11 guest draw stage {stage} unsupported"));
            }
        }
        if (1..8).any(|slot| bound("OMSetRenderTargets", slot) != 0) {
            return Err("D3D11 guest draw multiple render targets unsupported".into());
        }
        let shader = |name: &str| -> Result<&crate::gpu::Shader, String> {
            match self.graphics.resources.get(&bound(name, 0)) {
                Some(Resource::Shader(s)) => Ok(s),
                _ => Err(format!("D3D11 draw {name} missing")),
            }
        };
        let vertex = shader("VSSetShader")?;
        let pixel = shader("PSSetShader")?;
        let mut descriptors = Vec::new();
        for (stage, shader) in [("VS", vertex), ("PS", pixel)] {
            for binding in &shader.bindings {
                if binding.count != 1 {
                    return Err("D3D11 guest draw descriptor array unsupported".into());
                }
                let resource = match (binding.binding, binding.descriptor_type) {
                    (slot @ 0..14, 6) => BindingResource::Constant(buffer(bound(
                        &format!("{stage}SetConstantBuffers"),
                        slot as usize,
                    ))?),
                    (slot @ 14..142, 2) => BindingResource::Image(view(bound(
                        &format!("{stage}SetShaderResources"),
                        (slot - 14) as usize,
                    ))?),
                    (slot @ 14..142, 4 | 7) => {
                        let p = bound(&format!("{stage}SetShaderResources"), (slot - 14) as usize);
                        let Some(Resource::View {
                            native: NativeView::Buffer(v),
                            resource,
                            ..
                        }) = self.graphics.resources.get(&p)
                        else {
                            return Err("D3D11 draw buffer SRV missing".into());
                        };
                        buffer(*resource)?;
                        BindingResource::Buffer(v)
                    }
                    (slot @ 142..158, 0) => {
                        let p = bound(&format!("{stage}SetSamplers"), (slot - 142) as usize);
                        let Some(Resource::State(s)) = self.graphics.resources.get(&p) else {
                            return Err("D3D11 draw sampler missing".into());
                        };
                        BindingResource::Sampler(words(s, "sampler")?)
                    }
                    _ => return Err(format!("D3D11 draw descriptor unsupported: {binding:?}")),
                };
                descriptors.push(Descriptor {
                    set: binding.set,
                    binding: binding.binding,
                    resource,
                });
            }
        }
        let mut attributes = Vec::new();
        if bound("IASetInputLayout", 0) != 0 {
            for raw in state("IASetInputLayout")?.as_chunks::<24>().0 {
                let d = words::<6>(raw, "input attribute")?;
                attributes.push(Attribute {
                    location: d[0],
                    format: d[1],
                    slot: d[2],
                    offset: d[3],
                    input_class: d[4],
                    step: d[5],
                });
            }
        }
        let mut vertices = Vec::new();
        let input = value("IASetVertexBuffers");
        let slots: std::collections::BTreeSet<_> =
            attributes.iter().map(|a| a.slot as usize).collect();
        for slot in slots {
            let raw = input
                .get(slot * 8..slot * 8 + 8)
                .ok_or("D3D11 vertex binding values missing")?;
            let [stride, offset] = words::<2>(raw, "vertex binding")?;
            vertices.push(Vertex {
                slot,
                buffer: buffer(bound("IASetVertexBuffers", slot))?,
                stride,
                offset,
            });
        }
        let index = if indexed {
            Some(buffer(bound("IASetIndexBuffer", 0))?)
        } else {
            None
        };
        let [index_format, index_offset] = if indexed {
            words(value("IASetIndexBuffer"), "index binding")?
        } else {
            [0; 2]
        };
        let mut blend = [0u32; 66];
        for n in 0..8 {
            blend[2 + n * 8 + 7] = 15;
        }
        if bound("OMSetBlendState", 0) != 0 {
            blend = words(state("OMSetBlendState")?, "blend")?;
        }
        let mut blend_factor = [1.; 4];
        let mut sample_mask = u32::MAX;
        let blend_values = value("OMSetBlendState");
        if !blend_values.is_empty() {
            let v = words::<5>(blend_values, "blend values")?;
            blend_factor = std::array::from_fn(|i| f32::from_bits(v[i]));
            sample_mask = v[4];
        }
        let depth_state = if bound("OMSetDepthStencilState", 0) != 0 {
            words(state("OMSetDepthStencilState")?, "depth")?
        } else {
            [1, 1, 2, 0, 0xffff, 1, 1, 1, 8, 1, 1, 1, 8]
        };
        let stencil_ref = if value("OMSetDepthStencilState").is_empty() {
            0
        } else {
            words::<1>(value("OMSetDepthStencilState"), "stencil ref")?[0]
        };
        let raster = if bound("RSSetState", 0) != 0 {
            words(state("RSSetState")?, "raster")?
        } else {
            [3, 3, 0, 0, 0, 0, 1, 0, 0, 0]
        };
        let viewport = words::<6>(value("RSSetViewports"), "single viewport")?.map(f32::from_bits);
        let scissor = if raster[7] != 0 {
            words::<4>(value("RSSetScissorRects"), "single scissor")?.map(|v| v as i32)
        } else {
            [0; 4]
        };
        let [topology] = words(value("IASetPrimitiveTopology"), "topology")?;
        let first = a[if instanced { 3 } else { 2 }] as u32;
        let base_vertex = if indexed {
            a[if instanced { 4 } else { 3 }] as i32
        } else {
            0
        };
        let first_instance = if instanced {
            a[if indexed { 5 } else { 4 }] as u32
        } else {
            0
        };
        let d = Draw {
            vertex,
            pixel,
            target: view(bound("OMSetRenderTargets", 0))?,
            depth: if bound("OMSetRenderTargets", 8) != 0 {
                Some(view(bound("OMSetRenderTargets", 8))?)
            } else {
                None
            },
            index,
            vertices,
            attributes,
            descriptors,
            topology,
            index_format,
            index_offset,
            indexed,
            count,
            first,
            base_vertex,
            instances,
            first_instance,
            viewport,
            scissor,
            blend,
            depth_state,
            raster,
            blend_factor,
            sample_mask,
            stencil_ref,
        };
        self.graphics
            .backend
            .as_ref()
            .ok_or("D3D11 draw backend missing")?
            .draw(&d)?;
        self.graphics.drawn += 1;
        Ok(0)
    }
}
