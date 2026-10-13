//! Complete, bounded IL proof for IntPtr parameters used only as scalar outputs.
//! Tokens and pointed types come from the current image. No guest pointer is
//! admitted by the ordinary ABI or passed to native Mono storage.
use super::{Api, Descriptor, Object, Parameter, Shape};
use std::{collections::BTreeMap, ffi::CStr, ptr};

pub(in crate::x64::process::mono_startup) struct ScalarOutputDescriptor {
    pub(in crate::x64::process::mono_startup) call: Descriptor,
    pub(in crate::x64::process::mono_startup) outputs: Vec<(usize, Shape)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    Plain,
    Address(usize),
    Pointer(usize, Option<Shape>),
}
#[derive(Clone, Copy)]
enum Local {
    Plain,
    Address,
    Pointer(Shape),
}
#[derive(Clone)]
struct State {
    stack: Vec<Value>,
    locals: Vec<Option<Value>>,
    written: Vec<bool>,
}
#[derive(Clone)]
struct Instruction {
    at: usize,
    next: usize,
    op: u16,
    operand: u32,
    targets: Vec<usize>,
}
struct Call {
    inputs: usize,
    result: bool,
    converter: bool,
}
fn decode(code: &[u8]) -> Result<Vec<Instruction>, String> {
    let mut result = Vec::new();
    let mut pc = 0;
    while pc < code.len() {
        let at = pc;
        let first = code[pc];
        pc += 1;
        let op = if first == 0xfe {
            let second = *code.get(pc).ok_or("truncated scalar-output opcode")?;
            pc += 1;
            0xfe00 | u16::from(second)
        } else {
            u16::from(first)
        };
        let width = match op {
            0x0e | 0x11 | 0x13 | 0x1f | 0x2b..=0x37 => 1,
            0xfe09 | 0xfe0c | 0xfe0e => 2,
            0x20 | 0x22 | 0x28 | 0x38..=0x44 | 0x6f | 0x7b | 0x7e => 4,
            0x21 | 0x23 => 8,
            0x45 => {
                let bytes = code.get(pc..pc + 4).ok_or("truncated output switch")?;
                let count = u32::from_le_bytes(bytes.try_into().unwrap()) as usize;
                if count > 64 {
                    return Err("scalar-output switch exceeds bound".into());
                }
                4 + count * 4
            }
            0x00
            | 0x02..=0x0d
            | 0x14..=0x1e
            | 0x25
            | 0x26
            | 0x2a
            | 0x52..=0x57
            | 0x58..=0x6e
            | 0xfe01..=0xfe05 => 0,
            _ => return Err(format!("unproved scalar-output opcode 0x{op:x}")),
        };
        let bytes = code
            .get(pc..pc + width)
            .ok_or("truncated scalar-output operand")?;
        let operand = match width {
            1 => bytes[0] as u32,
            2 => u16::from_le_bytes(bytes.try_into().unwrap()) as u32,
            4 => u32::from_le_bytes(bytes.try_into().unwrap()),
            _ => 0,
        };
        pc += width;
        let target = |delta: i32| -> Result<usize, String> {
            let value = pc as i64 + i64::from(delta);
            if value < 0 || value >= code.len() as i64 {
                return Err("scalar-output branch leaves body".into());
            }
            Ok(value as usize)
        };
        let targets = match op {
            0x2b..=0x37 => vec![target(operand as u8 as i8 as i32)?],
            0x38..=0x44 => vec![target(operand as i32)?],
            0x45 => bytes[4..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| target(i32::from_le_bytes(*b)))
                .collect::<Result<_, _>>()?,
            _ => Vec::new(),
        };
        result.push(Instruction {
            at,
            next: pc,
            op,
            operand,
            targets,
        });
    }
    Ok(result)
}
fn plain(value: Value) -> Result<(), String> {
    if value != Value::Plain {
        return Err("scalar-output address escapes into an ordinary operation".into());
    }
    Ok(())
}
fn pop(state: &mut State) -> Result<Value, String> {
    state
        .stack
        .pop()
        .ok_or_else(|| "scalar-output IL stack underflow".into())
}
fn merge(destination: &mut Option<State>, source: &State) -> Result<(), String> {
    match destination {
        None => *destination = Some(source.clone()),
        Some(old) => {
            if old.stack != source.stack || old.locals.len() != source.locals.len() {
                return Err("scalar-output join has ambiguous pointer provenance".into());
            }
            for (left, right) in old.locals.iter_mut().zip(&source.locals) {
                if *left != *right {
                    if left.is_some() && right.is_some() {
                        return Err("scalar-output local join changes pointer provenance".into());
                    }
                    *left = None;
                }
            }
            for (left, right) in old.written.iter_mut().zip(&source.written) {
                *left &= *right;
            }
        }
    }
    Ok(())
}
fn prove(
    code: &[u8],
    arguments: &[Value],
    locals: &[Local],
    max_stack: usize,
    mut call: impl FnMut(u32, bool) -> Result<Call, String>,
    mut field: impl FnMut(u32, bool) -> Result<(), String>,
) -> Result<Vec<(usize, Shape)>, String> {
    let instructions = decode(code)?;
    if instructions.is_empty() || instructions.len() > 2048 || max_stack > 64 {
        return Err("scalar-output complete body exceeds proof bounds".into());
    }
    let indices: BTreeMap<_, _> = instructions
        .iter()
        .enumerate()
        .map(|(i, op)| (op.at, i))
        .collect();
    let mut states = vec![None; instructions.len()];
    states[0] = Some(State {
        stack: Vec::new(),
        locals: locals
            .iter()
            .map(|l| matches!(l, Local::Plain).then_some(Value::Plain))
            .collect(),
        written: vec![false; arguments.len()],
    });
    let mut outputs = BTreeMap::new();
    let mut returns = 0;
    for (index, instruction) in instructions.iter().enumerate() {
        let mut state = states[index]
            .take()
            .ok_or("scalar-output body contains unreachable IL")?;
        let op = instruction.op;
        match op {
            0x00 => {}
            0x02..=0x05 | 0x0e | 0xfe09 => {
                let n = if op <= 5 {
                    (op - 2) as usize
                } else {
                    instruction.operand as usize
                };
                state.stack.push(
                    *arguments
                        .get(n)
                        .ok_or("scalar-output argument index invalid")?,
                );
            }
            0x06..=0x09 | 0x11 | 0xfe0c => {
                let n = if op <= 9 {
                    (op - 6) as usize
                } else {
                    instruction.operand as usize
                };
                state.stack.push(
                    state
                        .locals
                        .get(n)
                        .copied()
                        .flatten()
                        .ok_or("scalar-output local lacks definite provenance")?,
                );
            }
            0x0a..=0x0d | 0x13 | 0xfe0e => {
                let n = if op <= 0xd {
                    (op - 0xa) as usize
                } else {
                    instruction.operand as usize
                };
                let value = pop(&mut state)?;
                let value = match (locals.get(n), value) {
                    (Some(Local::Plain), Value::Plain) => Value::Plain,
                    (Some(Local::Address), Value::Address(p)) => Value::Address(p),
                    (Some(Local::Pointer(shape)), Value::Pointer(p, prior))
                        if prior.is_none() || prior == Some(*shape) =>
                    {
                        Value::Pointer(p, Some(*shape))
                    }
                    _ => return Err("scalar-output local type/provenance mismatch".into()),
                };
                state.locals[n] = Some(value);
            }
            0x14..=0x23 => state.stack.push(Value::Plain),
            0x25 => {
                let value = *state.stack.last().ok_or("scalar-output dup underflow")?;
                state.stack.push(value);
            }
            0x26 => plain(pop(&mut state)?)?,
            0x28 | 0x6f => {
                let callee = call(instruction.operand, op == 0x6f)?;
                if callee.converter {
                    if op != 0x28 || callee.inputs != 1 || !callee.result {
                        return Err("scalar-output converter ABI mismatch".into());
                    }
                    let Value::Address(parameter) = pop(&mut state)? else {
                        return Err(
                            "scalar-output converter lacks input parameter provenance".into()
                        );
                    };
                    state.stack.push(Value::Pointer(parameter, None));
                } else {
                    for _ in 0..callee.inputs {
                        plain(pop(&mut state)?)?;
                    }
                    if callee.result {
                        state.stack.push(Value::Plain);
                    }
                }
            }
            0x2a => {
                if !state.stack.is_empty()
                    || arguments
                        .iter()
                        .enumerate()
                        .any(|(i, value)| matches!(value, Value::Address(_)) && !state.written[i])
                {
                    return Err("scalar-output return lacks a definite complete write".into());
                }
                returns += 1;
                continue;
            }
            0x2b | 0x38 => {}
            0x2c | 0x2d | 0x39 | 0x3a | 0x45 => plain(pop(&mut state)?)?,
            0x2e..=0x37 | 0x3b..=0x44 => {
                plain(pop(&mut state)?)?;
                plain(pop(&mut state)?)?;
            }
            0x52..=0x57 => {
                plain(pop(&mut state)?)?;
                let Value::Pointer(parameter, Some(shape)) = pop(&mut state)? else {
                    return Err("scalar-output store lacks actual pointed primitive type".into());
                };
                let width = [1, 2, 4, 8, 4, 8][(op - 0x52) as usize];
                if shape.width() != width || !matches!(shape, Shape::Scalar { .. }) {
                    return Err(
                        "scalar-output indirect store width differs from pointed type".into(),
                    );
                }
                if let Some(prior) = outputs.insert(parameter, shape) {
                    if prior != shape {
                        return Err("scalar-output parameter has differing write types".into());
                    }
                }
                state.written[parameter] = true;
            }
            0x58..=0x64 | 0xfe01..=0xfe05 => {
                plain(pop(&mut state)?)?;
                plain(pop(&mut state)?)?;
                state.stack.push(Value::Plain);
            }
            0x65..=0x6e => {
                plain(pop(&mut state)?)?;
                state.stack.push(Value::Plain);
            }
            0x7b | 0x7e => {
                field(instruction.operand, op == 0x7e)?;
                if op == 0x7b {
                    plain(pop(&mut state)?)?;
                }
                state.stack.push(Value::Plain);
            }
            _ => return Err("unproved scalar-output operation".into()),
        }
        if state.stack.len() > max_stack {
            return Err("scalar-output stack exceeds declared bound".into());
        }
        let mut destinations = instruction.targets.clone();
        if !matches!(op, 0x2b | 0x38) {
            destinations.push(instruction.next);
        }
        for at in destinations {
            let target = *indices
                .get(&at)
                .ok_or("scalar-output branch is not an instruction boundary")?;
            // Acyclic wrappers are enough for finite outputs; loops need a
            // separate termination/range proof, never an optimistic iteration.
            if target <= index {
                return Err("scalar-output body contains a back edge".into());
            }
            merge(&mut states[target], &state)?;
        }
    }
    if returns == 0
        || outputs.len()
            != arguments
                .iter()
                .filter(|v| matches!(v, Value::Address(_)))
                .count()
    {
        return Err("scalar-output body has no complete normal return".into());
    }
    Ok(outputs.into_iter().collect())
}

