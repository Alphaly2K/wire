//! Derive the engine header read from the complete original managed body.
use super::{Api, Object, Shape};
use std::ptr;

#[derive(Clone, Copy)]
pub(in crate::x64::process::mono_startup) struct HeaderRead {
    pub(in crate::x64::process::mono_startup) method: Object,
    pub(in crate::x64::process::mono_startup) owner: Object,
    pub(in crate::x64::process::mono_startup) offset: Object,
}
struct Body {
    cached: u32,
    zero: u32,
    equal: u32,
    explicit: u32,
    offset: u32,
    value_helpers: Option<(u32, u32)>,
}

/// Accept only the complete null-or-pointer-plus-static-offset I4 read. Branch
/// destinations, local writes and all operands are part of the proof. Nops
/// may vary; tokens are resolved from the current image, never stored specs.
fn body(code: &[u8]) -> Option<Body> {
    let mut instructions = Vec::new();
    let mut boundaries = std::collections::BTreeSet::new();
    let mut pc = 0;
    while pc < code.len() {
        let at = pc;
        boundaries.insert(at);
        let op = *code.get(pc)?;
        pc += 1;
        let arg = match op {
            0x7b | 0x7e | 0x28 => {
                let token = u32::from_le_bytes(code.get(pc..pc + 4)?.try_into().ok()?);
                pc += 4;
                token
            }
            0x2b | 0x2c => {
                let delta = *code.get(pc)? as i8 as i32;
                pc += 1;
                u32::try_from(pc as i32 + delta).ok()?
            }
            0x00 | 0x02 | 0x06 | 0x07 | 0x0a | 0x0b | 0x16 | 0x2a | 0x4a | 0x58 => 0,
            _ => return None,
        };
        if op != 0 {
            instructions.push((at, op, arg));
        }
    }
    let mut expected = vec![
        0x02, 0x7b, 0x7e, 0x28, 0x0a, 0x06, 0x2c, 0x16, 0x0b, 0x2b, 0x02, 0x7b, 0x28, 0x7e, 0x58,
        0x4a, 0x0b, 0x2b, 0x07, 0x2a,
    ];
    let value = instructions.get(7)?.1 == 0x28;
    if value {
        expected[7] = 0x28;
        expected.insert(16, 0x28);
    }
    if instructions.len() != expected.len()
        || instructions.iter().zip(expected).any(|(i, op)| i.1 != op)
        || instructions[1].2 != instructions[11].2
    {
        return None;
    }
    let shift = usize::from(value);
    for (branch, target) in [(6, 10), (9, 18 + shift), (17 + shift, 18 + shift)] {
        let destination = instructions[branch].2 as usize;
        let next = instructions[target].0;
        if !boundaries.contains(&destination)
            || destination > next
            || code.get(destination..next)?.iter().any(|b| *b != 0)
        {
            return None;
        }
    }
    Some(Body {
        cached: instructions[1].2,
        zero: instructions[2].2,
        equal: instructions[3].2,
        explicit: instructions[12].2,
        offset: instructions[13].2,
        value_helpers: value.then(|| (instructions[7].2, instructions[16].2)),
    })
}

