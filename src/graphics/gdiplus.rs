/*
 * Copyright (C) 2007 Google (Evan Stade)
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */

//! Adapted from Wine 10.0 dlls/gdiplus/{matrix.c,gdiplus.c,gdiplus_private.h,gdiplus.spec}.
//! Matrix math/order/status behavior is ported; allocation, pointer validation
//! and lifetime tracking use wire's checked guest memory. Unported exports
//! return NotImplemented, never a successful GpStatus without an object.
pub(crate) trait Backend {
    type Error: From<u32>;
    fn check(&self, p: u32, length: usize, write: bool) -> Result<(), Self::Error>;
    fn read_u32(&self, p: u32) -> Result<u32, Self::Error>;
    fn write_u32(&mut self, p: u32, value: u32) -> Result<(), Self::Error>;
    fn write_bytes(&mut self, p: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn allocate(&mut self, size: u32) -> Result<u32, Self::Error>;
    fn free(&mut self, p: u32) -> Result<(), Self::Error>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Export {
    pub name: &'static str,
    pub stack: u32,
}
impl Export {
    pub(crate) fn lookup(name: &str) -> Option<Self> {
        exports()
            .get_key_value(name)
            .map(|(&name, &stack)| Self { name, stack })
    }
}

#[path = "gdiplus/calls.rs"]
mod calls;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const OK: u32 = 0;
const INVALID: u32 = 2;
const OUT_OF_MEMORY: u32 = 3;
const NOT_IMPLEMENTED: u32 = 6;
const UNSUPPORTED_VERSION: u32 = 17;
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[derive(Default)]
pub struct GdiPlus {
    matrices: HashSet<u32>,
    allocations: HashSet<u32>,
    tokens: HashSet<u32>,
    next_token: u32,
}

/// Wine's x86 export specification determines stack cleanup even for APIs
/// whose functionality is not ported. Stubs without a declared ABI are absent.
fn exports() -> &'static HashMap<&'static str, u32> {
    static EXPORTS: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    EXPORTS.get_or_init(|| {
        include_str!("../../third_party/wine-10.0/gdiplus/gdiplus.spec")
            .lines()
            .filter_map(|line| {
                let (signature, _) = line.split_once(')')?;
                let (head, args) = signature.split_once('(')?;
                let mut head = head.split_whitespace();
                head.next()?;
                if head.next()? != "stdcall" {
                    return None;
                }
                let name = head.next()?;
                let bytes = args
                    .split_whitespace()
                    .map(|arg| match arg {
                        "int128" => 16,
                        "ptr" | "long" | "float" | "str" | "wstr" => 4,
                        _ => panic!("unrecognized Wine gdiplus ABI type {arg}"),
                    })
                    .sum();
                Some((name, bytes))
            })
            .collect()
    })
}
// Wine matrix_multiply: row vectors, with translation in the third row.
fn multiply(left: [f32; 6], right: [f32; 6]) -> [f32; 6] {
    [
        left[0] * right[0] + left[1] * right[2],
        left[0] * right[1] + left[1] * right[3],
        left[2] * right[0] + left[3] * right[2],
        left[2] * right[1] + left[3] * right[3],
        left[4] * right[0] + left[5] * right[2] + right[4],
        left[4] * right[1] + left[5] * right[3] + right[5],
    ]
}
fn determinant(m: [f32; 6]) -> f32 {
    m[0] * m[3] - m[1] * m[2]
}
fn invertible(m: [f32; 6]) -> bool {
    if m[1] == 0.0 && m[2] == 0.0 {
        m[0] != 0.0 && m[3] != 0.0
    } else {
        // Wine's fabs promotes REAL to double before comparing with 1e-5.
        (determinant(m) as f64).abs() >= 1e-5
    }
}
fn pointer<B: Backend>(mem: &B, p: u32, len: u32, write: bool) -> Result<(), B::Error> {
    if p == 0 {
        return Err(INVALID.into());
    }
    mem.check(p, len as usize, write)
}
fn read_floats<const N: usize, B: Backend>(mem: &B, p: u32) -> Result<[f32; N], B::Error> {
    pointer(mem, p, N as u32 * 4, false)?;
    let mut result = [0.0; N];
    for (i, value) in result.iter_mut().enumerate() {
        *value = f32::from_bits(mem.read_u32(p + i as u32 * 4)?);
    }
    Ok(result)
}
fn write_matrix<B: Backend>(mem: &mut B, p: u32, m: [f32; 6]) -> Result<(), B::Error> {
    pointer(mem, p, 24, true)?;
    let bytes: Vec<_> = m
        .into_iter()
        .flat_map(|f| f.to_bits().to_le_bytes())
        .collect();
    mem.write_bytes(p, &bytes)
}
fn matrix<B: Backend>(mem: &B, state: &GdiPlus, p: u32) -> Result<[f32; 6], B::Error> {
    if !state.matrices.contains(&p) {
        return Err(INVALID.into());
    }
    read_floats::<6, B>(mem, p)
}
fn create_matrix<B: Backend>(
    mem: &mut B,
    state: &mut GdiPlus,
    output: u32,
    values: [f32; 6],
) -> Result<(), B::Error> {
    pointer(mem, output, 4, true)?;
    let p = mem.allocate(24)?;
    if p == 0 {
        return Err(OUT_OF_MEMORY.into());
    }
    if let Err(error) = write_matrix(mem, p, values).and_then(|_| mem.write_u32(output, p)) {
        mem.free(p)?;
        return Err(error);
    }
    state.matrices.insert(p);
    Ok(())
}
fn output_bool<B: Backend>(mem: &mut B, p: u32, value: bool) -> Result<(), B::Error> {
    pointer(mem, p, 4, true)?;
    mem.write_u32(p, u32::from(value))
}
