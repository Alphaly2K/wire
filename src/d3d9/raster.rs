//! CPU fixed-function XYZRHW rasterizer. D3D9 samples at integer pixel centers;
//! the top-left rule gives shared triangle edges exactly one owner.
use super::*;
pub(super) fn render(d: &Device, n: u32) -> Result<u32, u32> {
    if let Some(&v) = d.render.get(&n) {
        return Ok(v);
    }
    Ok(match n {
        7 => (d.depth != 0) as u32,
        15 | 27 | 28 | 29 | 52 | 137 | 174 | 194 | 206 => 0,
        8 => 3,
        9 => 2,
        14 | 16 | 26 | 136 | 141 | 161 => 1,
        19 => 2,
        20 => 1,
        22 => 3,
        23 => 4,
        25 | 56 => 8,
        53..=55 => 1,
        24 | 57 => 0,
        58..=60 => u32::MAX,
        168 => 15,
        171 | 209 => 1,
        207 => 2,
        208 => 1,
        _ => return Err(UNAVAILABLE),
    })
}
pub(super) fn render_state(n: u32, v: u32) -> bool {
    match n {
        28 | 29 | 137 | 194 | 206 => v == 0,
        8 => v == 3,
        9 => v == 2,
        7 | 14 | 15 | 16 | 26 | 27 | 52 | 136 | 141 | 161 | 174 => v <= 1,
        19 | 20 => matches!(v, 1 | 2 | 5 | 6),
        22 => matches!(v, 1..=3),
        23 | 25 | 53..=56 => matches!(v, 1..=8),
        24 | 57 => v <= 255,
        58..=60 => true,
        168 => v <= 15,
        171 => v == 1,
        207..=209 => false,
        _ => false,
    }
}
pub(super) fn sampler(d: &Device, n: u32) -> Result<u32, u32> {
    sampler_slot(d, 0, n)
}
pub(super) fn sampler_slot(d: &Device, slot: u32, n: u32) -> Result<u32, u32> {
    if let Some(&v) = d.sampler.get(&(slot * 16 + n)) {
        return Ok(v);
    }
    Ok(match n {
        1..=3 => 1,
        4 => 0,
        5 | 6 => 1,
        7 | 8 | 9 | 11..=13 => 0,
        10 => 1,
        _ => return Err(UNAVAILABLE),
    })
}
pub(super) fn sampler_state(n: u32, v: u32) -> bool {
    match n {
        1 | 2 => matches!(v, 1 | 3),
        3 => matches!(v, 1 | 3), // W addressing has no effect on supported 2D samplers.
        4 => true,
        5 | 6 => matches!(v, 1 | 2),
        7 => v <= 2, // Only one mip level exists, so NONE/POINT/LINEAR select that level.
        8 | 9 | 11..=13 => v == 0,
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
    if d.declaration == 0
        && !matches!(
            d.fvf,
            0x102 | 0x142 | 0x104 | 0x144 | 0x202 | 0x242 | 0x204 | 0x244
        )
    {
        return Err(UNAVAILABLE);
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Vertex {
    x: f64,
    y: f64,
    z: f64,
    u: f64,
    v: f64,
    uv1: [f64; 2],
    color: [f64; 4],
    rhw: f64,
}
fn clipped(mut polygon: Vec<Vertex>, viewport: [u32; 6]) -> Vec<[Vertex; 3]> {
    // D3D homogeneous clip volume: -w<=x,y<=w and 0<=z<=w.
    for plane in 0..6 {
        let distance = |v: Vertex| match plane {
            0 => v.x + v.rhw,
            1 => v.rhw - v.x,
            2 => v.y + v.rhw,
            3 => v.rhw - v.y,
            4 => v.z,
            _ => v.rhw - v.z,
        };
        let input = std::mem::take(&mut polygon);
        if input.is_empty() {
            return Vec::new();
        }
        let mut prev = *input.last().unwrap();
        let mut dp = distance(prev);
        for cur in input {
            let dc = distance(cur);
            if (dp >= 0.) != (dc >= 0.) {
                let t = dp / (dp - dc);
                let mix = |a: f64, b: f64| a + (b - a) * t;
                polygon.push(Vertex {
                    x: mix(prev.x, cur.x),
                    y: mix(prev.y, cur.y),
                    z: mix(prev.z, cur.z),
                    rhw: mix(prev.rhw, cur.rhw),
                    u: mix(prev.u, cur.u),
                    v: mix(prev.v, cur.v),
                    uv1: std::array::from_fn(|i| mix(prev.uv1[i], cur.uv1[i])),
                    color: std::array::from_fn(|i| mix(prev.color[i], cur.color[i])),
                });
            }
            if dc >= 0. {
                polygon.push(cur);
            }
            prev = cur;
            dp = dc;
        }
    }
    let mut output = Vec::new();
    for mut v in polygon {
        if v.rhw <= 0. {
            return Vec::new();
        }
        v.rhw = 1. / v.rhw;
        v.x = viewport[0] as f64 + (v.x * v.rhw + 1.) * viewport[2] as f64 / 2.;
        v.y = viewport[1] as f64 + (1. - v.y * v.rhw) * viewport[3] as f64 / 2.;
        let min = f32::from_bits(viewport[4]) as f64;
        let max = f32::from_bits(viewport[5]) as f64;
        v.z = min + v.z * v.rhw * (max - min);
        output.push(v);
    }
    (1..output.len().saturating_sub(1))
        .map(|i| [output[0], output[i], output[i + 1]])
        .collect()
}
#[inline(always)]
fn edge(a: Vertex, b: Vertex, x: f64, y: f64) -> f64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}
fn top_left(a: Vertex, b: Vertex) -> bool {
    b.y < a.y || (b.y == a.y && b.x > a.x)
}
fn row_span(v: &[Vertex; 3], y: i32, mut left: i32, mut right: i32) -> (i32, i32) {
    for i in 0..3 {
        let a = v[i];
        let b = v[(i + 1) % 3];
        let dy = b.y - a.y;
        if dy == 0. {
            let n = (b.x - a.x) * (y as f64 - a.y);
            if n < 0. || (n == 0. && !top_left(a, b)) {
                return (1, 0);
            }
        } else {
            let crossing = a.x + (b.x - a.x) * (y as f64 - a.y) / dy;
            if crossing.is_finite() {
                // Retain a one-pixel margin for intersection rounding. The
                // original exact edge expressions still decide ownership.
                if dy > 0. {
                    right = right.min((crossing.ceil() as i32).saturating_add(1));
                } else {
                    left = left.max((crossing.floor() as i32).saturating_sub(1));
                }
            }
        }
    }
    (left, right)
}
#[inline(always)]
fn compare(f: u32, a: u32, b: u32) -> bool {
    match f {
        1 => false,
        2 => a < b,
        3 => a == b,
        4 => a <= b,
        5 => a > b,
        6 => a != b,
        7 => a >= b,
        _ => true,
    }
}
fn stencil(op: u32, old: u32, reference: u32, mask: u32) -> u32 {
    let n = match op {
        1 => old,
        2 => 0,
        3 => reference,
        4 => (old + 1).min(255),
        5 => old.saturating_sub(1),
        6 => !old,
        7 => old.wrapping_add(1),
        _ => old.wrapping_sub(1),
    };
    (old & !mask) | (n & mask & 255)
}
#[inline(always)]
fn modulate(a: u32, b: u32) -> u32 {
    if a == u32::MAX {
        return b;
    }
    if b == u32::MAX {
        return a;
    }
    (0..4).fold(0, |n, i| {
        n + (((((a >> (8 * i)) & 255) * ((b >> (8 * i)) & 255) + 127) / 255) << (8 * i))
    })
}
#[inline(always)]
fn combine(op: u32, arg1: u32, arg2: u32, texture: u32, diffuse: u32) -> u32 {
    let arg = |v| if v == 2 { texture } else { diffuse };
    match op {
        2 => arg(arg1),
        3 => arg(arg2),
        _ => modulate(arg(arg1), arg(arg2)),
    }
}
struct Sampling {
    address: [u32; 2],
    filter: [u32; 2],
}
impl Sampling {
    fn new(d: &Device) -> Result<Self, u32> {
        Ok(Self {
            address: [sampler(d, 1)?, sampler(d, 2)?],
            filter: [sampler(d, 5)?, sampler(d, 6)?],
        })
    }
}
// Keep sampling and color arithmetic in the pixel loop so LLVM can hoist
// immutable sampler/stage state and remove unused color paths for the draw.
#[inline(always)]
fn sample(src: &[u32], b: &Pixels, u: f64, v: f64, s: &Sampling) -> Result<u32, u32> {
    let address = |t: f64, n: u32| {
        if n == 1 {
            if (0.0..1.0).contains(&t) {
                t
            } else {
                t.rem_euclid(1.0)
            }
        } else {
            t.clamp(0.0, 1.0)
        }
    };
    let u = address(u, s.address[0]);
    let v = address(v, s.address[1]);
    let get = |x: i64, y: i64| -> u32 {
        let coord = |i: i64, max: u32, n: u32| {
            if i >= 0 && i < max as i64 {
                i as u32
            } else if n == 1 {
                i.rem_euclid(max as i64) as u32
            } else {
                i.clamp(0, max as i64 - 1) as u32
            }
        };
        let x = coord(x, b.width, s.address[0]);
        let y = coord(y, b.height, s.address[1]);
        let n = src[(y * b.width + x) as usize];
        if b.format == 22 {
            n | 0xff000000
        } else {
            n
        }
    };
    if s.filter == [1, 1] {
        return Ok(get(
            (u * b.width as f64).floor() as i64,
            (v * b.height as f64).floor() as i64,
        ));
    }
    // Both filters must agree; choosing min/mag from gradients comes later.
    if s.filter[0] != s.filter[1] {
        return Err(UNAVAILABLE);
    }
    let x = u * b.width as f64 - 0.5;
    let y = v * b.height as f64 - 0.5;
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    if fx == 0. && fy == 0. {
        return Ok(get(ix, iy));
    }
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
fn pack_shader_color(v: [f32; 4]) -> u32 {
    let c = v.map(|v| {
        if v.is_nan() {
            0
        } else {
            (v.clamp(0., 1.) * 255. + 0.5).floor() as u32
        }
    });
    (c[3] << 24) | (c[0] << 16) | (c[1] << 8) | c[2]
}
fn sample_float(src: &[u32], b: &Pixels, u: f64, v: f64, s: &Sampling) -> [f32; 4] {
    let address = |v: f64, mode: u32| {
        if mode == 1 {
            v.rem_euclid(1.)
        } else {
            v.clamp(0., 1.)
        }
    };
    if !u.is_finite() || !v.is_finite() {
        return [0.; 4];
    }
    let x = address(u, s.address[0]) * b.width as f64;
    let y = address(v, s.address[1]) * b.height as f64;
    let get = |x: i64, y: i64| {
        let coord = |i: i64, size: u32, mode: u32| {
            if mode == 1 {
                i.rem_euclid(size as i64) as u32
            } else {
                i.clamp(0, size as i64 - 1) as u32
            }
        };
        let n = src[(coord(y, b.height, s.address[1]) * b.width + coord(x, b.width, s.address[0]))
            as usize];
        [
            ((n >> 16) & 255) as f32 / 255.,
            ((n >> 8) & 255) as f32 / 255.,
            (n & 255) as f32 / 255.,
            if b.format == 22 {
                1.
            } else {
                (n >> 24) as f32 / 255.
            },
        ]
    };
    if s.filter == [1, 1] {
        return get(x.floor() as i64, y.floor() as i64);
    }
    let x = x - 0.5;
    let y = y - 0.5;
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = (x - x.floor()) as f32;
    let fy = (y - y.floor()) as f32;
    let p = [
        get(ix, iy),
        get(ix + 1, iy),
        get(ix, iy + 1),
        get(ix + 1, iy + 1),
    ];
    let w = [
        (1. - fx) * (1. - fy),
        fx * (1. - fy),
        (1. - fx) * fy,
        fx * fy,
    ];
    std::array::from_fn(|i| (0..4).map(|j| p[j][i] * w[j]).sum())
}
#[inline(always)]
fn blend(src: u32, dst: u32, sf: u32, df: u32) -> u32 {
    let factor = |f| match f {
        1 => 0,
        2 => 255,
        5 => src >> 24,
        _ => 255 - (src >> 24),
    };
    let s = factor(sf);
    let d = factor(df);
    if s == 255 && d == 0 {
        return src;
    }
    if s == 0 && d == 255 {
        return dst;
    }
    (0..4).fold(0, |out, c| {
        out + (((((src >> (c * 8)) & 255) * s + ((dst >> (c * 8)) & 255) * d + 127) / 255).min(255)
            << (c * 8))
    })
}
static RASTER_WORKERS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

struct Raster<'a> {
    source: &'a [u32],
    texture: &'a Pixels,
    target: &'a Pixels,
    render: &'a [u32; 175],
    stage: &'a [u32; 7],
    sampling: &'a Sampling,
    color_mask: u32,
    clip: [i32; 4],
    depth_width: u32,
    shader: Option<(&'a shader::Program, &'a [[f32; 4]; 32])>,
    extra: Option<(&'a Pixels, &'a [u32], &'a Sampling)>,
}
impl Raster<'_> {
    fn gpu(
        &self,
        triangles: &[[Vertex; 3]],
        dest: &mut [u32],
        depth: Option<&mut [u32]>,
        gpu: &mut super::gpu::GpuRaster,
    ) -> bool {
        if self.sampling.filter[0] != self.sampling.filter[1] {
            return false;
        }
        let mut vertices = Vec::with_capacity(triangles.len() * 36);
        for triangle in triangles {
            for v in triangle {
                vertices.extend([
                    v.x as f32,
                    v.y as f32,
                    v.z as f32,
                    v.rhw as f32,
                    v.u as f32,
                    v.v as f32,
                    v.uv1[0] as f32,
                    v.uv1[1] as f32,
                    v.color[0] as f32,
                    v.color[1] as f32,
                    v.color[2] as f32,
                    v.color[3] as f32,
                ]);
            }
        }
        if vertices.iter().any(|n| !n.is_finite()) {
            return false;
        }
        let mut params = [0u32; 200];
        params[..8].copy_from_slice(&[
            self.target.width,
            self.target.height,
            self.texture.width,
            self.texture.height,
            self.texture.format,
            self.target.format,
            self.depth_width,
            triangles.len() as u32,
        ]);
        params[8..12].copy_from_slice(&self.clip.map(|n| n as u32));
        params[12..14].copy_from_slice(&self.sampling.address);
        params[14..16].copy_from_slice(&self.sampling.filter);
        params[16..191].copy_from_slice(self.render);
        params[191..198].copy_from_slice(self.stage);
        params[198] = self.color_mask;
        params[199] = depth.is_some() as u32;
        let mut source = std::borrow::Cow::Borrowed(self.source);
        let extra = if let Some((t, s, sampling)) = self.extra {
            let offset = source.len() as u32;
            source.to_mut().extend(s);
            [
                t.width,
                t.height,
                t.format,
                sampling.address[0],
                sampling.address[1],
                sampling.filter[0],
                offset,
                0,
            ]
        } else {
            [0; 8]
        };
        let program = self
            .shader
            .map_or_else(|| vec![0], |(p, c)| p.gpu_words(c, extra));
        gpu.draw(&vertices, &params, &source, dest, depth, &program)
    }
    fn render(
        &self,
        triangles: &[[Vertex; 3]],
        dest: &mut [u32],
        depth: Option<&mut [u32]>,
        workers: usize,
    ) -> Result<(), u32> {
        let height = self.target.height as usize;
        let width = self.target.width as usize;
        let workers = workers.clamp(1, 4).min(height);
        if workers <= 1 {
            return self.rows(triangles, 0, dest, depth);
        }
        let rows = height.div_ceil(workers);
        let color_chunk = rows * width;
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers - 1);
            let mut bands = dest.chunks_mut(color_chunk).enumerate();
            let (_, first) = bands.next().unwrap();
            let first_depth = depth.map(|values| {
                let (first, rest) = values.split_at_mut(rows * self.depth_width as usize);
                (first, rest)
            });
            let (first_depth, mut remaining_depth) = match first_depth {
                Some((first, rest)) => (Some(first), Some(rest)),
                None => (None, None),
            };
            for (band, pixels) in bands {
                let band_depth = if let Some(values) = remaining_depth.take() {
                    let count = pixels.len() / width * self.depth_width as usize;
                    let (current, rest) = values.split_at_mut(count);
                    remaining_depth = Some(rest);
                    Some(current)
                } else {
                    None
                };
                handles.push(
                    std::thread::Builder::new()
                        .name("wire-raster".into())
                        .spawn_scoped(scope, move || {
                            self.rows(triangles, band * rows, pixels, band_depth)
                        })
                        .map_err(|_| NOMEM)?,
                );
            }
            let mut result = self.rows(triangles, 0, first, first_depth);
            for handle in handles {
                // Panics are programming errors, as in the serial renderer;
                // all bands join before any guest memory can be committed.
                let band_result = handle.join().expect("raster worker panicked");
                if result.is_ok() {
                    result = band_result;
                }
            }
            result
        })
    }
    fn rows(
        &self,
        triangles: &[[Vertex; 3]],
        first_y: usize,
        dest: &mut [u32],
        mut depth_pixels: Option<&mut [u32]>,
    ) -> Result<(), u32> {
        let source = self.source;
        let tb = self.texture;
        let b = self.target;
        let r = self.render;
        let texture_stage = self.stage;
        let sampling = self.sampling;
        let color_mask = self.color_mask;
        let clip = self.clip;
        let depth_width = self.depth_width;
        let last_y = first_y + dest.len() / b.width as usize;
        for &triangle in triangles {
            let mut v = triangle;
            let area = edge(v[0], v[1], v[2].x, v[2].y);
            if area == 0.0 {
                continue;
            }
            if (r[22] == 2 && area > 0.0) || (r[22] == 3 && area < 0.0) {
                continue;
            }
            if area < 0.0 {
                v.swap(1, 2);
            }
            let area = area.abs();
            // Equal reciprocal W makes interpolation affine. Retain the
            // full perspective division when any vertex differs.
            let affine = v[0].rhw == v[1].rhw && v[0].rhw == v[2].rhw;
            let owned = [
                top_left(v[1], v[2]),
                top_left(v[2], v[0]),
                top_left(v[0], v[1]),
            ];
            let constant_color =
                (v[0].color == v[1].color && v[0].color == v[2].color).then(|| {
                    (0..4).fold(0, |out, c| {
                        out | ((v[0].color[c].round() as u32) << (c * 8))
                    })
                });
            let gradients = [
                (v[1].u - v[0].u, v[2].u - v[0].u),
                (v[1].v - v[0].v, v[2].v - v[0].v),
            ]
            .map(|(a, b)| {
                [
                    (a * (v[2].y - v[0].y) - b * (v[1].y - v[0].y)) / area,
                    (b * (v[1].x - v[0].x) - a * (v[2].x - v[0].x)) / area,
                ]
            });
            let needs_weights = !affine
                || constant_color.is_none()
                || depth_pixels.is_some()
                || self.shader.is_some();
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
                .max(first_y as i32)
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
                .min(b.height as i32 - 1)
                .min(last_y as i32 - 1);
            for y in miny..=maxy {
                let row_uv = [
                    v[0].u + gradients[0][1] * (y as f64 - v[0].y),
                    v[0].v + gradients[1][1] * (y as f64 - v[0].y),
                ];
                let (row_left, row_right) = row_span(&v, y, minx, maxx);
                for x in row_left..=row_right {
                    let e = [
                        edge(v[1], v[2], x as f64, y as f64),
                        edge(v[2], v[0], x as f64, y as f64),
                        edge(v[0], v[1], x as f64, y as f64),
                    ];
                    if e[0] < 0.
                        || (e[0] == 0. && !owned[0])
                        || e[1] < 0.
                        || (e[1] == 0. && !owned[1])
                        || e[2] < 0.
                        || (e[2] == 0. && !owned[2])
                    {
                        continue;
                    }
                    let w = if needs_weights {
                        e.map(|n| n / area)
                    } else {
                        [0.; 3]
                    };
                    let pw = if affine {
                        w
                    } else {
                        let divisor = (0..3).map(|j| v[j].rhw * w[j]).sum::<f64>();
                        if divisor <= 0. {
                            continue;
                        }
                        std::array::from_fn::<_, 3, _>(|j| v[j].rhw * w[j] / divisor)
                    };
                    let (u, vv) = if affine {
                        (
                            row_uv[0] + gradients[0][0] * (x as f64 - v[0].x),
                            row_uv[1] + gradients[1][0] * (x as f64 - v[0].x),
                        )
                    } else {
                        (
                            (0..3).map(|j| v[j].u * pw[j]).sum(),
                            (0..3).map(|j| v[j].v * pw[j]).sum(),
                        )
                    };
                    let diffuse = constant_color.unwrap_or_else(|| {
                        (0..4).fold(0, |out, c| {
                            out + (((0..3).map(|j| v[j].color[c] * pw[j]).sum::<f64>().round()
                                as u32)
                                << (c * 8))
                        })
                    });
                    let shader_color = if let Some((program, constants)) = self.shader {
                        let dc = std::array::from_fn(|i| {
                            (0..3)
                                .map(|j| v[j].color[[2, 1, 0, 3][i]] * pw[j])
                                .sum::<f64>() as f32
                                / 255.
                        });
                        // Vertex storage is BGRA; shader registers are RGBA.
                        let uv1 = std::array::from_fn::<_, 2, _>(|i| {
                            (0..3).map(|j| v[j].uv1[i] * pw[j]).sum::<f64>() as f32
                        });
                        match program.execute(
                            constants,
                            dc,
                            [[u as f32, vv as f32, 0., 1.], [uv1[0], uv1[1], 0., 1.]],
                            |slot, at| {
                                let (t, src, s) = if slot == 0 {
                                    (tb, source, sampling)
                                } else {
                                    self.extra.unwrap()
                                };
                                sample_float(src, t, at[0] as f64, at[1] as f64, s)
                            },
                        ) {
                            Some(value) => Some(pack_shader_color(value)),
                            None => continue,
                        }
                    } else {
                        None
                    };
                    let tex = sample(source, tb, u, vv, sampling)?;
                    let rgb = combine(
                        texture_stage[1],
                        texture_stage[2],
                        texture_stage[3],
                        tex,
                        diffuse,
                    );
                    let alpha = if texture_stage[1..4] == texture_stage[4..7] {
                        rgb & 0xff000000
                    } else if texture_stage[4] == 1 {
                        diffuse & 0xff000000
                    } else {
                        combine(
                            texture_stage[4],
                            texture_stage[5],
                            texture_stage[6],
                            tex,
                            diffuse,
                        ) & 0xff000000
                    };
                    let mut color = shader_color.unwrap_or((rgb & 0xffffff) | alpha);
                    let index = ((y as u32 - first_y as u32) * b.width + x as u32) as usize;
                    if r[15] != 0 && !compare(r[25], color >> 24, r[24]) {
                        continue;
                    }
                    if let Some(values) = &mut depth_pixels {
                        let idx = ((y as u32 - first_y as u32) * depth_width + x as u32) as usize;
                        let old = values[idx];
                        let z = ((0..3).map(|j| v[j].z * w[j]).sum::<f64>().clamp(0., 1.)
                            * 0xffffff as f64)
                            .round() as u32;
                        let st = r[52] != 0;
                        let stencil_pass =
                            !st || compare(r[56], r[57] & r[58] & 255, old & r[58] & 255);
                        let depth_pass = r[7] == 0 || compare(r[23], z, old >> 8);
                        if st {
                            let op = if !stencil_pass {
                                53
                            } else if !depth_pass {
                                54
                            } else {
                                55
                            };
                            values[idx] =
                                (old & 0xffffff00) | stencil(r[op], old & 255, r[57], r[59] & 255);
                        }
                        if !stencil_pass || !depth_pass {
                            continue;
                        }
                        if r[7] != 0 && r[14] != 0 {
                            values[idx] = (z << 8) | (values[idx] & 255);
                        }
                    }
                    if r[27] != 0 {
                        color = blend(color, dest[index], r[19], r[20]);
                    }
                    dest[index] = (color & color_mask) | (dest[index] & !color_mask);
                    if b.format == 22 {
                        dest[index] |= 0xff000000;
                    }
                }
            }
        }
        Ok(())
    }
}

