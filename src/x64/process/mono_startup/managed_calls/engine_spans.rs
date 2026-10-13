//! Prove generated pinned numeric-array wrappers using their actual locals,
//! closed Span<T> methods and complete acyclic IL. A 16B span alone does not
//! establish its element type, direction, pinning or pointer provenance.
use super::{
    engine_argument_flow, engine_wrapper_instructions, engine_wrapper_operand_width,
    managed_il_flags, Api, Object,
};
use std::{ffi::CStr, ptr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::x64::process::mono_startup) struct NumericSpanLayout {
    pub(in crate::x64::process::mono_startup) element: usize,
    pub(in crate::x64::process::mono_startup) stride: usize,
    pub(in crate::x64::process::mono_startup) alignment: usize,
}
#[derive(Clone, Copy)]
struct Preparation {
    start: usize,
    end: usize,
    position: usize,
    span: u32,
    pinned: u32,
    wrapper: u32,
    layout: NumericSpanLayout,
}
fn local_load(op: u8, operand: u32) -> Option<u32> {
    match op {
        0x06..=0x09 => Some((op - 6) as u32),
        0x11 => Some(operand),
        _ => None,
    }
}
fn local_store(op: u8, operand: u32) -> Option<u32> {
    match op {
        0x0a..=0x0d => Some((op - 0x0a) as u32),
        0x13 => Some(operand),
        _ => None,
    }
}
fn argument(op: u8, operand: u32) -> Option<usize> {
    match op {
        0x02..=0x05 => Some((op - 2) as usize),
        0x0e => Some(operand as usize),
        _ => None,
    }
}
fn numeric_preparations(
    code: &[u8],
    target: u32,
    mut verify: impl FnMut(usize, u32, u32, u32, u32, [u32; 4]) -> Option<NumericSpanLayout>,
) -> Option<Vec<Preparation>> {
    let all = engine_wrapper_instructions(code)?;
    if all.len() > 256 {
        return None;
    }
    let compact: Vec<_> = all
        .iter()
        .enumerate()
        .filter(|(_, &(op, _))| op != 0)
        .collect();
    let target_at = all
        .iter()
        .position(|&(op, token)| op == 0x28 && token == target)?;
    let mut result = Vec::new();
    for sequence in compact.windows(12) {
        let i: Vec<_> = sequence
            .iter()
            .map(|(_, instruction)| **instruction)
            .collect();
        let (0x12, span) = i[0] else { continue };
        let Some(position) = argument(i[1].0, i[1].1) else {
            continue;
        };
        let (0x28, constructor) = i[2] else { continue };
        if i[3] != (0x12, span) {
            continue;
        }
        let (0x28, pinnable) = i[4] else { continue };
        let Some(pinned) = local_store(i[5].0, i[5].1) else {
            continue;
        };
        let (0x12, wrapper) = i[6] else { continue };
        if local_load(i[7].0, i[7].1) != Some(pinned)
            || i[8].0 != 0xe0 && i[8].0 != 0xd3
            || i[9] != (0x12, span)
        {
            continue;
        }
        let (0x28, length) = i[10] else { continue };
        let (0x28, wrapper_constructor) = i[11] else {
            continue;
        };
        // The next instruction starts forwarding arguments. Keep the complete
        // construction interval, including any NOPs, separate from that use.
        if span == pinned || span == wrapper || pinned == wrapper || sequence[11].0 >= target_at {
            return None;
        }
        let Some(layout) = verify(
            position,
            span,
            pinned,
            wrapper,
            target,
            [constructor, pinnable, length, wrapper_constructor],
        ) else {
            continue;
        };
        let prep = Preparation {
            start: sequence[0].0,
            end: sequence[11].0 + 1,
            position,
            span,
            pinned,
            wrapper,
            layout,
        };
        if result.iter().any(|p: &Preparation| {
            p.wrapper == wrapper
                || p.span == span
                || p.pinned == pinned
                || p.start < prep.end && prep.start < p.end
        }) {
            return None;
        }
        result.push(prep);
    }
    if result.is_empty() {
        return None;
    }
    let tail: Vec<_> = all[target_at + 1..]
        .iter()
        .copied()
        .filter(|&(op, _)| op != 0)
        .collect();
    if tail.len() != result.len() * 3 + 1 || tail.last() != Some(&(0x2a, 0)) {
        return None;
    }
    let mut retired = Vec::new();
    for sequence in tail[..tail.len() - 1].as_chunks::<3>().0 {
        let pinned = local_store(sequence[2].0, sequence[2].1)?;
        if sequence[0] != (0x16, 0)
            || !matches!(sequence[1].0, 0xd3 | 0xe0)
            || !result.iter().any(|prep| prep.pinned == pinned)
            || retired.contains(&pinned)
        {
            return None;
        }
        retired.push(pinned);
    }
    let mut offsets = Vec::new();
    let mut offset = 0;
    for &(op, _) in &all {
        offsets.push(offset);
        offset += 1 + engine_wrapper_operand_width(op)?;
    }
    for (at, &(op, operand)) in all[..target_at].iter().enumerate() {
        for prep in &result {
            if at >= prep.start && at < prep.end {
                continue;
            }
            // No other operation may read/write these local cells before the
            // target. Only loading the completed wrapper address is admitted.
            let accessed = local_load(op, operand)
                .or_else(|| local_store(op, operand))
                .or_else(|| (op == 0x12).then_some(operand));
            if accessed.is_some_and(|local| {
                local == prep.span
                    || local == prep.pinned
                    || local == prep.wrapper && (op != 0x12 || at < prep.end)
            }) {
                return None;
            }
        }
        if matches!(op, 0x2b..=0x2d | 0x38..=0x3a) {
            let delta = if op < 0x38 {
                operand as u8 as i8 as i64
            } else {
                operand as i32 as i64
            };
            let end = offsets[at] + 1 + engine_wrapper_operand_width(op)?;
            let next = offsets
                .binary_search(&usize::try_from(end as i64 + delta).ok()?)
                .ok()?;
            if next <= at || result.iter().any(|prep| at < prep.end && next > prep.start) {
                return None;
            }
        }
    }
    Some(result)
}

