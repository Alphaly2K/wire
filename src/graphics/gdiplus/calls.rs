//! GDI+ operations borrow their registry, guest memory and process heap explicitly.
use super::{
    create_matrix, determinant, invertible, matrix, multiply, output_bool, pointer, read_floats,
    write_matrix, Backend, GdiPlus, IDENTITY, INVALID, NOT_IMPLEMENTED, OK, OUT_OF_MEMORY,
    UNSUPPORTED_VERSION,
};

impl GdiPlus {
    #[allow(clippy::assign_op_pattern)] // Preserve Wine floating-point operand order.
    pub(crate) fn call<B: Backend>(
        &mut self,
        name: &str,
        a: &[u32; 16],
        mem: &mut B,
    ) -> Result<u32, B::Error> {
        match name {
            "GdiplusStartup" => {
                pointer(mem, a[0], 4, true)?;
                pointer(mem, a[1], 16, false)?;
                let version = mem.read_u32(a[1])?;
                if !(1..=2).contains(&version) {
                    return Err(UNSUPPORTED_VERSION.into());
                }
                if mem.read_u32(a[1] + 8)? != 0 {
                    if a[2] == 0 {
                        return Err(INVALID.into());
                    }
                    return Err(NOT_IMPLEMENTED.into()); // guest notification hooks not yet available
                }
                let token = self
                    .next_token
                    .checked_add(1)
                    .ok_or_else(|| B::Error::from(OUT_OF_MEMORY))?;
                mem.write_u32(a[0], token)?;
                self.next_token = token;
                self.tokens.insert(token);
            }
            "GdiplusShutdown" => {
                self.tokens.remove(&a[0]);
            }
            "GdipAlloc" => {
                let p = mem.allocate(a[0])?;
                if p != 0 {
                    self.allocations.insert(p);
                }
                return Ok(p);
            }
            "GdipFree" => {
                if self.allocations.contains(&a[0]) {
                    mem.free(a[0])?;
                    self.allocations.remove(&a[0]);
                }
            }
            "GdipCreateMatrix" => create_matrix(mem, self, a[0], IDENTITY)?,
            "GdipCreateMatrix2" => create_matrix(
                mem,
                self,
                a[6],
                std::array::from_fn(|i| f32::from_bits(a[i])),
            )?,
            "GdipCreateMatrix3" | "GdipCreateMatrix3I" => {
                pointer(mem, a[0], 16, false)?;
                pointer(mem, a[1], 24, false)?;
                let mut rect = read_floats::<4, B>(mem, a[0])?;
                let mut points = read_floats::<6, B>(mem, a[1])?;
                if name.ends_with('I') {
                    for (i, v) in rect.iter_mut().enumerate() {
                        *v = mem.read_u32(a[0] + i as u32 * 4)? as i32 as f32;
                    }
                    for (i, v) in points.iter_mut().enumerate() {
                        *v = mem.read_u32(a[1] + i as u32 * 4)? as i32 as f32;
                    }
                }
                let m11 = (points[2] - points[0]) / rect[2];
                let m21 = (points[4] - points[0]) / rect[3];
                let m12 = (points[3] - points[1]) / rect[2];
                let m22 = (points[5] - points[1]) / rect[3];
                create_matrix(
                    mem,
                    self,
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
                let m = matrix(mem, self, a[0])?;
                create_matrix(mem, self, a[1], m)?;
            }
            "GdipDeleteMatrix" => {
                if !self.matrices.contains(&a[0]) {
                    return Err(INVALID.into());
                }
                mem.free(a[0])?;
                self.matrices.remove(&a[0]);
            }
            "GdipGetMatrixElements" => {
                let m = matrix(mem, self, a[0])?;
                write_matrix(mem, a[1], m)?;
            }
            "GdipSetMatrixElements" => {
                matrix(mem, self, a[0])?;
                write_matrix(mem, a[0], std::array::from_fn(|i| f32::from_bits(a[i + 1])))?;
            }
            "GdipIsMatrixIdentity" | "GdipIsMatrixEqual" | "GdipIsMatrixInvertible" => {
                let m = matrix(mem, self, a[0])?;
                if name == "GdipIsMatrixInvertible" {
                    output_bool(mem, a[1], invertible(m))?;
                } else {
                    let (other, output) = if name == "GdipIsMatrixIdentity" {
                        (IDENTITY, a[1])
                    } else {
                        (matrix(mem, self, a[1])?, a[2])
                    };
                    output_bool(mem, output, m.map(f32::to_bits) == other.map(f32::to_bits))?;
                }
            }
            "GdipInvertMatrix" => {
                let mut m = matrix(mem, self, a[0])?;
                if !invertible(m) {
                    return Err(INVALID.into());
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
                let mut m = matrix(mem, self, a[0])?;
                let order = if name == "GdipMultiplyMatrix" || name == "GdipRotateMatrix" {
                    a[2]
                } else {
                    a[3]
                };
                if order > 1 {
                    return Err(INVALID.into());
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
                            "GdipMultiplyMatrix" => matrix(mem, self, a[1])?,
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
                let m = matrix(mem, self, a[0])?;
                let count = a[2] as i32;
                if count <= 0 {
                    return Err(INVALID.into());
                }
                let len = (count as u32)
                    .checked_mul(8)
                    .ok_or_else(|| B::Error::from(INVALID))?;
                pointer(mem, a[1], len, false)?;
                pointer(mem, a[1], len, true)?;
                let integer = name.ends_with('I');
                let vector = name.starts_with("GdipVector");
                for i in 0..count as u32 {
                    let p = a[1] + i * 8;
                    let xb = mem.read_u32(p)?;
                    let yb = mem.read_u32(p + 4)?;
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
                    mem.write_u32(p, bits(dx))?;
                    mem.write_u32(p + 4, bits(dy))?;
                }
            }
            _ => {
                return Err(NOT_IMPLEMENTED.into());
            }
        }
        Ok(OK)
    }
}
