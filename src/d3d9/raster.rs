//! CPU fixed-function XYZRHW rasterizer. D3D9 samples at integer pixel centers;
//! the top-left rule gives shared triangle edges exactly one owner.
use super::*;
pub(super) fn render(d: &Device, n: u32) -> Result<u32, u32> {
    if let Some(&v) = d.render.get(&n) {
        return Ok(v);
    }
    Ok(match n {
        7 | 15 | 27 | 28 | 29 | 52 | 137 | 174 | 194 | 206 => 0,
        8 => 3,
        9 => 2,
        14 | 16 | 26 | 136 | 141 | 161 => 1,
        19 => 2,
        20 => 1,
        22 => 3,
        23 | 25 => 8,
        24 => 0,
        60 => u32::MAX,
        168 => 15,
        171 | 209 => 1,
        207 => 2,
        208 => 1,
        _ => return Err(UNAVAILABLE),
    })
}
pub(super) fn render_state(n: u32, v: u32) -> bool {
    match n {
        7 | 15 | 28 | 29 | 52 | 137 | 194 | 206 => v == 0,
        8 => v == 3,
        9 => v == 2,
        14 | 16 | 26 | 27 | 136 | 141 | 161 | 174 => v <= 1,
        19 | 20 => matches!(v, 1 | 2 | 5 | 6),
        22 => matches!(v, 1..=3),
        23 | 25 => v == 8,
        24 => v <= 255,
        60 => true,
        168 => v <= 15,
        171 => v == 1,
        207..=209 => false,
        _ => false,
    }
}
pub(super) fn sampler(d: &Device, n: u32) -> Result<u32, u32> {
    if let Some(&v) = d.sampler.get(&n) {
        return Ok(v);
    }
    Ok(match n {
        1..=3 => 1,
        4 => 0,
        5 | 6 => 1,
        7 | 8 | 9 | 11 => 0,
        10 => 1,
        _ => return Err(UNAVAILABLE),
    })
}
pub(super) fn sampler_state(n: u32, v: u32) -> bool {
    match n {
        1 | 2 => matches!(v, 1 | 3),
        3 => v == 1,
        4 => true,
        5 | 6 => matches!(v, 1 | 2),
        7 | 8 | 9 | 11 => v == 0,
        10 => v == 1,
        _ => false,
    }
}
pub(super) fn stage(d: &Device, index: u32, n: u32) -> Result<u32, u32> {
    if index > 1 {
        return Err(UNAVAILABLE);
    }
    if let Some(&v) = d.stage.get(&(index * 32 + n)) {
        return Ok(v);
    }
    Ok(match n {
        1 => {
            if index == 0 {
                4
            } else {
                1
            }
        }
        4 => {
            if index == 0 {
                2
            } else {
                1
            }
        }
        2 | 5 => 2,
        3 | 6 => 1,
        11 | 24 => 0,
        _ => return Err(UNAVAILABLE),
    })
}
pub(super) fn stage_state(index: u32, n: u32, v: u32) -> bool {
    if index == 1 {
        return matches!(n, 1 | 4) && v == 1;
    }
    match n {
        1 => matches!(v, 2..=4),
        4 => matches!(v, 1..=4),
        2 | 3 | 5 | 6 => matches!(v, 0..=2),
        11 | 24 => v == 0,
        _ => false,
    }
}
pub(super) fn validate(d: &Device) -> Result<(), u32> {
    if !matches!(d.fvf, 0x104 | 0x144) || render(d, 7)? != 0 {
        return Err(UNAVAILABLE);
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Vertex {
    x: f64,
    y: f64,
    u: f64,
    v: f64,
    color: u32,
}
fn edge(a: Vertex, b: Vertex, x: f64, y: f64) -> f64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}
fn top_left(a: Vertex, b: Vertex) -> bool {
    b.y < a.y || (b.y == a.y && b.x > a.x)
}
fn modulate(a: u32, b: u32) -> u32 {
    (0..4).fold(0, |n, i| {
        n + (((((a >> (8 * i)) & 255) * ((b >> (8 * i)) & 255) + 127) / 255) << (8 * i))
    })
}
fn combine(op: u32, arg1: u32, arg2: u32, texture: u32, diffuse: u32) -> u32 {
    let arg = |v| if v == 2 { texture } else { diffuse };
    match op {
        2 => arg(arg1),
        3 => arg(arg2),
        _ => modulate(arg(arg1), arg(arg2)),
    }
}
fn sample(src: &[u32], b: &Pixels, u: f64, v: f64, d: &Device) -> Result<u32, u32> {
    let address = |t: f64, n: u32| {
        if n == 1 {
            t.rem_euclid(1.0)
        } else {
            t.clamp(0.0, 1.0)
        }
    };
    let u = address(u, sampler(d, 1)?);
    let v = address(v, sampler(d, 2)?);
    let get = |x: i64, y: i64| -> u32 {
        let coord = |i: i64, max: u32, n: u32| {
            if n == 1 {
                i.rem_euclid(max as i64) as u32
            } else {
                i.clamp(0, max as i64 - 1) as u32
            }
        };
        let x = coord(x, b.width, sampler(d, 1).unwrap());
        let y = coord(y, b.height, sampler(d, 2).unwrap());
        let n = src[(y * b.width + x) as usize];
        if b.format == 22 {
            n | 0xff000000
        } else {
            n
        }
    };
    if sampler(d, 5)? == 1 && sampler(d, 6)? == 1 {
        return Ok(get(
            (u * b.width as f64).floor() as i64,
            (v * b.height as f64).floor() as i64,
        ));
    }
    // Both filters must agree; choosing min/mag from gradients comes later.
    if sampler(d, 5)? != sampler(d, 6)? {
        return Err(UNAVAILABLE);
    }
    let x = u * b.width as f64 - 0.5;
    let y = v * b.height as f64 - 0.5;
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let p = [
        get(ix, iy),
        get(ix + 1, iy),
        get(ix, iy + 1),
        get(ix + 1, iy + 1),
    ];
    let weights = [
        (1.0 - fx) * (1.0 - fy),
        fx * (1.0 - fy),
        (1.0 - fx) * fy,
        fx * fy,
    ];
    Ok((0..4).fold(0, |out, c| {
        out + ((p
            .iter()
            .zip(weights)
            .map(|(p, w)| ((p >> (c * 8)) & 255) as f64 * w)
            .sum::<f64>()
            .round() as u32)
            << (c * 8))
    }))
}
fn blend(src: u32, dst: u32, sf: u32, df: u32) -> u32 {
    let factor = |f| match f {
        1 => 0,
        2 => 255,
        5 => src >> 24,
        _ => 255 - (src >> 24),
    };
    let s = factor(sf);
    let d = factor(df);
    (0..4).fold(0, |out, c| {
        out + (((((src >> (c * 8)) & 255) * s + ((dst >> (c * 8)) & 255) * d + 127) / 255).min(255)
            << (c * 8))
    })
}
impl D3D9 {
    pub(super) fn draw(&mut self, h: u32, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        let d = self.device(h)?;
        validate(d)?;
        if !d.scene {
            return Err(INVALID);
        }
        if !matches!(a[1], 4..=6) || a[2] > 65536 {
            return Err(UNAVAILABLE);
        }
        let stride = if d.fvf == 0x144 { 28 } else { 24 };
        if a[4] < stride || a[4] > 4096 {
            return Err(INVALID);
        }
        let count = if a[1] == 4 { a[2] * 3 } else { a[2] + 2 };
        if a[2] == 0 {
            return Ok(0);
        }
        let size = (count - 1)
            .checked_mul(a[4])
            .and_then(|n| n.checked_add(stride))
            .ok_or(INVALID)?;
        mem.ptr(a[3], size, false).map_err(|_| INVALID)?;
        let mut vertices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let w = words::<6>(mem, a[3] + i * a[4])?;
            let xyzw = w[..4]
                .iter()
                .map(|&n| f32::from_bits(n))
                .collect::<Vec<_>>();
            if xyzw.iter().any(|n| !n.is_finite())
                || !(0.0..=1.0).contains(&xyzw[2])
                || xyzw[3] != 1.0
            {
                return Err(UNAVAILABLE);
            }
            let uv = if stride == 28 {
                words::<2>(mem, a[3] + i * a[4] + 20)?
            } else {
                [w[4], w[5]]
            };
            let u = f32::from_bits(uv[0]);
            let v = f32::from_bits(uv[1]);
            if !u.is_finite() || !v.is_finite() || xyzw[0].abs() > 1e6 || xyzw[1].abs() > 1e6 {
                return Err(INVALID);
            }
            vertices.push(Vertex {
                x: xyzw[0] as f64,
                y: xyzw[1] as f64,
                u: u as f64,
                v: v as f64,
                color: if stride == 28 { w[4] } else { 0xffffffff },
            });
        }
        if d.texture == 0 {
            return Err(UNAVAILABLE);
        }
        let texture = self.buffer(d.texture)?;
        let tb = texture.borrow();
        let target = self.buffer(d.target)?;
        let b = target.borrow();
        if tb.locked || b.locked || Rc::ptr_eq(&texture, &target) {
            return Err(INVALID);
        }
        mem.ptr(b.bits, b.len(), true).map_err(|_| INVALID)?;
        let source = resource::read(mem, &tb)?;
        let mut dest = resource::read(mem, &b)?;
        let vp = d.viewport;
        let mut clip = [
            vp[0] as i32,
            vp[1] as i32,
            (vp[0] + vp[2]) as i32,
            (vp[1] + vp[3]) as i32,
        ];
        if render(d, 174)? != 0 {
            clip = [
                clip[0].max(d.scissor[0]),
                clip[1].max(d.scissor[1]),
                clip[2].min(d.scissor[2]),
                clip[3].min(d.scissor[3]),
            ];
        }
        // Bound worst-case CPU work before any target mutation.
        if (clip[2] - clip[0]).max(0) as u64 * (clip[3] - clip[1]).max(0) as u64 * a[2] as u64
            > 64 * 1024 * 1024
        {
            return Err(UNAVAILABLE);
        }
        for i in 0..a[2] as usize {
            let ids = match a[1] {
                4 => [i * 3, i * 3 + 1, i * 3 + 2],
                5 => {
                    if i % 2 == 0 {
                        [i, i + 1, i + 2]
                    } else {
                        [i + 1, i, i + 2]
                    }
                }
                _ => [0, i + 1, i + 2],
            };
            let mut v = ids.map(|j| vertices[j]);
            let area = edge(v[0], v[1], v[2].x, v[2].y);
            if area == 0.0 {
                continue;
            }
            if (render(d, 22)? == 2 && area > 0.0) || (render(d, 22)? == 3 && area < 0.0) {
                continue;
            }
            if area < 0.0 {
                v.swap(1, 2);
            }
            let area = area.abs();
            let minx = v
                .iter()
                .map(|v| v.x.ceil() as i32)
                .min()
                .unwrap()
                .max(clip[0])
                .max(0);
            let miny = v
                .iter()
                .map(|v| v.y.ceil() as i32)
                .min()
                .unwrap()
                .max(clip[1])
                .max(0);
            let maxx = v
                .iter()
                .map(|v| v.x.floor() as i32)
                .max()
                .unwrap()
                .min(clip[2] - 1)
                .min(b.width as i32 - 1);
            let maxy = v
                .iter()
                .map(|v| v.y.floor() as i32)
                .max()
                .unwrap()
                .min(clip[3] - 1)
                .min(b.height as i32 - 1);
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let e = [
                        edge(v[1], v[2], x as f64, y as f64),
                        edge(v[2], v[0], x as f64, y as f64),
                        edge(v[0], v[1], x as f64, y as f64),
                    ];
                    if e.iter().enumerate().any(|(j, &n)| {
                        n < 0.0 || (n == 0.0 && !top_left(v[(j + 1) % 3], v[(j + 2) % 3]))
                    }) {
                        continue;
                    }
                    let w = e.map(|n| n / area);
                    let u = (0..3).map(|j| v[j].u * w[j]).sum();
                    let vv = (0..3).map(|j| v[j].v * w[j]).sum();
                    let diffuse = (0..4).fold(0, |out, c| {
                        out + (((0..3)
                            .map(|j| ((v[j].color >> (c * 8)) & 255) as f64 * w[j])
                            .sum::<f64>()
                            .round() as u32)
                            << (c * 8))
                    });
                    let tex = sample(&source, &tb, u, vv, d)?;
                    let rgb = combine(
                        stage(d, 0, 1)?,
                        stage(d, 0, 2)?,
                        stage(d, 0, 3)?,
                        tex,
                        diffuse,
                    );
                    let alpha = if stage(d, 0, 4)? == 1 {
                        diffuse & 0xff000000
                    } else {
                        combine(
                            stage(d, 0, 4)?,
                            stage(d, 0, 5)?,
                            stage(d, 0, 6)?,
                            tex,
                            diffuse,
                        ) & 0xff000000
                    };
                    let mut color = (rgb & 0xffffff) | alpha;
                    let index = (y as u32 * b.width + x as u32) as usize;
                    if render(d, 27)? != 0 {
                        color = blend(color, dest[index], render(d, 19)?, render(d, 20)?);
                    }
                    let mask = render(d, 168)?;
                    let channels = ((mask & 1) * 0xff0000)
                        | (((mask >> 1) & 1) * 0xff00)
                        | (((mask >> 2) & 1) * 0xff)
                        | (((mask >> 3) & 1) * 0xff000000);
                    dest[index] = (color & channels) | (dest[index] & !channels);
                    if b.format == 22 {
                        dest[index] |= 0xff000000;
                    }
                }
            }
        }
        resource::write(mem, &b, &dest)?;
        self.draws += 1;
        Ok(0)
    }
}