impl D3D9 {
    pub(super) fn draw(&mut self, h: u32, a: &Args, mem: &GuestMem) -> Result<u32, u32> {
        self.draw_selected(h, a, mem, None)
    }
    pub(super) fn draw_selected(
        &mut self,
        h: u32,
        a: &Args,
        mem: &GuestMem,
        indices: Option<&[u32]>,
    ) -> Result<u32, u32> {
        let d = self.device(h)?;
        validate(d)?;
        if !d.scene {
            return Err(INVALID);
        }
        if !matches!(a[1], 4..=6) || a[2] > 65536 {
            return Err(UNAVAILABLE);
        }
        let layout = self.layout(d)?;
        let stride = layout.size;
        if a[4] < stride || a[4] > 4096 {
            return Err(INVALID);
        }
        let count = if a[1] == 4 { a[2] * 3 } else { a[2] + 2 };
        if a[2] == 0 {
            return Ok(0);
        }
        // A draw is atomic with respect to guest state. Resolve immutable
        // state once, rather than looking up BTreeMaps for every pixel.
        let mut r = [0; 175];
        for n in [
            7, 14, 15, 19, 20, 22, 23, 24, 25, 27, 52, 53, 54, 55, 56, 57, 58, 59, 168, 174,
        ] {
            r[n] = render(d, n as u32)?;
        }
        let mut texture_stage = [0; 7];
        for (n, value) in texture_stage.iter_mut().enumerate().skip(1) {
            *value = stage(d, 0, n as u32)?;
        }
        let sampling = Sampling::new(d)?;
        if sampling.filter[0] != sampling.filter[1] {
            return Err(UNAVAILABLE);
        }
        let program = if d.pixel_shader == 0 {
            None
        } else {
            match &self.objects.get(&d.pixel_shader).ok_or(INVALID)?.payload {
                Payload::PixelShader { program, .. } => Some(program),
                _ => return Err(INVALID),
            }
        };
        // POSITIONT / XYZRHW vertices bypass vertex processing, including
        // a bound shader; the binding itself remains observable and owned.
        let vertex_program = if d.vertex_shader == 0 || layout.transformed {
            None
        } else {
            match &self.objects.get(&d.vertex_shader).ok_or(INVALID)?.payload {
                Payload::VertexShader { program, .. } => Some(program),
                _ => return Err(INVALID),
            }
        };
        let constants = program.map(|p| p.constants(&d.pixel_constants));
        let color_mask = ((r[168] & 1) * 0xff0000)
            | (((r[168] >> 1) & 1) * 0xff00)
            | (((r[168] >> 2) & 1) * 0xff)
            | (((r[168] >> 3) & 1) * 0xff000000);
        let max = indices
            .and_then(|v| v.iter().max().copied())
            .unwrap_or(count - 1);
        let size = max
            .checked_mul(a[4])
            .and_then(|n| n.checked_add(stride))
            .ok_or(INVALID)?;
        mem.ptr(a[3], size, false).map_err(|_| INVALID)?;
        let mut vertices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let i = indices.map_or(i, |v| v[i as usize]);
            let base = a[3] + i * a[4];
            let xyz = words::<3>(mem, base + layout.position)?.map(|v| f32::from_bits(v) as f64);
            let mut xyzw = [xyz[0], xyz[1], xyz[2], 1.];
            if layout.position_w {
                xyzw[3] = f32::from_bits(
                    mem.read_u32(base + layout.position + 12)
                        .map_err(|_| INVALID)?,
                ) as f64;
            }
            if layout.transformed {
                if !(0.0..=1.0).contains(&xyzw[2]) || xyzw[3] <= 0. {
                    return Err(UNAVAILABLE);
                }
            } else if vertex_program.is_none() {
                for n in [256, 2, 3] {
                    if let Some(matrix) = d.transforms.get(&n) {
                        xyzw = std::array::from_fn(|j| {
                            (0..4).map(|k| xyzw[k] * matrix[k * 4 + j] as f64).sum()
                        });
                    }
                }
            }
            if xyzw.iter().any(|n| !n.is_finite()) {
                return Err(INVALID);
            }
            let uv = words::<2>(mem, base + layout.uv)?;
            let u = f32::from_bits(uv[0]);
            let v = f32::from_bits(uv[1]);
            if !u.is_finite() || !v.is_finite() || xyzw[0].abs() > 1e6 || xyzw[1].abs() > 1e6 {
                return Err(INVALID);
            }
            let mut vertex = Vertex {
                x: xyzw[0],
                y: xyzw[1],
                z: xyzw[2],
                u: u as f64,
                v: v as f64,
                uv1: if let Some(off) = layout.uv1 {
                    let n = words::<2>(mem, base + off)?.map(|n| f32::from_bits(n) as f64);
                    if n.iter().any(|n| !n.is_finite()) {
                        return Err(INVALID);
                    }
                    n
                } else {
                    [0.; 2]
                },
                color: {
                    let color = if let Some(off) = layout.color {
                        mem.read_u32(base + off).map_err(|_| INVALID)?
                    } else {
                        0xffffffff
                    };
                    std::array::from_fn(|i| ((color >> (i * 8)) & 255) as f64)
                },
                rhw: xyzw[3],
            };
            if let Some(program) = vertex_program {
                let out = program.execute(
                    &d.vertex_constants,
                    xyzw.map(|n| n as f32),
                    [
                        vertex.color[2] as f32 / 255.,
                        vertex.color[1] as f32 / 255.,
                        vertex.color[0] as f32 / 255.,
                        vertex.color[3] as f32 / 255.,
                    ],
                    [[u, v], vertex.uv1.map(|n| n as f32)],
                )?;
                vertex.x = out.position[0] as f64;
                vertex.y = out.position[1] as f64;
                vertex.z = out.position[2] as f64;
                vertex.rhw = out.position[3] as f64;
                vertex.u = out.uv[0][0] as f64;
                vertex.v = out.uv[0][1] as f64;
                vertex.uv1 = out.uv[1].map(|n| n as f64);
                vertex.color =
                    std::array::from_fn(|i| out.color[[2, 1, 0, 3][i]].clamp(0., 1.) as f64 * 255.);
            }
            vertices.push(vertex);
        }
        if d.texture == 0 && program.is_some_and(|p| p.texture_mask & 1 != 0) {
            return Err(UNAVAILABLE);
        }
        if d.texture == 0 && program.is_none() {
            // Fixed-function stages requiring a missing texture are disabled;
            // stage zero's incoming CURRENT value is vertex diffuse.
            let uses_texture = match texture_stage[1] {
                2 => texture_stage[2] == 2,
                3 => texture_stage[3] == 2,
                4 => texture_stage[2] == 2 || texture_stage[3] == 2,
                _ => false,
            };
            if uses_texture {
                texture_stage = [0, 2, 0, 0, 2, 0, 0];
            }
        }
        let texture = if d.texture == 0 {
            Rc::new(RefCell::new(Pixels {
                width: 1,
                height: 1,
                format: 21,
                pool: 0,
                usage: 0,
                bits: 0,
                locked: false,
            }))
        } else {
            self.buffer(d.texture)?
        };
        let tb = texture.borrow();
        if program.is_some_and(|p| p.uv1) && layout.uv1.is_none() {
            return Err(UNAVAILABLE);
        }
        let extra = if program.is_some_and(|p| p.texture_mask & 2 != 0) {
            Some(self.buffer(d.texture1)?)
        } else {
            None
        };
        let extra = extra.as_ref().map(|t| t.borrow());
        let extra_source = extra.as_ref().map(|t| resource::read(mem, t)).transpose()?;
        let extra_sampling = Sampling {
            address: [sampler_slot(d, 1, 1)?, sampler_slot(d, 1, 2)?],
            filter: [sampler_slot(d, 1, 5)?, sampler_slot(d, 1, 6)?],
        };
        if extra.is_some() && extra_sampling.filter[0] != extra_sampling.filter[1] {
            return Err(UNAVAILABLE);
        }
        let target = self.buffer(d.target)?;
        let b = target.borrow();
        if tb.locked
            || b.locked
            || Rc::ptr_eq(&texture, &target)
            || extra.as_ref().is_some_and(|t| t.locked || t.bits == b.bits)
        {
            return Err(INVALID);
        }
        mem.ptr(b.bits, b.len(), true).map_err(|_| INVALID)?;
        let source = if d.texture == 0 {
            vec![0]
        } else {
            resource::read(mem, &tb)?
        };
        let mut dest = resource::read(mem, &b)?;
        let depth_buffer = if r[7] != 0 || r[52] != 0 {
            let ds = self.buffer(d.depth)?;
            {
                let ds = ds.borrow();
                if ds.locked || ds.width < b.width || ds.height < b.height {
                    return Err(INVALID);
                }
                mem.ptr(ds.bits, ds.len(), true).map_err(|_| INVALID)?;
            }
            Some(ds)
        } else {
            None
        };
        let mut depth_pixels = depth_buffer
            .as_ref()
            .map(|ds| resource::read(mem, &ds.borrow()))
            .transpose()?;
        let depth_width = depth_buffer.as_ref().map_or(0, |ds| ds.borrow().width);
        let vp = d.viewport;
        let mut clip = [
            vp[0] as i32,
            vp[1] as i32,
            (vp[0] + vp[2]) as i32,
            (vp[1] + vp[3]) as i32,
        ];
        if r[174] != 0 {
            clip = [
                clip[0].max(d.scissor[0]),
                clip[1].max(d.scissor[1]),
                clip[2].min(d.scissor[2]),
                clip[3].min(d.scissor[3]),
            ];
        }
        // Bound worst-case CPU work before any target mutation.
        if (clip[2] - clip[0]).max(0) as u64
            * (clip[3] - clip[1]).max(0) as u64
            * a[2] as u64
            * if layout.transformed { 1 } else { 8 }
            > 64 * 1024 * 1024
        {
            return Err(UNAVAILABLE);
        }
        let mut draw_triangles = Vec::new();
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
            let triangle = ids.map(|j| vertices[j]);
            let triangles = if layout.transformed {
                vec![triangle]
            } else {
                clipped(triangle.to_vec(), vp)
            };
            draw_triangles.extend(triangles);
        }
        let raster = Raster {
            source: &source,
            texture: &tb,
            target: &b,
            render: &r,
            stage: &texture_stage,
            sampling: &sampling,
            color_mask,
            clip,
            depth_width,
            shader: program.zip(constants.as_ref()),
            extra: extra
                .as_deref()
                .zip(extra_source.as_deref())
                .map(|(t, s)| (t, s, &extra_sampling)),
        };
        // Large draws use disjoint host-owned row bands. Guest execution and
        // checked read/write remain on this thread, with one atomic commit.
        let workers = if (clip[2] - clip[0]).max(0) as u64
            * (clip[3] - clip[1]).max(0) as u64
            * draw_triangles.len() as u64
            >= 128 * 1024
        {
            *std::sync::OnceLock::get_or_init(&RASTER_WORKERS, || {
                std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
            })
        } else {
            1
        };
        if !self.enable_gpu
            || !raster.gpu(
                &draw_triangles,
                &mut dest,
                depth_pixels.as_deref_mut(),
                &mut self.gpu,
            )
        {
            raster.render(
                &draw_triangles,
                &mut dest,
                depth_pixels.as_deref_mut(),
                workers,
            )?;
        }
        resource::write(mem, &b, &dest)?;
        if let (Some(ds), Some(values)) = (depth_buffer, depth_pixels) {
            resource::write(mem, &ds.borrow(), &values)?;
        }
        self.draws += 1;
        if self.first_draw_seconds.is_none() {
            self.first_draw_seconds = self.run_start.map(|t| t.elapsed().as_secs_f64());
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_row_bands_match_serial_color_depth_stencil_and_triangle_order() {
        raster_comparison(false, false);
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a real Metal device; run with --ignored"]
    fn metal_pixels_match_software_coverage_masks_stencil_and_rounding_bounds() {
        raster_comparison(true, false);
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a real Metal device; run with --ignored"]
    fn metal_ps2_pixels_match_software_sampling_constants_and_masks() {
        raster_comparison(true, true);
    }
    fn raster_comparison(metal: bool, programmable: bool) {
        let mut gpu = super::super::gpu::GpuRaster::default();
        let target = Pixels {
            width: 47,
            height: 37, // uneven bands exercise the short final stripe
            format: 21,
            pool: 0,
            usage: 0,
            bits: 0,
            locked: false,
        };
        let texture = Pixels {
            width: 8,
            height: 8,
            ..target
        };
        let source: Vec<u32> = (0..64u32)
            .map(|i| 0x80000000 | i.wrapping_mul(0x182347) & 0xffffff)
            .collect();
        let tokens = [
            0xffff0200, 0x03000042, 0x800f0000, 0xb0e40000, 0xa0e40800, 0x03000042, 0x800f0002,
            0xb0e40001, 0xa0e40801, 0x04000012, 0x800f0000, 0xa0e40002, 0x80e40000, 0x80e40002,
            0x04000004, 0x801f0001, 0x80e40000, 0xa0e40000, 0xa0e40001, 0x03000005, 0x800f0800,
            0x80c60001, 0x90e40000, 0xffff,
        ];
        let program =
            shader::Program::parse(|i| tokens.get(i).copied().ok_or("truncated".into())).unwrap();
        let mut constants = [[0.; 4]; 32];
        constants[0] = [0.8, 1.2, 0.5, 0.9];
        constants[1] = [0.05, -0.1, 0.2, 0.1];
        constants[2] = [0.25; 4];
        let texture1 = Pixels {
            width: 3,
            height: 5,
            ..texture
        };
        let source1: Vec<_> = (0..15u32).map(|i| 0xb0472000 | (i * 0x117)).collect();
        let sampling1 = Sampling {
            address: [3, 1],
            filter: [2, 2],
        };
        for case in 0..8 {
            let mut seed = 0x83023b47u32;
            let mut triangles = Vec::new();
            for _ in 0..24 {
                triangles.push(std::array::from_fn(|i| {
                    let mut next = || {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed >> 16) as f64 / 65536.
                    };
                    Vertex {
                        x: next() * 70. - 10.,
                        y: next() * 60. - 10.,
                        z: next(),
                        u: next() * 2. - 0.5,
                        v: next() * 2. - 0.5,
                        uv1: [0.2, 0.3],
                        color: if case & 1 == 0 {
                            [255.; 4]
                        } else {
                            std::array::from_fn(|_| (next() * 255.).round())
                        },
                        rhw: if case & 2 == 0 { 1. } else { 0.5 + i as f64 },
                    }
                }));
            }
            let mut r = [0; 175];
            for (n, value) in [
                (7, 1),
                (14, 1),
                (15, 1),
                (19, 5),
                (20, 6),
                (22, 1),
                (23, 4),
                (24, 70),
                (25, 7),
                (27, 1),
                (52, 1),
                (53, 7),
                (54, 8),
                (55, 3),
                (56, 4),
                (57, 7),
                (58, 255),
                (59, 15),
            ] {
                r[n] = value;
            }
            let sampling = Sampling {
                address: [1, 3],
                filter: if case & 4 == 0 { [1, 1] } else { [2, 2] },
            };
            let raster = Raster {
                source: &source,
                texture: &texture,
                target: &target,
                render: &r,
                stage: &[0, 4, 2, 0, 4, 2, 0],
                sampling: &sampling,
                color_mask: 0xffff00ff,
                clip: [3, 2, 44, 35],
                depth_width: 53, // a wider depth attachment must use its own pitch
                shader: programmable.then_some((&program, &constants)),
                extra: programmable.then_some((&texture1, &source1, &sampling1)),
            };
            for with_depth in [false, true] {
                let initial = vec![0x49382617; 47 * 37];
                let depth = vec![0xffffff03; 53 * 40];
                let mut serial = initial.clone();
                let mut serial_depth = depth.clone();
                raster
                    .render(
                        &triangles,
                        &mut serial,
                        with_depth.then_some(serial_depth.as_mut_slice()),
                        1,
                    )
                    .unwrap();
                if metal {
                    let mut pixels = initial.clone();
                    let mut values = depth.clone();
                    assert!(
                        raster.gpu(
                            &triangles,
                            &mut pixels,
                            with_depth.then_some(values.as_mut_slice()),
                            &mut gpu
                        ),
                        "{:?}",
                        gpu.error
                    );
                    for (i, (&actual, &expected)) in pixels.iter().zip(&serial).enumerate() {
                        assert_eq!(
                            actual != initial[i],
                            expected != initial[i],
                            "coverage case {case} pixel {i}"
                        );
                        assert_eq!(actual & !raster.color_mask, expected & !raster.color_mask);
                        for channel in 0..4 {
                            assert!(
                                ((actual >> (channel * 8)) & 255)
                                    .abs_diff((expected >> (channel * 8)) & 255)
                                    <= 1,
                                "case {case} pixel {i}: {actual:08x} vs {expected:08x}"
                            );
                        }
                    }
                    for (&actual, &expected) in values.iter().zip(&serial_depth) {
                        assert_eq!(actual & 255, expected & 255, "stencil case {case}");
                        assert!(
                            (actual >> 8).abs_diff(expected >> 8)
                                <= if programmable { 8 } else { 4 },
                            "depth case {case}: {actual:08x} vs {expected:08x}"
                        );
                    }
                    continue;
                }
                for workers in [2, 3, 4] {
                    let mut parallel = initial.clone();
                    let mut parallel_depth = depth.clone();
                    raster
                        .render(
                            &triangles,
                            &mut parallel,
                            with_depth.then_some(parallel_depth.as_mut_slice()),
                            workers,
                        )
                        .unwrap();
                    assert_eq!(parallel, serial, "case {case}, workers {workers}");
                    assert_eq!(
                        parallel_depth, serial_depth,
                        "depth case {case}, workers {workers}"
                    );
                }
            }
        }
    }
    #[test]
    fn scanline_bounds_keep_every_pixel_owned_by_exact_integer_halfplanes() {
        let mut seed = 0x43a218e5u32;
        for _ in 0..2000 {
            let mut points = [[0i64; 2]; 3];
            for p in &mut points {
                for n in p {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    *n = (seed % 1025) as i64 - 512;
                }
            }
            let area = (points[1][0] - points[0][0]) * (points[2][1] - points[0][1])
                - (points[1][1] - points[0][1]) * (points[2][0] - points[0][0]);
            if area == 0 {
                continue;
            }
            if area < 0 {
                points.swap(1, 2);
            }
            let v = points.map(|[x, y]| Vertex {
                x: x as f64 / 8.,
                y: y as f64 / 8.,
                z: 0.,
                u: 0.,
                v: 0.,
                uv1: [0.; 2],
                color: [255.; 4],
                rhw: 1.,
            });
            for y in -16..=16 {
                let (left, right) = row_span(&v, y, -16, 16);
                for x in -16..=16 {
                    let owned = (0..3).all(|i| {
                        let a = points[i];
                        let b = points[(i + 1) % 3];
                        let dx = b[0] - a[0];
                        let dy = b[1] - a[1];
                        let n = dx * (y as i64 * 8 - a[1]) - dy * (x as i64 * 8 - a[0]);
                        n > 0 || (n == 0 && (dy < 0 || (dy == 0 && dx > 0)))
                    });
                    if owned {
                        assert!(
                            left <= x && x <= right,
                            "{points:?}: ({x},{y}) outside {left}..{right}"
                        );
                    }
                }
            }
        }
    }
}