impl Api {
    pub(in crate::x64::process::mono_startup) fn engine_header_read(
        self,
        method: Object,
        owner: Object,
        images: &[Object],
    ) -> Result<HeaderRead, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing engine header metadata API: ", $name).into());
                }
                unsafe { std::mem::transmute::<Object, $ty>(p) }
            }};
        }
        let descriptor = self
            .describe(method, images, images)?
            .ok_or("engine header read has no IL")?;
        let returns = unsafe { (self.returns)((self.signature)(method)) };
        let return_kind = unsafe { (self.kind)(returns) };
        let return_class = unsafe { (self.type_class)(returns) };
        if descriptor.receiver != Some(owner)
            || !descriptor.parameters.is_empty()
            || !matches!(
                descriptor.returns,
                Some(Shape::Scalar {
                    width: 4,
                    boolean: false,
                    ..
                })
            )
            || !matches!(return_kind, 8 | 0x11)
        {
            return Err("engine header read is not an instance four-byte getter".into());
        }
        let clauses = sym!(
            "mono_method_header_get_num_clauses",
            unsafe extern "C" fn(Object) -> i32
        );
        let locals = sym!(
            "mono_method_header_get_locals",
            unsafe extern "C" fn(Object, *mut u32, *mut i32) -> *const Object
        );
        let header = unsafe { (self.header)(method) };
        if header.is_null() {
            return Err("engine header IL is absent".into());
        }
        let (mut length, mut stack, mut count) = (0, 0, 0);
        let code = unsafe { (self.code)(header, &mut length, &mut stack) };
        let types = unsafe { locals(header, &mut count, ptr::null_mut()) };
        let valid = unsafe { clauses(header) } == 0
            && count == 2
            && !types.is_null()
            && [2, return_kind].iter().enumerate().all(|(index, kind)| {
                let ty = unsafe { *types.add(index) };
                !ty.is_null()
                    && unsafe { (self.kind)(ty) } == *kind
                    && unsafe { (self.byref)(ty) } == 0
                    && (index == 0 || unsafe { (self.type_class)(ty) } == return_class)
            });
        let read = if valid && !code.is_null() && length <= 256 && stack <= 8 {
            body(unsafe { std::slice::from_raw_parts(code, length as usize) })
        } else {
            None
        };
        unsafe { (self.free)(header) };
        let read = read.ok_or("engine header getter has an unverified complete body")?;
        if read.value_helpers.is_some() != (return_kind == 0x11) {
            return Err("engine header null/conversion branch does not match result type".into());
        }
        let field = sym!(
            "mono_field_from_token",
            unsafe extern "C" fn(Object, u32, *mut Object, Object) -> Object
        );
        let find_field = sym!(
            "mono_class_get_field_from_name",
            unsafe extern "C" fn(Object, *const super::c_char) -> Object
        );
        let parent = sym!(
            "mono_field_get_parent",
            unsafe extern "C" fn(Object) -> Object
        );
        let field_type = sym!(
            "mono_field_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let flags = sym!("mono_field_get_flags", unsafe extern "C" fn(Object) -> u32);
        let resolve = sym!(
            "mono_get_method",
            unsafe extern "C" fn(Object, u32, Object) -> Object
        );
        let intptr = sym!("mono_get_intptr_class", unsafe extern "C" fn() -> Object);
        let intptr = unsafe { intptr() };
        let image = unsafe { (self.image)(owner) };
        if let Some((none, convert)) = read.value_helpers {
            // Both helpers keep executing their original IL. Qualify their
            // exact declared signatures; only the caller's I4 read is bridged.
            for (token, name, arity) in [(none, c"get_None", 0), (convert, c"op_Implicit", 1)] {
                let helper = unsafe { resolve(image, token, ptr::null_mut()) };
                if helper.is_null()
                    || unsafe { (self.class)(helper) } != return_class
                    || unsafe { (self.generic)(helper) } != 0
                    || unsafe { (self.inflated)(helper) } != 0
                    || unsafe { (self.name)(helper) }.is_null()
                    || unsafe { std::ffi::CStr::from_ptr((self.name)(helper)) } != name
                {
                    return Err("engine header value helper identity mismatch".into());
                }
                let desc = self
                    .describe(helper, images, images)?
                    .ok_or("value helper has no IL")?;
                if desc.receiver.is_some()
                    || desc.parameters.len() != arity
                    || desc.returns != descriptor.returns
                    || desc.parameters.iter().any(|p| {
                        p.byref
                            || !matches!(
                                p.shape,
                                Shape::Scalar {
                                    width: 4,
                                    boolean: false,
                                    ..
                                }
                            )
                    })
                {
                    return Err("engine header value helper signature mismatch".into());
                }
                if arity == 1 {
                    let mut iterator = ptr::null_mut();
                    let arg = unsafe { (self.parameters)((self.signature)(helper), &mut iterator) };
                    if arg.is_null() || unsafe { (self.kind)(arg) } != 8 {
                        return Err("engine header value conversion does not consume I4".into());
                    }
                }
            }
        }
        let resolve_field = |token| {
            let mut declaring = ptr::null_mut();
            // Mono writes retklass on both cached and MemberRef resolution.
            unsafe { field(image, token, &mut declaring, ptr::null_mut()) }
        };
        let cached = resolve_field(read.cached);
        let zero = resolve_field(read.zero);
        let offset = resolve_field(read.offset);
        for (f, class, kind, is_static) in [
            (cached, owner, 0x18, false),
            (zero, intptr, 0x18, true),
            (offset, owner, 8, true),
        ] {
            if f.is_null() || unsafe { parent(f) } != class {
                return Err("engine header field has the wrong owner".into());
            }
            let ty = unsafe { field_type(f) };
            if ty.is_null()
                || unsafe { (self.kind)(ty) } != kind
                || unsafe { (self.byref)(ty) } != 0
                || (unsafe { flags(f) } & 0x10 != 0) != is_static
                || unsafe { flags(f) } & 0x40 != 0
            {
                return Err("engine header field type/static contract mismatch".into());
            }
        }
        if cached != unsafe { find_field(owner, c"m_CachedPtr".as_ptr()) }
            || zero != unsafe { find_field(intptr, c"Zero".as_ptr()) }
            || offset
                != unsafe { find_field(owner, c"OffsetOfInstanceIDInCPlusPlusObject".as_ptr()) }
        {
            return Err("engine header body reads different semantic fields".into());
        }
        for (token, name, result, arguments) in [
            (read.equal, c"op_Equality", 2, vec![0x18, 0x18]),
            (read.explicit, c"op_Explicit", 0x0f, vec![0x18]),
        ] {
            let helper = unsafe { resolve(image, token, ptr::null_mut()) };
            if helper.is_null()
                || unsafe { (self.class)(helper) } != intptr
                || unsafe { (self.generic)(helper) } != 0
                || unsafe { (self.inflated)(helper) } != 0
                || unsafe { (self.name)(helper) }.is_null()
                || unsafe { std::ffi::CStr::from_ptr((self.name)(helper)) } != name
            {
                return Err("engine header helper identity mismatch".into());
            }
            let sig = unsafe { (self.signature)(helper) };
            if sig.is_null()
                || unsafe { (self.instance)(sig) } != 0
                || unsafe { (self.count)(sig) } != arguments.len() as u32
            {
                return Err("engine header helper is not the required static signature".into());
            }
            let ret = unsafe { (self.returns)(sig) };
            if ret.is_null()
                || unsafe { (self.kind)(ret) } != result
                || unsafe { (self.byref)(ret) } != 0
            {
                return Err("engine header helper return mismatch".into());
            }
            if result == 0x0f {
                let target = unsafe { (self.pointer_type)(ret) };
                if target.is_null() || unsafe { (self.kind)(target) } != 1 {
                    return Err("engine header conversion does not return void pointer".into());
                }
            }
            let mut iterator = ptr::null_mut();
            for kind in arguments {
                let ty = unsafe { (self.parameters)(sig, &mut iterator) };
                if ty.is_null()
                    || unsafe { (self.kind)(ty) } != kind
                    || unsafe { (self.byref)(ty) } != 0
                    || unsafe { (self.type_class)(ty) } != intptr
                {
                    return Err("engine header helper parameter mismatch".into());
                }
            }
        }
        Ok(HeaderRead {
            method,
            owner,
            offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut code = vec![0, 2, 0x7b];
        code.extend(0x04000123u32.to_le_bytes());
        code.push(0x7e);
        code.extend(0x0a000045u32.to_le_bytes());
        code.push(0x28);
        code.extend(0x0a000020u32.to_le_bytes());
        code.extend([0x0a, 6, 0x2c, 4, 0x16, 0x0b, 0x2b, 22, 0, 2, 0x7b]);
        code.extend(0x04000123u32.to_le_bytes());
        code.push(0x28);
        code.extend(0x0a000010u32.to_le_bytes());
        code.push(0x7e);
        code.extend(0x04000345u32.to_le_bytes());
        code.extend([0x58, 0x4a, 0x0b, 0x2b, 0, 7, 0x2a]);
        code
    }
    #[test]
    fn engine_header_requires_complete_null_branch_and_exact_read_provenance() {
        let code = fixture();
        let read = body(&code).unwrap();
        assert_eq!(read.cached, 0x04000123);
        assert_eq!(read.offset, 0x04000345);
        for (index, replacement) in [(20, 0), (24, 1), (28, 0x24), (43, 0x58), (44, 6), (46, 1)] {
            let mut invalid = code.clone();
            invalid[index] = replacement;
            assert!(body(&invalid).is_none(), "mutated byte {index}");
        }
        for end in 0..code.len() {
            assert!(body(&code[..end]).is_none());
        }
        let mut extended = code.clone();
        extended.push(0x2a);
        assert!(body(&extended).is_none());
        let mut shifted = code.clone();
        for at in [3, 28] {
            shifted[at..at + 4].copy_from_slice(&0x04005678u32.to_le_bytes());
        }
        shifted[38..42].copy_from_slice(&0x04009876u32.to_le_bytes());
        assert_eq!(body(&shifted).unwrap().cached, 0x04005678);
        assert_eq!(body(&shifted).unwrap().offset, 0x04009876);
    }
    #[test]
    fn engine_header_value_result_preserves_both_original_helper_calls() {
        let mut code = fixture();
        code.splice(21..22, [0x28, 0x12, 0x34, 0x56, 0x06]);
        code[20] = 8;
        code[28] = 27;
        code.splice(48..48, [0x28, 0x78, 0x9a, 0xbc, 0x06]);
        let read = body(&code).unwrap();
        assert_eq!(read.value_helpers, Some((0x06563412, 0x06bc9a78)));
        assert_eq!(read.cached, 0x04000123);
        assert_eq!(read.offset, 0x04000345);
        for (at, replacement) in [(20, 4), (28, 22), (48, 0x6f), (47, 0x4b), (54, 1)] {
            let mut invalid = code.clone();
            invalid[at] = replacement;
            assert!(body(&invalid).is_none(), "mutated byte {at}");
        }
        for end in 0..code.len() {
            assert!(body(&code[..end]).is_none());
        }
    }
}