impl Api {
    pub(in crate::x64::process::mono_startup) fn describe_scalar_outputs(
        self,
        method: Object,
        images: &[Object],
    ) -> Result<Option<ScalarOutputDescriptor>, String> {
        if method.is_null() {
            return Err("NULL scalar-output method".into());
        }
        let signature = unsafe { (self.signature)(method) };
        if signature.is_null() {
            return Ok(None);
        }
        let count = unsafe { (self.count)(signature) } as usize;
        if count > 16 {
            return Ok(None);
        }
        let mut iterator = ptr::null_mut();
        let types: Vec<_> = (0..count)
            .map(|_| unsafe { (self.parameters)(signature, &mut iterator) })
            .collect();
        if !types
            .iter()
            .any(|&ty| !ty.is_null() && unsafe { (self.kind)(ty) } == 0x18)
        {
            return Ok(None);
        }
        let owner = unsafe { (self.class)(method) };
        let mut implementation = 0;
        let flags = unsafe { (self.flags)(method, &mut implementation) };
        let ret = unsafe { (self.returns)(signature) };
        if owner.is_null()
            || !images.contains(&unsafe { (self.image)(owner) })
            || !super::managed_il_flags(flags, implementation, false)
            || unsafe { (self.instance)(signature) } != 0
            || unsafe { (self.generic)(method) } != 0
            || unsafe { (self.inflated)(method) } != 0
            || ret.is_null()
            || unsafe { (self.kind)(ret) } != 1
            || unsafe { (self.byref)(ret) } != 0
        {
            return Err("scalar-output entry is not owned concrete static void IL".into());
        }
        self.closed.concrete(owner)?;
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing scalar-output metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        let intptr = unsafe { intptr() };
        let clauses = sym!(
            "mono_method_header_get_num_clauses",
            unsafe extern "C" fn(Object) -> i32
        );
        let local_types = sym!(
            "mono_method_header_get_locals",
            unsafe extern "C" fn(Object, *mut u32, *mut i32) -> *const Object
        );
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let resolve_field = sym!(
            "mono_field_from_token",
            unsafe extern "C" fn(Object, u32, *mut Object, Object) -> Object
        );
        let field_type = sym!(
            "mono_field_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let field_flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
        let mut alignment = 0;
        if intptr.is_null()
            || !images.contains(&unsafe { (self.image)(intptr) })
            || unsafe { (self.size)(intptr, &mut alignment) } != 8
            || alignment != 8
        {
            return Err("scalar-output IntPtr identity/width mismatch".into());
        }
        let pointer_shape = Shape::Scalar {
            class: intptr,
            width: 8,
            alignment: 8,
            boolean: false,
        };
        let ordinary = |ty: Object| -> Result<(), String> {
            if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                return Err("scalar-output dependency has byref/missing type".into());
            }
            self.shape(ty, images).map(|_| ())
        };
        let mut parameters = Vec::new();
        let mut arguments = Vec::new();
        for (index, &ty) in types.iter().enumerate() {
            if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                return Err("scalar-output input is missing/byref".into());
            }
            let shape = if unsafe { (self.kind)(ty) } == 0x18 {
                if unsafe { (self.type_class)(ty) } != intptr {
                    return Err("scalar-output input differs from actual IntPtr".into());
                }
                arguments.push(Value::Address(index));
                pointer_shape
            } else {
                arguments.push(Value::Plain);
                self.shape(ty, images)?
            };
            parameters.push(Parameter {
                shape,
                byref: false,
            });
        }
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("scalar-output entry has no body".into());
        }
        let body = (|| {
            let (mut length, mut stack, mut nlocals) = (0, 0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let types = unsafe { local_types(header, &mut nlocals, ptr::null_mut()) };
            if code.is_null()
                || !(1..=16384).contains(&length)
                || stack > 64
                || nlocals > 64
                || nlocals != 0 && types.is_null()
                || unsafe { clauses(header) } != 0
            {
                return Err("scalar-output header exceeds plain bounded IL contract".into());
            }
            let mut locals = Vec::new();
            for index in 0..nlocals as usize {
                let ty = unsafe { *types.add(index) };
                if ty.is_null() || unsafe { (self.byref)(ty) } != 0 {
                    return Err("scalar-output local type is missing/byref".into());
                }
                locals.push(match unsafe { (self.kind)(ty) } {
                    0x18 if unsafe { (self.type_class)(ty) } == intptr => Local::Address,
                    0xf => {
                        let element = unsafe { (self.pointer_type)(ty) };
                        let shape = self.shape(element, images)?;
                        if !matches!(shape, Shape::Scalar { .. })
                            || !matches!(unsafe { (self.kind)(element) }, 2..=0xd | 0x11)
                            || unsafe { (self.kind)(element) } == 0x11
                                && unsafe { (self.is_enum)((self.type_class)(element)) } == 0
                        {
                            return Err(
                                "scalar-output pointer local is not a primitive/enum cell".into()
                            );
                        }
                        Local::Pointer(shape)
                    }
                    _ => {
                        ordinary(ty)?;
                        Local::Plain
                    }
                });
            }
            let image = unsafe { (self.image)(owner) };
            prove(
                unsafe { std::slice::from_raw_parts(code, length as usize) },
                &arguments,
                &locals,
                stack as usize,
                |token, virtual_call| {
                    let callee = unsafe { resolve(image, token, ptr::null_mut()) };
                    if callee.is_null() {
                        return Err("scalar-output call token unresolved".into());
                    }
                    let class = unsafe { (self.class)(callee) };
                    let sig = unsafe { (self.signature)(callee) };
                    if class.is_null()
                        || sig.is_null()
                        || !images.contains(&unsafe { (self.image)(class) })
                    {
                        return Err("scalar-output dependency outside owned image family".into());
                    }
                    self.closed.concrete(class)?;
                    let instance = unsafe { (self.instance)(sig) } != 0;
                    let count = unsafe { (self.count)(sig) } as usize;
                    if count > 16 || virtual_call && !instance {
                        return Err("scalar-output dependency signature invalid".into());
                    }
                    let result = unsafe { (self.returns)(sig) };
                    let mut it = ptr::null_mut();
                    let params: Vec<_> = (0..count)
                        .map(|_| unsafe { (self.parameters)(sig, &mut it) })
                        .collect();
                    let name = unsafe { (self.name)(callee) };
                    let converter = class == intptr
                        && !name.is_null()
                        && unsafe { CStr::from_ptr(name) } == c"op_Explicit"
                        && !instance
                        && params.len() == 1
                        && !params[0].is_null()
                        && !result.is_null()
                        && unsafe { (self.kind)(params[0]) } == 0x18
                        && unsafe { (self.type_class)(params[0]) } == intptr
                        && unsafe { (self.byref)(params[0]) } == 0
                        && unsafe { (self.kind)(result) } == 0xf
                        && unsafe { (self.byref)(result) } == 0;
                    if converter {
                        let element = unsafe { (self.pointer_type)(result) };
                        let mut implementation = 0;
                        let flags = unsafe { (self.flags)(callee, &mut implementation) };
                        if element.is_null()
                            || unsafe { (self.kind)(element) } != 1
                            || !super::managed_il_flags(flags, implementation, false)
                            || unsafe { (self.generic)(callee) } != 0
                            || unsafe { (self.inflated)(callee) } != 0
                        {
                            return Err(
                                "scalar-output actual corlib converter contract mismatch".into()
                            );
                        }
                    } else {
                        if instance && unsafe { (self.value)(class) } != 0 {
                            return Err("scalar-output dependency has an unboxed receiver".into());
                        }
                        for ty in params {
                            ordinary(ty)?;
                        }
                        if result.is_null() || unsafe { (self.byref)(result) } != 0 {
                            return Err("scalar-output dependency return invalid".into());
                        }
                        if unsafe { (self.kind)(result) } != 1 {
                            ordinary(result)?;
                        }
                    }
                    Ok(Call {
                        inputs: count + usize::from(instance),
                        result: !result.is_null() && unsafe { (self.kind)(result) } != 1,
                        converter,
                    })
                },
                |token, is_static| {
                    let mut declaring = ptr::null_mut();
                    let field =
                        unsafe { resolve_field(image, token, &mut declaring, ptr::null_mut()) };
                    if field.is_null()
                        || declaring.is_null()
                        || !images.contains(&unsafe { (self.image)(declaring) })
                        || (unsafe { field_flags(field) } & 0x10 != 0) != is_static
                    {
                        return Err("scalar-output field metadata mismatch".into());
                    }
                    ordinary(unsafe { field_type(field) })
                },
            )
        })();
        unsafe { (self.free)(header) };
        let outputs = body?;
        let name = unsafe { (self.name)(method) };
        if name.is_null() {
            return Err("scalar-output method name absent".into());
        }
        Ok(Some(ScalarOutputDescriptor {
            call: Descriptor {
                receiver: None,
                parameters,
                returns: None,
                name: unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned(),
            },
            outputs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{Memory, READ, WRITE};
    fn shape(width: usize) -> Shape {
        Shape::Scalar {
            class: width as Object,
            width,
            alignment: width,
            boolean: width == 1,
        }
    }
    fn calls(token: u32, _: bool) -> Result<Call, String> {
        match token {
            17 => Ok(Call {
                inputs: 1,
                result: true,
                converter: true,
            }),
            19 => Ok(Call {
                inputs: 1,
                result: false,
                converter: false,
            }),
            _ => Err("unknown fixture dependency".into()),
        }
    }
    fn prove_fixture(code: &[u8]) -> Result<Vec<(usize, Shape)>, String> {
        prove(
            code,
            &[Value::Address(0), Value::Plain],
            &[Local::Pointer(shape(1))],
            4,
            calls,
            |_, _| Err("unknown field".into()),
        )
    }
    #[test]
    fn complete_cfg_requires_definite_outputs_and_exact_pointer_provenance() {
        let code = [
            2, 0x28, 17, 0, 0, 0, 0x0a, 3, 0x2c, 5, 6, 0x17, 0x52, 0x2b, 3, 6, 0x16, 0x52, 0x2a,
        ];
        assert_eq!(prove_fixture(&code).unwrap(), vec![(0, shape(1))]);
        for end in 0..code.len() {
            assert!(prove_fixture(&code[..end]).is_err());
        }
        let mut missing = code;
        missing[9] = 8; // false branch returns without writing
        assert!(prove_fixture(&missing).is_err());
        let mut inside_operand = code;
        inside_operand[9] = 2;
        assert!(prove_fixture(&inside_operand).is_err());
        let mut looped = code;
        looped[9] = (-10i8) as u8;
        assert!(prove_fixture(&looped).is_err());
        let mut wrong_width = code;
        wrong_width[12] = 0x54;
        assert!(prove_fixture(&wrong_width).is_err());
    }
    #[test]
    fn pointers_cannot_escape_be_read_or_enter_numeric_operations() {
        let prefix = [2, 0x28, 17, 0, 0, 0, 0x0a];
        for tail in [
            vec![6, 0x4a, 0x26, 6, 0x17, 0x52, 0x2a],
            vec![6, 0x17, 0x58, 0x26, 6, 0x17, 0x52, 0x2a],
            vec![6, 0x28, 19, 0, 0, 0, 6, 0x17, 0x52, 0x2a],
            vec![6, 0x26, 6, 0x17, 0x52, 0x2a],
        ] {
            assert!(prove_fixture(&[prefix.as_slice(), tail.as_slice()].concat()).is_err());
        }
        assert!(
            prove_fixture(&[prefix.as_slice(), &[6, 0x17, 0x52, 0x2a, 0x2a]].concat()).is_err()
        );
    }
    #[test]
    fn pointer_parameters_at_different_positions_keep_distinct_typed_outputs() {
        let code = [
            3, 0x28, 17, 0, 0, 0, 0x0a, 4, 0x28, 17, 0, 0, 0, 0x0b, 6, 0x17, 0x52, 7, 0x17, 0x54,
            0x2a,
        ];
        assert_eq!(
            prove(
                &code,
                &[Value::Plain, Value::Address(1), Value::Address(2)],
                &[Local::Pointer(shape(1)), Local::Pointer(shape(4))],
                3,
                calls,
                |_, _| unreachable!()
            )
            .unwrap(),
            vec![(1, shape(1)), (2, shape(4))]
        );
    }

    // Run separately in one process, using the frozen no-JIT runtime and an
    // independently compiled fixture. Never replaces game corelib/support.
    #[test]
    #[ignore = "requires original ScalarOutputsProbe.dll and separately built no-JIT Mono"]
    fn actual_native_metadata_and_il_use_private_output_cells() {
        use std::ffi::CString;
        let library_path =
            CString::new(std::env::var("WIRE_MONO_OUTPUT_LIBRARY").unwrap()).unwrap();
        let classlib = CString::new(std::env::var("WIRE_MONO_OUTPUT_CLASSLIB").unwrap()).unwrap();
        let fixture = CString::new(std::env::var("WIRE_MONO_OUTPUT_FIXTURE").unwrap()).unwrap();
        let library =
            unsafe { libc::dlopen(library_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        assert!(!library.is_null());
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(library, concat!($name, "\0").as_ptr().cast()) };
                assert!(!p.is_null(), "missing {}", $name);
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let dirs = symbol!(
            "mono_set_dirs",
            unsafe extern "C" fn(*const libc::c_char, *const libc::c_char)
        );
        let config = symbol!(
            "mono_config_parse",
            unsafe extern "C" fn(*const libc::c_char)
        );
        let mode = symbol!("mono_jit_set_aot_mode", unsafe extern "C" fn(i32));
        let prepare = symbol!(
            "wire_mono_prepare_memory_adapter",
            unsafe extern "C" fn(u32) -> i32
        );
        let init = symbol!(
            "mono_jit_init_version",
            unsafe extern "C" fn(*const libc::c_char, *const libc::c_char) -> Object
        );
        let open = symbol!(
            "mono_domain_assembly_open",
            unsafe extern "C" fn(Object, *const libc::c_char) -> Object
        );
        let image = symbol!(
            "mono_assembly_get_image",
            unsafe extern "C" fn(Object) -> Object
        );
        let corlib = symbol!("mono_get_corlib", unsafe extern "C" fn() -> Object);
        let find = symbol!(
            "mono_class_from_name",
            unsafe extern "C" fn(Object, *const libc::c_char, *const libc::c_char) -> Object
        );
        let method = symbol!(
            "mono_class_get_method_from_name",
            unsafe extern "C" fn(Object, *const libc::c_char, i32) -> Object
        );
        let cleanup = symbol!("mono_jit_cleanup", unsafe extern "C" fn(Object));
        unsafe {
            dirs(classlib.as_ptr(), classlib.as_ptr());
            config(ptr::null());
            mode(5);
        }
        assert_eq!(unsafe { prepare(1) }, 0);
        if std::env::var("WIRE_MONO_UNITY_GC_PROFILE").as_deref() == Ok("1") {
            let options = symbol!(
                "mono_jit_parse_options",
                unsafe extern "C" fn(i32, *mut *const libc::c_char)
            );
            let mut arguments = [c"--gc-params=incremental=3".as_ptr()];
            unsafe { options(1, arguments.as_mut_ptr()) };
        }
        let domain = unsafe {
            init(
                c"wire-scalar-output-fixture".as_ptr(),
                c"v4.0.30319".as_ptr(),
            )
        };
        assert!(!domain.is_null());
        let assembly = unsafe { open(domain, fixture.as_ptr()) };
        assert!(!assembly.is_null());
        let image = unsafe { image(assembly) };
        let family = [image, unsafe { corlib() }];
        let owner = unsafe { find(image, c"".as_ptr(), c"ScalarOutputsProbe".as_ptr()) };
        assert!(!owner.is_null());
        let api = Api::load(library).unwrap();
        if let Ok(core) = std::env::var("WIRE_MONO_OUTPUT_CORE") {
            let core = CString::new(core).unwrap();
            let assembly = unsafe { open(domain, core.as_ptr()) };
            assert!(!assembly.is_null());
            let get_image = symbol!(
                "mono_assembly_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let core_image = unsafe { get_image(assembly) };
            let family = [image, family[1], core_image];
            let opaque = unsafe { find(image, c"".as_ptr(), c"OpaqueReferenceCell".as_ptr()) };
            let allocate = symbol!(
                "mono_object_new",
                unsafe extern "C" fn(Object, Object) -> Object
            );
            let retain = symbol!(
                "mono_gchandle_new_v2",
                unsafe extern "C" fn(Object, i32) -> Object
            );
            let release = symbol!("mono_gchandle_free_v2", unsafe extern "C" fn(Object));
            let field = symbol!(
                "mono_class_get_field_from_name",
                unsafe extern "C" fn(Object, *const libc::c_char) -> Object
            );
            let offset = symbol!("mono_field_get_offset", unsafe extern "C" fn(Object) -> i32);
            let object = unsafe { allocate(domain, opaque) };
            assert!(!object.is_null());
            let root = unsafe { retain(object, 1) };
            let reference = unsafe { field(opaque, c"Cached".as_ptr()) };
            let pointer = unsafe { field(opaque, c"Native".as_ptr()) };
            let reference_offset = unsafe { offset(reference) } as usize;
            let specs = crate::x64::process::mono_startup::field_layouts::reference_cell_at(
                library,
                core_image,
                &family,
                object,
                opaque,
                reference_offset,
            )
            .unwrap();
            assert_eq!(specs.len(), 1);
            assert_eq!(specs[0].name, c"Cached");
            assert_eq!(specs[0].kind, 0xe);
            assert!(
                crate::x64::process::mono_startup::field_layouts::reference_cell_at(
                    library,
                    core_image,
                    &family,
                    object,
                    opaque,
                    unsafe { offset(pointer) } as usize
                )
                .unwrap()
                .is_empty()
            );
            assert!(
                crate::x64::process::mono_startup::field_layouts::reference_cell_at(
                    library,
                    core_image,
                    &family,
                    object,
                    opaque,
                    reference_offset + 1
                )
                .is_err()
            );
            use crate::x64::process::mono_startup::{instances, reference_arrays};
            let root_api = instances::Api::load(library, core_image).unwrap();
            for (name, expected) in [
                (
                    c"ThrowNested",
                    "System.MissingMethodException: \"missing original fixture entry\"",
                ),
                (c"ThrowDeep", "[inner exception depth limit]"),
                (c"ThrowCycle", "[inner exception cycle]"),
            ] {
                let target = unsafe { method(owner, name.as_ptr(), 0) };
                let mut exception = ptr::null_mut();
                assert!(unsafe {
                    (api.invoke)(target, ptr::null_mut(), ptr::null_mut(), &mut exception)
                }
                .is_null());
                assert!(!exception.is_null());
                let diagnostic = api.exception_diagnostic(root_api, exception).unwrap();
                assert!(diagnostic.contains(expected), "{diagnostic}");
                eprintln!("PASS exception chain: {diagnostic}");
            }
            let mut roots = instances::Registry::default();
            let object_token = roots.borrow(root_api, object).unwrap();
            let new_array = symbol!(
                "mono_array_new",
                unsafe extern "C" fn(Object, Object, usize) -> Object
            );
            let array = unsafe { new_array(domain, opaque, 1) };
            assert!(api.reference_array_result(array).unwrap());
            assert!(!api.reference_array_result(object).unwrap());
            assert!(!api.reference_array_result(ptr::null_mut()).unwrap());
            let array_token = roots.borrow(root_api, array).unwrap();
            let array_api = reference_arrays::Api::load(
                library,
                [image, family[1], core_image, image, family[1], core_image],
                &family,
            )
            .unwrap();
            let mut arrays = reference_arrays::Registry::default();
            let borrowed = arrays
                .borrow_with(
                    array_api,
                    root_api,
                    &mut roots,
                    array,
                    |registry, native| registry.borrow(root_api, native),
                )
                .unwrap();
            assert_eq!(borrowed, array_token);
            let mut memory = Memory::default();
            arrays.publish(&mut memory, array_token).unwrap();
            let address = arrays.address(array_token, 8, 0).unwrap();
            assert_eq!(address, array_token + 32);
            assert!(arrays.has_reference(0, address));
            assert!(memory.check(array_token, 16, READ).is_err());
            arrays
                .store(
                    array_api,
                    root_api,
                    &roots,
                    &mut memory,
                    [0, address, object_token],
                )
                .unwrap();
            let array_address = symbol!(
                "mono_array_addr_with_size",
                unsafe extern "C" fn(Object, i32, usize) -> Object
            );
            assert_eq!(
                unsafe { array_address(array, 8, 0).cast::<Object>().read() },
                object
            );
            assert!(arrays
                .store(
                    array_api,
                    root_api,
                    &roots,
                    &mut memory,
                    [0, address, u64::MAX]
                )
                .is_err());
            assert_eq!(memory.read_int(address, 8).unwrap(), object_token);
            let collect = symbol!("mono_gc_collect", unsafe extern "C" fn(i32));
            let generation = symbol!("mono_gc_max_generation", unsafe extern "C" fn() -> i32);
            unsafe { collect(generation()) };
            roots.verify(root_api).unwrap();
            arrays.verify(array_api, root_api, &roots).unwrap();
            roots.release(root_api);
            arrays.retire(&mut memory, &roots).unwrap();
            roots.retire_views(&mut memory).unwrap();
            unsafe { release(root) };
        }
        for (name, count, expected) in [
            (c"Boolean", 1, 1),
            (c"Mode", 2, 3),
            (c"Mode", 2, 0),
            (c"Wide", 2, 0xfedc_ba98_7654_3210),
            (c"Pair", 2, 42),
        ] {
            let target = unsafe { method(owner, name.as_ptr(), count) };
            let descriptor = api
                .describe_scalar_outputs(target, &family)
                .unwrap()
                .unwrap();
            let mut memory = Memory::default();
            memory.map(0x1000, 4096, READ | WRITE).unwrap();
            for index in 0..count as usize {
                memory
                    .write_int(0x1000 + index as u64 * 8, 8, 0x1100 + index as u64 * 8)
                    .unwrap();
                memory
                    .write_int(
                        0x1100 + index as u64 * 8,
                        8,
                        if descriptor.outputs.iter().any(|o| o.0 == index) {
                            0x1200
                        } else if name == c"Wide" {
                            expected
                        } else if name == c"Mode" && expected == 0 {
                            0
                        } else {
                            7
                        },
                    )
                    .unwrap();
            }
            memory.write_int(0x1200, 8, 0xcccc_cccc_cccc_cccc).unwrap();
            let mut frame = super::super::Frame::prepare(
                &memory,
                &descriptor.call,
                0x1000,
                |_, _| unreachable!(),
                |_, _| false,
            )
            .unwrap();
            frame
                .prepare_scalar_outputs(&memory, 0x1000, &descriptor.outputs, |a, _| a == 0x1200)
                .unwrap();
            let mut parameters = frame.pointers();
            let mut exception = ptr::null_mut();
            let result = unsafe {
                (api.invoke)(
                    target,
                    ptr::null_mut(),
                    parameters.as_mut_ptr(),
                    &mut exception,
                )
            };
            assert!(
                result.is_null() && exception.is_null(),
                "fixture {:?}",
                name
            );
            let outputs = frame.outputs().unwrap();
            assert_eq!(outputs.len(), 1); // Pair aliases share the actual cell.
            assert_eq!(outputs[0].word, expected);
            assert_eq!(memory.read_int(0x1200, 8).unwrap(), 0xcccc_cccc_cccc_cccc);
        }
        for (name, count) in [
            (c"ReadBeforeWrite", 1),
            (c"Offset", 1),
            (c"Escaping", 1),
            (c"Partial", 2),
        ] {
            assert!(
                api.describe_scalar_outputs(
                    unsafe { method(owner, name.as_ptr(), count) },
                    &family
                )
                .is_err(),
                "unsafe fixture {:?}",
                name
            );
        }
        // Optional read-only scan of the actual engine family: no game methods
        // are invoked here, but every query is checked before game acceptance.
        if let Ok(core) = std::env::var("WIRE_MONO_OUTPUT_CORE") {
            let imgui = std::path::Path::new(&core)
                .parent()
                .unwrap()
                .join("UnityEngine.IMGUIModule.dll");
            let core = CString::new(core).unwrap();
            let assembly = unsafe { open(domain, core.as_ptr()) };
            assert!(!assembly.is_null());
            let get_image = symbol!(
                "mono_assembly_get_image",
                unsafe extern "C" fn(Object) -> Object
            );
            let core_image = unsafe { get_image(assembly) };
            let family = [core_image, family[1]];
            let target_class = unsafe {
                find(
                    core_image,
                    c"UnityEngine.Rendering".as_ptr(),
                    c"RenderTargetIdentifier".as_ptr(),
                )
            };
            let layout = crate::x64::process::mono_startup::render_targets::Layout::describe(
                library,
                core_image,
                target_class,
            )
            .unwrap()
            .unwrap();
            let mut bytes = vec![0; layout.width];
            layout.canonicalize(&mut bytes).unwrap();
            let owner = unsafe {
                find(
                    core_image,
                    c"UnityEngine.Rendering".as_ptr(),
                    c"SupportedRenderingFeatures".as_ptr(),
                )
            };
            let methods = symbol!(
                "mono_class_get_methods",
                unsafe extern "C" fn(Object, *mut Object) -> Object
            );
            let mut iterator = ptr::null_mut();
            let mut checked = 0;
            loop {
                let target = unsafe { methods(owner, &mut iterator) };
                if target.is_null() {
                    break;
                }
                match api.describe_scalar_outputs(target, &family) {
                    Ok(Some(_)) => checked += 1,
                    Ok(None) => {}
                    Err(reason) => panic!("actual query {:?}: {reason}", unsafe {
                        CStr::from_ptr((api.name)(target))
                    }),
                }
            }
            assert_eq!(checked, 11);
            eprintln!("PASS actual engine metadata: {checked} scalar-output queries and render-target layout");
            if imgui.is_file() {
                let path = CString::new(imgui.to_str().unwrap()).unwrap();
                let assembly = unsafe { open(domain, path.as_ptr()) };
                assert!(!assembly.is_null());
                let imgui = unsafe { get_image(assembly) };
                let event = unsafe { find(imgui, c"UnityEngine".as_ptr(), c"Event".as_ptr()) };
                assert!(!event.is_null());
                let intptr = unsafe { find(family[1], c"System".as_ptr(), c"IntPtr".as_ptr()) };
                let integer = unsafe { find(family[1], c"System".as_ptr(), c"Int32".as_ptr()) };
                let void = unsafe { find(family[1], c"System".as_ptr(), c"Void".as_ptr()) };
                let family = [core_image, family[1], imgui];
                api.fixed_typed_icall(
                    unsafe { method(event, c"Internal_Create".as_ptr(), 1) },
                    event,
                    &[(0x18, intptr, false), (8, integer, false)],
                    &family,
                )
                .unwrap();
                api.fixed_typed_icall(
                    unsafe { method(event, c"Internal_Destroy".as_ptr(), 1) },
                    event,
                    &[(1, void, false), (0x18, intptr, false)],
                    &family,
                )
                .unwrap();
                eprintln!(
                    "PASS actual cross-module Event metadata: Int32 factory and IntPtr destructor"
                );
            }
        }
        unsafe { cleanup(domain) };
    }
}
