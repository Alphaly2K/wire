//! Derive generated array-result protocols from actual closed Unmarshal<T>
//! calls, local types and complete finally boundaries. No MethodSpec constants.
use super::{
    engine_wrapper_instructions, engine_wrapper_operand_width, managed_il_flags, Api,
    ExceptionClause, NumericSpanLayout, Object,
};
use std::{ffi::CStr, ptr};

fn load(i: (u8, u32)) -> Option<u32> {
    match i {
        (op @ 6..=9, _) => Some((op - 6) as u32),
        (0x11, n) => Some(n),
        _ => None,
    }
}
fn store(i: (u8, u32)) -> Option<u32> {
    match i {
        (op @ 0xa..=0xd, _) => Some((op - 0xa) as u32),
        (0x13, n) => Some(n),
        _ => None,
    }
}
fn tail(code: &[u8], target: u32, clause: ExceptionClause) -> Option<(u32, u32, u32, u32)> {
    let all = engine_wrapper_instructions(code)?;
    let mut offsets = Vec::new();
    let mut offset = 0usize;
    for &(op, _) in &all {
        offsets.push(offset);
        offset += 1 + engine_wrapper_operand_width(op)?;
    }
    let compact: Vec<_> = all.iter().enumerate().filter(|(_, i)| i.0 != 0).collect();
    if clause.flags != 2
        || compact
            .iter()
            .filter(|(_, i)| **i == (0x28, target))
            .count()
            != 1
    {
        return None;
    }
    let at = compact.iter().position(|(_, i)| **i == (0x28, target))?;
    let &(0x12, native) = compact.get(at.checked_sub(1)?)?.1 else {
        return None;
    };
    let t = compact.get(at..)?;
    if t.len() != 10 || *t[2].1 != (0x12, native) || t[3].1 .0 != 0x12 {
        return None;
    }
    let array = t[3].1 .1;
    let (0x28, converter) = *t[4].1 else {
        return None;
    };
    let result = store(*t[6].1)?;
    let delta = match *t[1].1 {
        (0xde, n) => n as u8 as i8 as i64,
        (0xdd, n) => n as i32 as i64,
        _ => return None,
    };
    let handler = offsets[t[1].0] + 1 + engine_wrapper_operand_width(t[1].1 .0)?;
    let done = offsets[t[8].0];
    if converter >> 24 != 0x2b
        || native == array
        || native == result
        || array == result
        || load(*t[5].1) != Some(array)
        || load(*t[8].1) != Some(result)
        || *t[7].1 != (0xdc, 0)
        || *t[9].1 != (0x2a, 0)
        || clause.handler_offset as usize != handler
        || clause.handler_offset.checked_add(clause.handler_len)? as usize != done
        || clause.try_offset.checked_add(clause.try_len)? as usize != handler
        || clause.try_offset as usize > offsets[compact[at - 1].0]
        || handler as i64 + delta != done as i64
    {
        return None;
    }
    // InitLocals establishes a zero output header and null array. Prefix code
    // may prepare other arguments but must never prefill/access these locals.
    for &(index, i) in &compact[..at - 1] {
        let local = load(*i)
            .or_else(|| store(*i))
            .or_else(|| (i.0 == 0x12).then_some(i.1));
        if local.is_some_and(|n| [native, array, result].contains(&n))
            || matches!(i.0, 0x2a | 0xdd | 0xde | 0xdc)
        {
            return None;
        }
        if matches!(i.0, 0x2b..=0x44) {
            let delta = if i.0 < 0x38 {
                i.1 as u8 as i8 as i64
            } else {
                i.1 as i32 as i64
            };
            let end = offsets[index] + 1 + engine_wrapper_operand_width(i.0)?;
            let destination = usize::try_from(end as i64 + delta).ok()?;
            if destination <= offsets[index]
                || destination > offsets[compact[at - 1].0]
                || offsets.binary_search(&destination).is_err()
            {
                return None;
            }
        }
    }
    Some((native, array, result, converter))
}

