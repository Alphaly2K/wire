//! Adapt typed stage outputs without touching the original shader body.
use crate::{finish, parse_module, validate_module, Translation};
use naga::{
    Binding, Expression, Function, FunctionResult, Handle, Module, Scalar, Span, Statement,
    StructMember, Type, TypeInner, VectorSize,
};
use std::collections::BTreeMap;
const S: Span = Span::UNDEFINED;
struct Leaf {
    ty: Handle<Type>,
    binding: Binding,
    path: Vec<u32>,
}
fn leaves(
    module: &Module,
    ty: Handle<Type>,
    binding: Option<&Binding>,
    path: &mut Vec<u32>,
    result: &mut Vec<Leaf>,
) -> Result<(), String> {
    if result.len() >= 64 || path.len() > 8 {
        return Err("shader link interface capacity".into());
    }
    if let Some(binding) = binding {
        result.push(Leaf {
            ty,
            binding: binding.clone(),
            path: path.clone(),
        });
    } else if let TypeInner::Struct { ref members, .. } = module.types[ty].inner {
        for (index, member) in members.iter().enumerate() {
            path.push(index as u32);
            leaves(module, member.ty, member.binding.as_ref(), path, result)?;
            path.pop();
        }
    } else {
        return Err("shader link unbound interface".into());
    }
    Ok(())
}
fn shape(module: &Module, ty: Handle<Type>) -> Result<(u32, Scalar), String> {
    match module.types[ty].inner {
        TypeInner::Scalar(scalar) => Ok((1, scalar)),
        TypeInner::Vector { size, scalar } => Ok((size as u32, scalar)),
        _ => Err("shader link interface must be scalar/vector".into()),
    }
}
fn location(binding: &Binding) -> Option<u32> {
    match binding {
        Binding::Location { location, .. } => Some(*location),
        _ => None,
    }
}
/// D3D permits an output vector to have more components than its consumer.
/// Preserve builtins, omit unused locations, and select only consumed lanes.
/// Missing locations, short vectors and scalar reinterpretations stay errors.
pub fn link_vertex(vertex: &[u8], pixel: &[u8]) -> Result<Translation, String> {
    let vertex = parse_module(vertex, 1)?;
    let pixel = parse_module(pixel, 2)?;
    link_modules(vertex, pixel)
}
pub(crate) fn link_modules(mut vertex: Module, pixel: Module) -> Result<Translation, String> {
    validate_module(&vertex)?;
    validate_module(&pixel)?;
    let mut inputs = Vec::new();
    for argument in &pixel.entry_points[0].function.arguments {
        leaves(
            &pixel,
            argument.ty,
            argument.binding.as_ref(),
            &mut Vec::new(),
            &mut inputs,
        )?;
    }
    let mut needed = BTreeMap::new();
    for input in inputs {
        if let Some(at) = location(&input.binding) {
            if at >= 32 || needed.insert(at, input).is_some() {
                return Err("shader link duplicate/invalid pixel location".into());
            }
        }
    }
    let result = vertex.entry_points[0]
        .function
        .result
        .as_ref()
        .ok_or("shader link vertex result absent")?;
    let mut outputs = Vec::new();
    leaves(
        &vertex,
        result.ty,
        result.binding.as_ref(),
        &mut Vec::new(),
        &mut outputs,
    )?;
    let mut selected = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for mut output in outputs {
        if let Some(at) = location(&output.binding) {
            if at >= 32 || !seen.insert(at) {
                return Err("shader link duplicate/invalid vertex location".into());
            }
            let Some(input) = needed.remove(&at) else {
                continue;
            };
            let (available, scalar) = shape(&vertex, output.ty)?;
            let (used, expected) = shape(&pixel, input.ty)?;
            if scalar != expected || available < used {
                return Err(format!("shader I/O location mismatch at {at}: vertex={scalar:?}/{available} pixel={expected:?}/{used}"));
            }
            // D3D interpolation is selected by the pixel input declaration.
            output.binding = input.binding;
            let inner = if used == 1 {
                TypeInner::Scalar(scalar)
            } else {
                TypeInner::Vector {
                    size: match used {
                        2 => VectorSize::Bi,
                        3 => VectorSize::Tri,
                        4 => VectorSize::Quad,
                        _ => unreachable!(),
                    },
                    scalar,
                }
            };
            let target = vertex.types.insert(Type { name: None, inner }, S);
            selected.push((output, target, available, used));
        } else {
            let target = output.ty;
            selected.push((output, target, 0, 0));
        }
    }
    if let Some((&at, _)) = needed.first_key_value() {
        return Err(format!(
            "shader I/O location mismatch at {at}: vertex=missing"
        ));
    }
    let mut layout = naga::proc::Layouter::default();
    layout
        .update(vertex.to_ctx())
        .map_err(|e| format!("shader link layout: {e}"))?;
    let mut offset = 0;
    let mut alignment = naga::proc::Alignment::ONE;
    let mut members = Vec::new();
    for (output, target, _, _) in &selected {
        let member_layout = layout[*target];
        offset = member_layout.alignment.round_up(offset);
        alignment = alignment.max(member_layout.alignment);
        members.push(StructMember {
            name: None,
            ty: *target,
            binding: Some(output.binding.clone()),
            offset,
        });
        offset += member_layout.size;
    }
    if members.is_empty() {
        return Err("shader link vertex output empty".into());
    }
    let ty = vertex.types.insert(
        Type {
            name: Some("WireLinkedVertexOutput".into()),
            inner: TypeInner::Struct {
                members,
                span: alignment.round_up(offset),
            },
        },
        S,
    );
    let mut original = std::mem::take(&mut vertex.entry_points[0].function);
    let mut wrapper = Function {
        name: original.name.clone(),
        arguments: original.arguments.clone(),
        result: Some(FunctionResult { ty, binding: None }),
        ..Function::default()
    };
    original.name = Some("wire_vertex_body".into());
    for argument in &mut original.arguments {
        argument.binding = None;
    }
    if let Some(result) = &mut original.result {
        result.binding = None;
    }
    let function = vertex.functions.append(original, S);
    let arguments = (0..wrapper.arguments.len())
        .map(|i| {
            wrapper
                .expressions
                .append(Expression::FunctionArgument(i as u32), S)
        })
        .collect();
    let call = wrapper
        .expressions
        .append(Expression::CallResult(function), S);
    wrapper.body.push(
        Statement::Call {
            function,
            arguments,
            result: Some(call),
        },
        S,
    );
    let first = wrapper.expressions.len();
    let mut components = Vec::new();
    for (output, _, available, used) in selected {
        let mut value = call;
        for index in output.path {
            value = wrapper
                .expressions
                .append(Expression::AccessIndex { base: value, index }, S);
        }
        if used != available {
            // Compose from scalar accesses so any legal vector width is
            // handled uniformly; no numeric casts or invented components.
            let lanes: Vec<_> = (0..used)
                .map(|index| {
                    wrapper
                        .expressions
                        .append(Expression::AccessIndex { base: value, index }, S)
                })
                .collect();
            value = if used == 1 {
                lanes[0]
            } else {
                let scalar = shape(&vertex, output.ty)?.1;
                let inner = TypeInner::Vector {
                    size: if used == 2 {
                        VectorSize::Bi
                    } else {
                        VectorSize::Tri
                    },
                    scalar,
                };
                let ty = vertex.types.insert(Type { name: None, inner }, S);
                wrapper.expressions.append(
                    Expression::Compose {
                        ty,
                        components: lanes,
                    },
                    S,
                )
            };
        }
        components.push(value);
    }
    let value = wrapper
        .expressions
        .append(Expression::Compose { ty, components }, S);
    wrapper
        .body
        .push(Statement::Emit(wrapper.expressions.range_from(first)), S);
    wrapper
        .body
        .push(Statement::Return { value: Some(value) }, S);
    vertex.entry_points[0].function = wrapper;
    finish(&vertex, 1)
}