impl Api {
    pub(in crate::x64::process::mono_startup) fn engine_numeric_spans(
        self,
        method: Object,
        owner: Object,
        wrapper_span: Object,
        images: &[Object],
    ) -> Result<Vec<Option<NumericSpanLayout>>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing numeric span metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let methods = sym!(
            "mono_class_get_methods",
            unsafe extern "C" fn(Object, *mut Object) -> Object
        );
        let token = sym!("mono_method_get_token", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let locals = sym!(
            "mono_method_header_get_locals",
            unsafe extern "C" fn(Object, *mut u32, *mut i32) -> *const Object
        );
        let clauses = sym!(
            "mono_method_header_get_num_clauses",
            unsafe extern "C" fn(Object) -> i32
        );
        let is_pinned = sym!(
            "wire_mono_type_is_pinned",
            unsafe extern "C" fn(Object) -> i32
        );
        let element_class = sym!(
            "mono_class_get_element_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let class_name = sym!(
            "mono_class_get_name",
            unsafe extern "C" fn(Object) -> *const libc::c_char
        );
        let namespace = sym!(
            "mono_class_get_namespace",
            unsafe extern "C" fn(Object) -> *const libc::c_char
        );
        let arguments = sym!(
            "mono_unity_class_get_generic_argument_count",
            unsafe extern "C" fn(Object) -> u32
        );
        let type_argument = sym!(
            "mono_unity_class_get_generic_argument_at",
            unsafe extern "C" fn(Object, u32) -> Object
        );
        let corlib = sym!("mono_get_corlib", unsafe extern "C" fn() -> Object);
        let call_conv = sym!(
            "mono_signature_get_call_conv",
            unsafe extern "C" fn(Object) -> u32
        );
        let explicit_this = sym!(
            "mono_signature_explicit_this",
            unsafe extern "C" fn(Object) -> u32
        );
        let image = unsafe { (self.image)(owner) };
        let signature = unsafe { (self.signature)(method) };
        if signature.is_null() || !images.contains(&image) {
            return Err("numeric target metadata absent".into());
        }
        let arity = unsafe { (self.count)(signature) } as usize;
        let target = unsafe { token(method) };
        if arity > 16 || target >> 24 != 6 {
            return Ok(vec![None; arity]);
        }
        let corlib = unsafe { corlib() };
        let mut result = None;
        let mut iterator = ptr::null_mut();
        loop {
            let wrapper = unsafe { methods(owner, &mut iterator) };
            if wrapper.is_null() {
                break;
            }
            let sig = unsafe { (self.signature)(wrapper) };
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(wrapper, &mut implementation) };
            if sig.is_null()
                || !managed_il_flags(flags, implementation, unsafe { (self.instance)(sig) } != 0)
                || unsafe { (self.generic)(wrapper) } != 0
                || unsafe { (self.inflated)(wrapper) } != 0
            {
                continue;
            }
            let mut source = Vec::new();
            if unsafe { (self.instance)(sig) } != 0 {
                source.push(ptr::null_mut());
            }
            let mut parameter = ptr::null_mut();
            for _ in 0..unsafe { (self.count)(sig) } {
                source.push(unsafe { (self.parameters)(sig, &mut parameter) });
            }
            if source.len() != arity {
                continue;
            }
            let header = unsafe { (self.header)(wrapper) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack, mut count, mut init) = (0, 0, 0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let types = unsafe { locals(header, &mut count, &mut init) };
            let inferred = if code.is_null()
                || !(12..=768).contains(&length)
                || stack > 32
                || count > 32
                || types.is_null()
                || init == 0
                || unsafe { clauses(header) } != 0
            {
                None
            } else {
                let code = unsafe { std::slice::from_raw_parts(code, length as usize) };
                let preparations = numeric_preparations(
                    code,
                    target,
                    |position, span, pinned, cell, _, helpers| {
                        let input = *source.get(position)?;
                        if input.is_null()
                            || unsafe { (self.kind)(input) } != 0x1d
                            || unsafe { (self.byref)(input) } != 0
                            || [span, pinned, cell].iter().any(|&i| i >= count)
                        {
                            return None;
                        }
                        let array = unsafe { (self.type_class)(input) };
                        let element = unsafe { element_class(array) };
                        if element.is_null() {
                            return None;
                        }
                        let local_span = unsafe { *types.add(span as usize) };
                        let pinned = unsafe { *types.add(pinned as usize) };
                        let cell = unsafe { *types.add(cell as usize) };
                        if [local_span, pinned, cell].iter().any(|ty| ty.is_null()) {
                            return None;
                        }
                        let span_class = unsafe { (self.type_class)(local_span) };
                        let name = unsafe { class_name(span_class) };
                        let ns = unsafe { namespace(span_class) };
                        if name.is_null()
                            || ns.is_null()
                            || unsafe { CStr::from_ptr(name) } != c"Span`1"
                            || unsafe { CStr::from_ptr(ns) } != c"System"
                            || unsafe { (self.image)(span_class) } != corlib
                            || unsafe { arguments(span_class) } != 1
                            || unsafe { type_argument(span_class, 0) } != element
                            || self.closed.constructed(span_class).is_err()
                            || unsafe { (self.kind)(local_span) } != 0x15
                            || unsafe { (self.byref)(local_span) } != 0
                            || unsafe { (self.byref)(pinned) } == 0
                            || unsafe { is_pinned(pinned) } == 0
                            || unsafe { (self.type_class)(pinned) } != element
                            || unsafe { (self.byref)(cell) } != 0
                            || unsafe { (self.kind)(cell) } != 0x11
                            || unsafe { (self.type_class)(cell) } != wrapper_span
                        {
                            return None;
                        }
                        let element_type = unsafe { (self.kind)(pinned) };
                        let (stride, alignment) = match element_type {
                            4 | 5 => (1, 1),
                            3 | 6 | 7 => (2, 2),
                            8 | 9 | 12 => (4, 4),
                            0xa | 0xb | 13 => (8, 8),
                            0x11 | 0x15 => {
                                super::super::value_layout::Api::load(self.library, unsafe {
                                    (self.image)(element)
                                })
                                .ok()?
                                .layout_in(element, images)
                                .ok()?
                                .scalar_inline()
                                .ok()?
                            }
                            _ => return None,
                        };
                        let helpers = helpers.map(|t| unsafe { resolve(image, t, owner) });
                        for (index, &helper) in helpers.iter().enumerate() {
                            if helper.is_null() {
                                return None;
                            }
                            let mut implementation = 0;
                            let flags = unsafe { (self.flags)(helper, &mut implementation) };
                            let sig = unsafe { (self.signature)(helper) };
                            let name = unsafe { (self.name)(helper) };
                            let expected =
                                [c".ctor", c"GetPinnableReference", c"get_Length", c".ctor"][index];
                            if sig.is_null()
                                || name.is_null()
                                || unsafe { CStr::from_ptr(name) } != expected
                                || !managed_il_flags(flags, implementation, true)
                                || unsafe { (self.class)(helper) }
                                    != if index == 3 { wrapper_span } else { span_class }
                                || unsafe { (self.instance)(sig) } == 0
                                || unsafe { (self.count)(sig) } != [1, 0, 0, 2][index]
                                || unsafe { call_conv(sig) } != 0
                                || unsafe { explicit_this(sig) } != 0
                            {
                                return None;
                            }
                            let ret = unsafe { (self.returns)(sig) };
                            if ret.is_null()
                                || unsafe { (self.kind)(ret) } != [1, element_type, 8, 1][index]
                                || (unsafe { (self.byref)(ret) } != 0) != (index == 1)
                                || index == 1 && unsafe { (self.type_class)(ret) } != element
                            {
                                return None;
                            }
                            let mut parameter = ptr::null_mut();
                            if index == 0 {
                                let ty = unsafe { (self.parameters)(sig, &mut parameter) };
                                if ty.is_null()
                                    || unsafe { (self.kind)(ty) } != 0x1d
                                    || unsafe { (self.byref)(ty) } != 0
                                    || unsafe { (self.type_class)(ty) } != array
                                {
                                    return None;
                                }
                            } else if index == 3 {
                                let pointer = unsafe { (self.parameters)(sig, &mut parameter) };
                                let length = unsafe { (self.parameters)(sig, &mut parameter) };
                                if pointer.is_null()
                                    || length.is_null()
                                    || unsafe { (self.kind)(pointer) } != 0xf
                                    || unsafe { (self.byref)(pointer) } != 0
                                    || unsafe { (self.kind)(length) } != 8
                                    || unsafe { (self.byref)(length) } != 0
                                {
                                    return None;
                                }
                                let void = unsafe { (self.pointer_type)(pointer) };
                                if void.is_null() || unsafe { (self.kind)(void) } != 1 {
                                    return None;
                                }
                            }
                        }
                        Some(NumericSpanLayout {
                            element: element as usize,
                            stride,
                            alignment,
                        })
                    },
                );
                preparations.and_then(|prepared| {
                    engine_argument_flow(
                        code,
                        target,
                        arity,
                        |_, _| None,
                        |_| false,
                        |token| {
                            let called = unsafe { resolve(image, token, owner) };
                            if called.is_null() {
                                return None;
                            }
                            let class = unsafe { (self.class)(called) };
                            if class.is_null() || !images.contains(&unsafe { (self.image)(class) })
                            {
                                return None;
                            }
                            let sig = unsafe { (self.signature)(called) };
                            if sig.is_null()
                                || unsafe { call_conv(sig) } != 0
                                || unsafe { explicit_this(sig) } != 0
                            {
                                return None;
                            }
                            let ret = unsafe { (self.returns)(sig) };
                            if ret.is_null() {
                                return None;
                            }
                            let count = unsafe { (self.count)(sig) } as usize
                                + usize::from(unsafe { (self.instance)(sig) } != 0);
                            (count <= 16).then_some((count, unsafe { (self.kind)(ret) } != 1))
                        },
                        |at, local| {
                            prepared
                                .iter()
                                .find(|p| p.wrapper == local && at >= p.end)
                                .map(|p| (p.position, p.layout))
                        },
                    )
                })
            };
            unsafe { (self.free)(header) };
            let Some(inferred) = inferred else {
                continue;
            };
            if result
                .as_ref()
                .is_some_and(|previous| previous != &inferred)
            {
                return Ok(vec![None; arity]);
            }
            result = Some(inferred);
        }
        Ok(result.unwrap_or_else(|| vec![None; arity]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TARGET: u32 = 0x06000213;
    const HELPERS: [u32; 4] = [0x0a000053, 0x0a000097, 0x0a000008, 0x06000075];
    fn call(code: &mut Vec<u8>, token: u32) {
        code.push(0x28);
        code.extend_from_slice(&token.to_le_bytes());
    }
    fn wrapper() -> Vec<u8> {
        let mut code = vec![0x02, 0x12, 0, 0x03];
        call(&mut code, HELPERS[0]);
        code.extend([0x12, 0]);
        call(&mut code, HELPERS[1]);
        code.extend([0x0b, 0x12, 2, 0x07, 0xe0, 0x12, 0]);
        call(&mut code, HELPERS[2]);
        call(&mut code, HELPERS[3]);
        code.extend([0, 0x12, 2, 0x04]);
        call(&mut code, TARGET);
        code.extend([0x16, 0xe0, 0x0b, 0x2a]);
        code
    }
    fn layout() -> NumericSpanLayout {
        NumericSpanLayout {
            element: 7,
            stride: 16,
            alignment: 4,
        }
    }
    fn preparations(code: &[u8]) -> Option<Vec<Preparation>> {
        numeric_preparations(code, TARGET, |position, span, pin, wrapper, _, helpers| {
            (position == 1 && (span, pin, wrapper) == (0, 1, 2) && helpers == HELPERS)
                .then_some(layout())
        })
    }
    fn flow(code: &[u8], prepared: &[Preparation]) -> Option<Vec<Option<NumericSpanLayout>>> {
        engine_argument_flow(
            code,
            TARGET,
            3,
            |_, _| None,
            |_| false,
            |token| match HELPERS.iter().position(|&helper| helper == token) {
                Some(index) => Some([(2, false), (1, true), (1, true), (3, false)][index]),
                None if token == 0x06000155 => Some((1, false)),
                _ => None,
            },
            |at, local| {
                prepared
                    .iter()
                    .find(|p| p.wrapper == local && at >= p.end)
                    .map(|p| (p.position, p.layout))
            },
        )
    }
    #[test]
    fn generated_numeric_wrapper_tracks_actual_local_type_to_positional_target() {
        let code = wrapper();
        let prepared = preparations(&code).unwrap();
        assert_eq!(
            flow(&code, &prepared),
            Some(vec![None, Some(layout()), None])
        );
        let mut wrong_helper = code.clone();
        wrong_helper[5] ^= 1;
        assert!(preparations(&wrong_helper).is_none());
        let mut wrong_span = code.clone();
        wrong_span[10] = 3;
        assert!(preparations(&wrong_span).is_none());
        assert!(preparations(&code[..code.len() - 3]).is_none());
        let mut overwritten = code.clone();
        overwritten.splice(33..33, [0x16, 0x0c]);
        assert!(preparations(&overwritten).is_none());
        let mut skipped = vec![0x17, 0x2d, 34];
        skipped.extend(code);
        assert!(preparations(&skipped).is_none());
    }
    #[test]
    fn numeric_local_cannot_be_reused_after_an_unverified_mutating_call() {
        let mut code = wrapper();
        let mut mutate = vec![0x12, 2];
        call(&mut mutate, 0x06000155);
        code.splice(33..33, mutate);
        let prepared = preparations(&code).unwrap();
        assert!(flow(&code, &prepared).is_none());
        let mut swapped = wrapper();
        swapped[36] = 0x05;
        let prepared = preparations(&swapped).unwrap();
        assert!(flow(&swapped, &prepared).is_none());
    }
}