impl Api {
    pub(in crate::x64::process::mono_startup) fn engine_blittable_output(
        self,
        method: Object,
        owner: Object,
        wrapper: Object,
        images: &[Object],
    ) -> Result<Option<NumericSpanLayout>, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let p = unsafe { libc::dlsym(self.library, concat!($name, "\0").as_ptr().cast()) };
                if p.is_null() {
                    return Err(concat!("missing array-output metadata API: ", $name).into());
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
        let clause_next = sym!(
            "mono_method_header_get_clauses",
            unsafe extern "C" fn(Object, Object, *mut Object, *mut ExceptionClause) -> i32
        );
        let element = sym!(
            "mono_class_get_element_class",
            unsafe extern "C" fn(Object) -> Object
        );
        let class_type = sym!(
            "mono_class_get_type",
            unsafe extern "C" fn(Object) -> Object
        );
        let rank = sym!("mono_class_get_rank", unsafe extern "C" fn(Object) -> u32);
        let table = sym!(
            "mono_image_get_table_info",
            unsafe extern "C" fn(Object, i32) -> Object
        );
        let rows = sym!(
            "mono_table_info_get_rows",
            unsafe extern "C" fn(Object) -> i32
        );
        let column = sym!(
            "mono_metadata_decode_row_col",
            unsafe extern "C" fn(Object, i32, u32) -> u32
        );
        let image = unsafe { (self.image)(owner) };
        let target = unsafe { token(method) };
        if wrapper.is_null()
            || target >> 24 != 6
            || !images.contains(&image)
            || !images.contains(&unsafe { (self.image)(wrapper) })
        {
            return Ok(None);
        }
        let signature = unsafe { (self.signature)(method) };
        if signature.is_null()
            || unsafe { (self.instance)(signature) } != 0
            || !(1..=16).contains(&unsafe { (self.count)(signature) })
        {
            return Ok(None);
        }
        let arity = unsafe { (self.count)(signature) };
        let return_type = unsafe { (self.returns)(signature) };
        if return_type.is_null()
            || unsafe { (self.kind)(return_type) } != 1
            || unsafe { (self.byref)(return_type) } != 0
        {
            return Ok(None);
        }
        let mut parameters = ptr::null_mut();
        let mut last = ptr::null_mut();
        for _ in 0..arity {
            last = unsafe { (self.parameters)(signature, &mut parameters) };
        }
        if last.is_null()
            || unsafe { (self.byref)(last) } == 0
            || unsafe { (self.kind)(last) } != 0x11
            || unsafe { (self.type_class)(last) } != wrapper
        {
            return Ok(None);
        }
        let methods_table = unsafe { table(image, 6) };
        let param_table = unsafe { table(image, 8) };
        if methods_table.is_null() || param_table.is_null() {
            return Err("array-output parameter tables absent".into());
        }
        let row = (target & 0xffffff) as i32 - 1;
        let method_rows = unsafe { rows(methods_table) };
        let param_rows = unsafe { rows(param_table) };
        if row < 0 || row >= method_rows || !(0..=1_000_000).contains(&param_rows) {
            return Err("array-output parameter table bounds invalid".into());
        }
        let first = unsafe { column(methods_table, row, 5) };
        let end = if row + 1 < method_rows {
            unsafe { column(methods_table, row + 1, 5) }
        } else {
            param_rows as u32 + 1
        };
        if first == 0 || end < first || end - first > 17 || end > param_rows as u32 + 1 {
            return Err("array-output ParamList bounds invalid".into());
        }
        let mut attributes = None;
        for r in first..end {
            if unsafe { column(param_table, r as i32 - 1, 1) } == arity {
                if attributes.is_some() {
                    return Err("duplicate array-output parameter sequence".into());
                }
                attributes = Some(unsafe { column(param_table, r as i32 - 1, 0) });
            }
        }
        if attributes.is_none_or(|a| a & 3 != 2) {
            return Ok(None);
        }
        let exact = |ty: Object, kind, class, reference| {
            !ty.is_null()
                && unsafe { (self.kind)(ty) } == kind
                && unsafe { (self.type_class)(ty) } == class
                && (unsafe { (self.byref)(ty) } != 0) == reference
        };
        let mut result = None;
        let mut iterator = ptr::null_mut();
        loop {
            let candidate = unsafe { methods(owner, &mut iterator) };
            if candidate.is_null() {
                break;
            }
            let signature = unsafe { (self.signature)(candidate) };
            let mut implementation = 0;
            let flags = unsafe { (self.flags)(candidate, &mut implementation) };
            if signature.is_null()
                || !managed_il_flags(
                    flags,
                    implementation,
                    unsafe { (self.instance)(signature) } != 0,
                )
                || unsafe { (self.generic)(candidate) } != 0
                || unsafe { (self.inflated)(candidate) } != 0
            {
                continue;
            }
            let returns = unsafe { (self.returns)(signature) };
            if returns.is_null()
                || unsafe { (self.kind)(returns) } != 0x1d
                || unsafe { (self.byref)(returns) } != 0
            {
                continue;
            }
            let array = unsafe { (self.type_class)(returns) };
            if array.is_null()
                || unsafe { rank(array) } != 1
                || self.closed.concrete(array).is_err()
            {
                continue;
            }
            let header = unsafe { (self.header)(candidate) };
            if header.is_null() {
                continue;
            }
            let (mut length, mut stack, mut count, mut init) = (0, 0, 0, 0);
            let code = unsafe { (self.code)(header, &mut length, &mut stack) };
            let types = unsafe { locals(header, &mut count, &mut init) };
            let mut iter = ptr::null_mut();
            let mut clause = ExceptionClause::default();
            let proof = if code.is_null()
                || !(20..=768).contains(&length)
                || stack > 32
                || count > 32
                || types.is_null()
                || init == 0
                || unsafe { clauses(header) } != 1
                || unsafe { clause_next(header, candidate, &mut iter, &mut clause) } == 0
            {
                None
            } else {
                tail(
                    unsafe { std::slice::from_raw_parts(code, length as usize) },
                    target,
                    clause,
                )
            };
            let proved_tail = proof.is_some();
            let inferred = proof.and_then(|(native, copied, returned, helper)| {
                if [native, copied, returned].iter().any(|&n| n >= count)
                    || !exact(unsafe { *types.add(native as usize) }, 0x11, wrapper, false)
                    || !exact(unsafe { *types.add(copied as usize) }, 0x1d, array, false)
                    || !exact(unsafe { *types.add(returned as usize) }, 0x1d, array, false)
                {
                    return None;
                }
                let helper = unsafe { resolve(image, helper, owner) };
                if helper.is_null()
                    || unsafe { (self.class)(helper) } != wrapper
                    || unsafe { (self.inflated)(helper) } == 0
                {
                    return None;
                }
                let name = unsafe { (self.name)(helper) };
                let signature = unsafe { (self.signature)(helper) };
                let mut implementation = 0;
                let flags = unsafe { (self.flags)(helper, &mut implementation) };
                if name.is_null()
                    || unsafe { CStr::from_ptr(name) } != c"Unmarshal"
                    || signature.is_null()
                    || !managed_il_flags(flags, implementation, true)
                    || unsafe { (self.instance)(signature) } == 0
                    || unsafe { (self.count)(signature) } != 1
                {
                    return None;
                }
                let mut iter = ptr::null_mut();
                let parameter = unsafe { (self.parameters)(signature, &mut iter) };
                let ret = unsafe { (self.returns)(signature) };
                if !exact(parameter, 0x1d, array, true)
                    || ret.is_null()
                    || unsafe { (self.kind)(ret) } != 1
                    || unsafe { (self.byref)(ret) } != 0
                {
                    return None;
                }
                let element = unsafe { element(array) };
                if element.is_null() || !images.contains(&unsafe { (self.image)(element) }) {
                    return None;
                }
                let ty = unsafe { class_type(element) };
                if ty.is_null() {
                    return None;
                }
                let (stride, alignment) = match unsafe { (self.kind)(ty) } {
                    4 | 5 => (1, 1),
                    3 | 6 | 7 => (2, 2),
                    8 | 9 | 12 => (4, 4),
                    0xa | 0xb | 13 => (8, 8),
                    0x11 | 0x15 => super::super::value_layout::Api::load(self.library, unsafe {
                        (self.image)(element)
                    })
                    .ok()?
                    .layout_in(element, images)
                    .ok()?
                    .scalar_inline()
                    .ok()?,
                    _ => return None,
                };
                Some(NumericSpanLayout {
                    element: element as usize,
                    stride,
                    alignment,
                })
            });
            unsafe { (self.free)(header) };
            if proved_tail && inferred.is_none() {
                return Ok(None);
            }
            if let Some(inferred) = inferred {
                if result.is_some_and(|prior| prior != inferred) {
                    return Ok(None);
                }
                result = Some(inferred);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn array_tail_requires_unique_closed_converter_and_complete_finally() {
        let target = 0x06000123u32;
        let helper = 0x2b000017u32;
        let mut code = vec![0x12, 0, 0x28];
        code.extend(target.to_le_bytes());
        code.extend([0xde, 12, 0x12, 0, 0x12, 1, 0x28]);
        code.extend(helper.to_le_bytes());
        code.extend([7, 0xc, 0xdc, 8, 0x2a]);
        let clause = ExceptionClause {
            flags: 2,
            try_offset: 0,
            try_len: 9,
            handler_offset: 9,
            handler_len: 12,
            data: 0,
        };
        assert_eq!(tail(&code, target, clause), Some((0, 1, 2, helper)));
        for at in [1, 8, 10, 12, 20, 21, 22] {
            let mut changed = code.clone();
            changed[at] = 0xff;
            assert!(tail(&changed, target, clause).is_none());
        }
        for length in 0..code.len() {
            assert!(tail(&code[..length], target, clause).is_none());
        }
        let mut changed = clause;
        changed.handler_len += 1;
        assert!(tail(&code, target, changed).is_none());
        let mut changed = vec![0x12, 1, 0x26];
        changed.extend(&code);
        let clause = ExceptionClause {
            try_len: 12,
            handler_offset: 12,
            ..clause
        };
        assert!(tail(&changed, target, clause).is_none());
    }
}
