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
use crate::api::ApiState;
use crate::mem::GuestMem;
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
pub fn stack_bytes(name: &str) -> Option<u32> {
    static EXPORTS: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();
    EXPORTS
        .get_or_init(|| {
            include_str!("../third_party/wine-10.0/gdiplus/gdiplus.spec")
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
        .get(name)
        .copied()
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
fn pointer(mem: &GuestMem, p: u32, len: u32, write: bool) -> Result<(), u32> {
    if p == 0 {
        return Err(INVALID);
    }
    mem.ptr(p, len, write).map(|_| ()).map_err(|_| INVALID)
}
fn read_floats<const N: usize>(mem: &GuestMem, p: u32) -> Result<[f32; N], u32> {
    pointer(mem, p, N as u32 * 4, false)?;
    let mut result = [0.0; N];
    for (i, value) in result.iter_mut().enumerate() {
        *value = f32::from_bits(mem.read_u32(p + i as u32 * 4).map_err(|_| INVALID)?);
    }
    Ok(result)
}
fn write_matrix(mem: &GuestMem, p: u32, m: [f32; 6]) -> Result<(), u32> {
    pointer(mem, p, 24, true)?;
    let bytes: Vec<_> = m
        .into_iter()
        .flat_map(|f| f.to_bits().to_le_bytes())
        .collect();
    mem.write_bytes(p, &bytes).map_err(|_| INVALID)
}
fn matrix(mem: &GuestMem, state: &ApiState, p: u32) -> Result<[f32; 6], u32> {
    if !state.gdiplus.matrices.contains(&p) {
        return Err(INVALID);
    }
    read_floats(mem, p)
}
fn create_matrix(
    mem: &mut GuestMem,
    state: &mut ApiState,
    output: u32,
    values: [f32; 6],
) -> Result<(), u32> {
    pointer(mem, output, 4, true)?;
    let p = state
        .heap
        .alloc(mem, state.process_heap, 24, false)
        .map_err(|_| OUT_OF_MEMORY)?;
    if p == 0 {
        return Err(OUT_OF_MEMORY);
    }
    if let Err(error) =
        write_matrix(mem, p, values).and_then(|_| mem.write_u32(output, p).map_err(|_| INVALID))
    {
        state.heap.free(state.process_heap, p);
        return Err(error);
    }
    state.gdiplus.matrices.insert(p);
    Ok(())
}
fn output_bool(mem: &GuestMem, p: u32, value: bool) -> Result<(), u32> {
    pointer(mem, p, 4, true)?;
    mem.write_u32(p, u32::from(value)).map_err(|_| INVALID)
}

/// Return (value, diagnostic, implemented), preserving the distinction between
/// exported ABI and working functionality in the API trace.
#[allow(clippy::assign_op_pattern)] // Preserve Wine's floating-point operand order.
pub fn dispatch(
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> (u32, String, bool) {
    let mut implemented = true;
    let result = (|| -> Result<u32, u32> {
        match name {
            "GdiplusStartup" => {
                pointer(mem, a[0], 4, true)?;
                pointer(mem, a[1], 16, false)?;
                let version = mem.read_u32(a[1]).map_err(|_| INVALID)?;
                if !(1..=2).contains(&version) {
                    return Err(UNSUPPORTED_VERSION);
                }
                if mem.read_u32(a[1] + 8).map_err(|_| INVALID)? != 0 {
                    if a[2] == 0 {
                        return Err(INVALID);
                    }
                    implemented = false;
                    return Err(NOT_IMPLEMENTED); // guest notification hooks not yet available
                }
                let token = state
                    .gdiplus
                    .next_token
                    .checked_add(1)
                    .ok_or(OUT_OF_MEMORY)?;
                state.gdiplus.next_token = token;
                state.gdiplus.tokens.insert(token);
                mem.write_u32(a[0], token).map_err(|_| INVALID)?;
            }
            "GdiplusShutdown" => {
                state.gdiplus.tokens.remove(&a[0]);
            }
            "GdipAlloc" => {
                let p = state
                    .heap
                    .alloc(mem, state.process_heap, a[0], true)
                    .map_err(|_| OUT_OF_MEMORY)?;
                if p != 0 {
                    state.gdiplus.allocations.insert(p);
                }
                return Ok(p);
            }
            "GdipFree" => {
                if state.gdiplus.allocations.remove(&a[0]) {
                    state.heap.free(state.process_heap, a[0]);
                }
            }
            "GdipCreateMatrix" => create_matrix(mem, state, a[0], IDENTITY)?,
            "GdipCreateMatrix2" => create_matrix(
                mem,
                state,
                a[6],
                std::array::from_fn(|i| f32::from_bits(a[i])),
            )?,
            "GdipCreateMatrix3" | "GdipCreateMatrix3I" => {
                pointer(mem, a[0], 16, false)?;
                pointer(mem, a[1], 24, false)?;
                let mut rect = read_floats::<4>(mem, a[0])?;
                let mut points = read_floats::<6>(mem, a[1])?;
                if name.ends_with('I') {
                    for (i, v) in rect.iter_mut().enumerate() {
                        *v = mem.read_u32(a[0] + i as u32 * 4).map_err(|_| INVALID)? as i32 as f32;
                    }
                    for (i, v) in points.iter_mut().enumerate() {
                        *v = mem.read_u32(a[1] + i as u32 * 4).map_err(|_| INVALID)? as i32 as f32;
                    }
                }
                let m11 = (points[2] - points[0]) / rect[2];
                let m21 = (points[4] - points[0]) / rect[3];
                let m12 = (points[3] - points[1]) / rect[2];
                let m22 = (points[5] - points[1]) / rect[3];
                create_matrix(
                    mem,
                    state,
                    a[2],
                    [
                        m11,
                        m12,
                        m21,
                        m22,
                        points[0] - m11 * rect[0] - m21 * rect[1],
                        points[1] - m12 * rect[0] - m22 * rect[1],
                    ],
                )?;
            }
            "GdipCloneMatrix" => {
                let m = matrix(mem, state, a[0])?;
                create_matrix(mem, state, a[1], m)?;
            }
            "GdipDeleteMatrix" => {
                if !state.gdiplus.matrices.remove(&a[0]) {
                    return Err(INVALID);
                }
                state.heap.free(state.process_heap, a[0]);
            }
            "GdipGetMatrixElements" => {
                let m = matrix(mem, state, a[0])?;
                write_matrix(mem, a[1], m)?;
            }
            "GdipSetMatrixElements" => {
                matrix(mem, state, a[0])?;
                write_matrix(mem, a[0], std::array::from_fn(|i| f32::from_bits(a[i + 1])))?;
            }
            "GdipIsMatrixIdentity" | "GdipIsMatrixEqual" | "GdipIsMatrixInvertible" => {
                let m = matrix(mem, state, a[0])?;
                if name == "GdipIsMatrixInvertible" {
                    output_bool(mem, a[1], invertible(m))?;
                } else {
                    let (other, output) = if name == "GdipIsMatrixIdentity" {
                        (IDENTITY, a[1])
                    } else {
                        (matrix(mem, state, a[1])?, a[2])
                    };
                    output_bool(mem, output, m.map(f32::to_bits) == other.map(f32::to_bits))?;
                }
            }
            "GdipInvertMatrix" => {
                let mut m = matrix(mem, state, a[0])?;
                if !invertible(m) {
                    return Err(INVALID);
                }
                if m[1] == 0.0 && m[2] == 0.0 {
                    m[4] = -m[4] / m[0];
                    m[5] = -m[5] / m[3];
                    m[0] = 1.0 / m[0];
                    m[3] = 1.0 / m[3];
                } else {
                    let d = 1.0 / determinant(m);
                    m = [
                        m[3] * d,
                        -m[1] * d,
                        -m[2] * d,
                        m[0] * d,
                        (m[2] * m[5] - m[3] * m[4]) * d,
                        -(m[0] * m[5] - m[1] * m[4]) * d,
                    ];
                }
                write_matrix(mem, a[0], m)?;
            }
            "GdipMultiplyMatrix"
            | "GdipTranslateMatrix"
            | "GdipScaleMatrix"
            | "GdipShearMatrix"
            | "GdipRotateMatrix" => {
                let mut m = matrix(mem, state, a[0])?;
                let order = if name == "GdipMultiplyMatrix" || name == "GdipRotateMatrix" {
                    a[2]
                } else {
                    a[3]
                };
                if order > 1 {
                    return Err(INVALID);
                }
                let x = f32::from_bits(a[1]);
                let y = f32::from_bits(a[2]);
                match name {
                    "GdipTranslateMatrix" => {
                        if order == 1 {
                            m[4] += x;
                            m[5] += y;
                        } else {
                            m[4] = x * m[0] + y * m[2] + m[4];
                            m[5] = x * m[1] + y * m[3] + m[5];
                        }
                    }
                    "GdipScaleMatrix" => {
                        if order == 1 {
                            m[0] *= x;
                            m[1] *= y;
                            m[2] *= x;
                            m[3] *= y;
                            m[4] *= x;
                            m[5] *= y;
                        } else {
                            m[0] *= x;
                            m[1] *= x;
                            m[2] *= y;
                            m[3] *= y;
                        }
                    }
                    _ => {
                        let rhs = match name {
                            "GdipMultiplyMatrix" => matrix(mem, state, a[1])?,
                            "GdipShearMatrix" => [1.0, y, x, 1.0, 0.0, 0.0],
                            _ => {
                                let radians = (std::f64::consts::PI * x as f64 / 180.0) as f32;
                                let c = (radians as f64).cos() as f32;
                                let s = (radians as f64).sin() as f32;
                                [c, s, -s, c, 0.0, 0.0]
                            }
                        };
                        m = if order == 1 {
                            multiply(m, rhs)
                        } else {
                            multiply(rhs, m)
                        };
                    }
                }
                write_matrix(mem, a[0], m)?;
            }
            "GdipTransformMatrixPoints"
            | "GdipTransformMatrixPointsI"
            | "GdipVectorTransformMatrixPoints"
            | "GdipVectorTransformMatrixPointsI" => {
                let m = matrix(mem, state, a[0])?;
                let count = a[2] as i32;
                if count <= 0 {
                    return Err(INVALID);
                }
                let len = (count as u32).checked_mul(8).ok_or(INVALID)?;
                pointer(mem, a[1], len, true)?;
                let integer = name.ends_with('I');
                let vector = name.starts_with("GdipVector");
                for i in 0..count as u32 {
                    let p = a[1] + i * 8;
                    let xb = mem.read_u32(p).map_err(|_| INVALID)?;
                    let yb = mem.read_u32(p + 4).map_err(|_| INVALID)?;
                    let x = if integer {
                        xb as i32 as f32
                    } else {
                        f32::from_bits(xb)
                    };
                    let y = if integer {
                        yb as i32 as f32
                    } else {
                        f32::from_bits(yb)
                    };
                    let dx = x * m[0] + y * m[2];
                    let dy = x * m[1] + y * m[3];
                    let dx = if vector { dx } else { dx + m[4] };
                    let dy = if vector { dy } else { dy + m[5] };
                    let bits = |v: f32| {
                        if integer {
                            let rounded = ((v as f64 + 0.5) as f32).floor() as f64;
                            if rounded < i32::MIN as f64
                                || rounded > i32::MAX as f64
                                || !rounded.is_finite()
                            {
                                i32::MIN as u32
                            } else {
                                rounded as i32 as u32
                            }
                        } else {
                            v.to_bits()
                        }
                    };
                    mem.write_u32(p, bits(dx)).map_err(|_| INVALID)?;
                    mem.write_u32(p + 4, bits(dy)).map_err(|_| INVALID)?;
                }
            }
            _ => {
                implemented = false;
                return Err(NOT_IMPLEMENTED);
            }
        }
        Ok(OK)
    })();
    let value = result.unwrap_or_else(|e| if name == "GdipAlloc" { 0 } else { e });
    let note = if implemented {
        format!("GDI+ status/value {value}")
    } else {
        "GDI+ NotImplemented; function not ported".into()
    };
    (value, note, implemented)
}
