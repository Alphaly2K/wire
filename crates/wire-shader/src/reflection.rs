//! Reflection from the validated upstream IR; no second bytecode parser.
use naga::{
    AddressSpace, ArraySize, Binding, Handle, ImageClass, Module, ScalarKind, Type, TypeInner,
};
use serde::Serialize;
use std::collections::BTreeSet;
#[derive(Debug, Serialize)]
pub struct ResourceBinding {
    pub set: u32,
    pub binding: u32,
    pub descriptor_type: u32,
    pub count: u32,
    pub min_size: u32,
    pub shape: String,
}
#[derive(Debug, Serialize)]
pub struct InternalBinding {
    pub set: u32,
    pub binding: u32,
    pub parameter: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Io {
    pub location: u32,
    pub components: u32,
    pub scalar: &'static str,
    pub width: u8,
    pub interpolation: Option<String>,
    pub sampling: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub code: String,
    pub entry_point: String,
    pub stage: u32,
    pub bindings: Vec<ResourceBinding>,
    pub internal_bindings: Vec<InternalBinding>,
    pub inputs: Vec<u32>,
    pub input_interface: Vec<Io>,
    pub output_interface: Vec<Io>,
    pub workgroup_size: [u32; 3],
}
fn scalar(kind: ScalarKind) -> Result<&'static str, String> {
    match kind {
        ScalarKind::Float => Ok("float"),
        ScalarKind::Uint => Ok("uint"),
        ScalarKind::Sint => Ok("sint"),
        _ => Err("unsupported shader interface scalar".into()),
    }
}
pub(crate) fn io(
    module: &Module,
    ty: Handle<Type>,
    binding: Option<&Binding>,
    result: &mut Vec<Io>,
) -> Result<(), String> {
    if let Some(Binding::Location {
        location,
        interpolation,
        sampling,
        blend_src,
        per_primitive,
    }) = binding
    {
        if *location >= 32 || blend_src.is_some() || *per_primitive {
            return Err("unsupported shader interface location/qualifier".into());
        }
        let (components, value) = match module.types[ty].inner {
            TypeInner::Scalar(value) => (1, value),
            TypeInner::Vector { size, scalar } => (size as u32, scalar),
            _ => return Err("unsupported shader interface type".into()),
        };
        result.push(Io {
            location: *location,
            components,
            scalar: scalar(value.kind)?,
            width: value.width,
            interpolation: interpolation.map(|v| format!("{v:?}")),
            sampling: sampling.map(|v| format!("{v:?}")),
        });
    } else if binding.is_none() {
        if let TypeInner::Struct { ref members, .. } = module.types[ty].inner {
            for member in members {
                io(module, member.ty, member.binding.as_ref(), result)?;
            }
        }
    }
    if result.len() > 32 {
        return Err("shader interface capacity".into());
    }
    Ok(())
}
pub(crate) fn reflected(
    module: &Module,
    stage: u32,
) -> Result<(Vec<ResourceBinding>, Vec<InternalBinding>), String> {
    let mut bindings = Vec::new();
    let mut internal = Vec::new();
    let mut seen = BTreeSet::new();
    let mut layout = naga::proc::Layouter::default();
    layout
        .update(module.to_ctx())
        .map_err(|e| format!("shader resource layout: {e}"))?;
    let stage_set = match stage {
        1 => 0,
        2 => 1,
        4 => 2,
        _ => return Err("unsupported shader stage".into()),
    };
    for (_, variable) in module.global_variables.iter() {
        if variable.space == AddressSpace::Immediate {
            return Err("portable shader push data requires lowering".into());
        }
        let Some(binding) = variable.binding else {
            continue;
        };
        if !seen.insert((binding.group, binding.binding)) {
            return Err("duplicate shader resource binding".into());
        }
        let (ty, count) = match module.types[variable.ty].inner {
            TypeInner::BindingArray {
                base,
                size: ArraySize::Constant(size),
            } => (base, size.get()),
            TypeInner::BindingArray { .. } => {
                return Err("unbounded shader resource binding array".into())
            }
            _ => (variable.ty, 1),
        };
        if binding.group == 3 {
            if stage != 1
                || binding.binding > 1
                || count != 1
                || variable.space != AddressSpace::Uniform
            {
                return Err("invalid internal draw parameter binding".into());
            }
            match module.types[ty].inner {
                TypeInner::Struct { ref members, span }
                    if members.len() == 1 && span == 4 && members[0].offset == 0 =>
                {
                    if !matches!(
                        module.types[members[0].ty].inner,
                        TypeInner::Scalar(naga::Scalar {
                            kind: ScalarKind::Sint,
                            width: 4
                        })
                    ) {
                        return Err("invalid internal draw parameter scalar".into());
                    }
                }
                _ => return Err("invalid internal draw parameter layout".into()),
            }
            internal.push(InternalBinding {
                set: 3,
                binding: binding.binding,
                parameter: if binding.binding == 0 {
                    "baseVertex"
                } else {
                    "baseInstance"
                },
            });
            continue;
        }
        if binding.group != stage_set || count == 0 || count > 128 {
            return Err("shader resource stage/count".into());
        }
        let (descriptor_type, shape, min_size) = match variable.space {
            AddressSpace::Uniform => (6, "uniform".into(), layout[ty].size),
            AddressSpace::Storage { access } => (7, format!("storage:{access:?}"), layout[ty].size),
            AddressSpace::Handle => match module.types[ty].inner {
                TypeInner::Sampler { comparison } => (
                    0,
                    if comparison {
                        "comparison"
                    } else {
                        "filtering"
                    }
                    .into(),
                    0,
                ),
                TypeInner::Image {
                    dim,
                    arrayed,
                    class,
                } => {
                    let kind = match class {
                        ImageClass::Storage { .. } => 3,
                        ImageClass::External => return Err("external shader image".into()),
                        _ => 2,
                    };
                    (kind, format!("{dim:?}:{arrayed}:{class:?}"), 0)
                }
                _ => return Err("unsupported shader handle type".into()),
            },
            _ => return Err("unsupported bound shader address space".into()),
        };
        let valid_register = match descriptor_type {
            6 => binding.binding < 14,
            0 => (142..158).contains(&binding.binding),
            2 => (14..142).contains(&binding.binding),
            3 | 7 => {
                (14..142).contains(&binding.binding)
                    || (158..166).contains(&binding.binding)
                    || binding.binding < 14
            }
            _ => false,
        };
        if !valid_register {
            return Err("shader D3D resource register mismatch".into());
        }
        bindings.push(ResourceBinding {
            set: binding.group,
            binding: binding.binding,
            descriptor_type,
            count,
            min_size,
            shape,
        });
        if bindings.len() > 128 {
            return Err("shader reflection capacity".into());
        }
    }
    bindings.sort_by_key(|v| (v.set, v.binding));
    internal.sort_by_key(|v| v.binding);
    Ok((bindings, internal))
}
