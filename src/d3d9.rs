//! Bounded fixed-function D3D9 backend with optional public Metal acceleration.
//! ABI data comes from Hangover's pinned Wine header. Guest code stays in the
//! CPU interpreter; Metal executes only wire's static host graphics shader.
use crate::{
    api::{ApiState, ApiTable, Args},
    mem::GuestMem,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
mod device;
mod gpu;
mod raster;
mod reset;
mod resource;
mod shader;
mod stateblock;
mod vertex;
mod vertex_shader;
const INVALID: u32 = 0x8876086c;
const UNAVAILABLE: u32 = 0x8876086a;
const NOMEM: u32 = 0x8007000e;
const ABI: &str = include_str!("../third_party/hangover/d3d9-methods.tsv");
pub(crate) fn stack_bytes(name: &str) -> Option<u32> {
    if name == "Direct3DCreate9" {
        return Some(4);
    }
    ABI.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?, f.nth(1)?.parse::<u32>().ok()?))
        })
        .find_map(|(n, b)| (n == name).then_some(b))
}
struct Pixels {
    width: u32,
    height: u32,
    format: u32,
    pool: u32,
    usage: u32,
    bits: u32,
    locked: bool,
}
impl Pixels {
    fn len(&self) -> u32 {
        self.width * self.height * 4
    }
    fn pitch(&self) -> u32 {
        self.width * 4
    }
}
type Buffer = Rc<RefCell<Pixels>>;
#[derive(Clone)]
struct Device {
    factory: u32,
    hwnd: u32,
    flags: u32,
    params: [u32; 14],
    back: u32,
    target: u32,
    depth: u32,
    stream: [u32; 3], // buffer, byte offset, stride (stream zero)
    indices: u32,
    texture: u32,
    texture1: u32,
    fvf: u32,
    declaration: u32,
    vertex_shader: u32,
    vertex_constants: [[f32; 4]; 256],
    pixel_shader: u32,
    pixel_constants: [[f32; 4]; 32],
    transforms: BTreeMap<u32, [f32; 16]>,
    scene: bool,
    render: BTreeMap<u32, u32>,
    sampler: BTreeMap<u32, u32>,
    stage: BTreeMap<u32, u32>,
    viewport: [u32; 6],
    scissor: [i32; 4],
    swap: u32,
}
enum Payload {
    StateBlock {
        device: u32,
        snapshot: Box<Device>,
        keys: std::collections::BTreeSet<stateblock::Key>,
    },
    VertexShader {
        device: u32,
        program: vertex_shader::VertexProgram,
    },
    PixelShader {
        device: u32,
        program: shader::Program,
    },
    Factory,
    Device(Box<Device>),
    Texture {
        device: u32,
        surface: u32,
        data: Buffer,
    },
    Surface {
        device: u32,
        container: u32,
        data: Buffer,
    },
    Swap {
        device: u32,
    },
    Vertices {
        device: u32,
        bits: u32,
        len: u32,
        usage: u32,
        pool: u32,
        fvf: u32,
        locked: bool,
    },
    Declaration {
        device: u32,
        elements: Vec<u8>,
        layout: vertex::Layout,
    },
}
struct Object {
    kind: &'static str,
    refs: u32,
    pins: u32,
    parents: Vec<u32>,
    leases: Vec<u32>,
    children: Vec<u32>,
    priority: u32,
    payload: Payload,
}
#[derive(Default)]
pub struct D3D9 {
    pub enable_gpu: bool,
    shader_error: Option<String>,
    pub shader_dump_dir: Option<std::path::PathBuf>,
    gpu: gpu::GpuRaster,
    objects: BTreeMap<u32, Object>,
    recording: BTreeMap<u32, stateblock::Recording>,
    vtables: BTreeMap<&'static str, u32>,
    allocated: u64,
    pub frames: u64,
    pub draws: u64,
    pub last_presented: u32,
    pub run_start: Option<std::time::Instant>,
    pub first_draw_seconds: Option<f64>,
    pub first_present_seconds: Option<f64>,
}
fn put(mem: &GuestMem, p: u32, words: &[u32]) -> Result<(), u32> {
    mem.write_bytes(
        p,
        &words
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .map_err(|_| INVALID)
}
fn words<const N: usize>(mem: &GuestMem, p: u32) -> Result<[u32; N], u32> {
    let mut bytes = vec![0; N * 4];
    mem.read_bytes(p, &mut bytes).map_err(|_| INVALID)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
impl D3D9 {
    pub fn prepare_gpu(&mut self) {
        if self.enable_gpu {
            self.gpu.prepare();
        }
    }
    pub fn gpu_draws(&self) -> u64 {
        self.gpu.draws
    }
    pub fn gpu_error(&self) -> Option<&str> {
        self.gpu.error.as_deref()
    }
    fn pin(&mut self, h: u32) -> Result<(), u32> {
        let o = self.objects.get_mut(&h).ok_or(INVALID)?;
        o.pins = o.pins.checked_add(1).ok_or(NOMEM)?;
        Ok(())
    }
    fn addref(&mut self, h: u32) -> Result<u32, u32> {
        let o = self.objects.get_mut(&h).ok_or(INVALID)?;
        let leases = if o.refs == 0 {
            o.leases.clone()
        } else {
            Vec::new()
        };
        o.refs = o.refs.checked_add(1).ok_or(NOMEM)?;
        let count = o.refs;
        for p in leases {
            self.pin(p)?;
        }
        Ok(count)
    }
    fn free_buffer(&mut self, data: Buffer, state: &mut ApiState) {
        if Rc::strong_count(&data) == 1 {
            let p = data.borrow();
            self.allocated -= p.len() as u64;
            state.heap.free(state.process_heap, p.bits);
        }
    }
    fn collect(&mut self, mut work: Vec<u32>, state: &mut ApiState) {
        while let Some(h) = work.pop() {
            if !self
                .objects
                .get(&h)
                .is_some_and(|o| o.refs == 0 && o.pins == 0)
            {
                continue;
            }
            let o = self.objects.remove(&h).unwrap();
            let recording = self.recording.remove(&h);
            for p in o
                .parents
                .into_iter()
                .chain(o.children)
                .chain(recording.into_iter().flat_map(|r| r.children))
            {
                if let Some(o) = self.objects.get_mut(&p) {
                    o.pins = o.pins.checked_sub(1).expect("D3D owned reference");
                    work.push(p);
                }
            }
            match o.payload {
                Payload::Surface { data, .. } | Payload::Texture { data, .. } => {
                    self.free_buffer(data, state)
                }
                Payload::Vertices { bits, len, .. } => {
                    self.allocated -= len as u64;
                    state.heap.free(state.process_heap, bits);
                }
                _ => {}
            }
            state.heap.free(state.process_heap, h);
        }
    }
    fn unpin(&mut self, h: u32, state: &mut ApiState) {
        let o = self.objects.get_mut(&h).expect("D3D pinned object");
        o.pins = o.pins.checked_sub(1).expect("D3D pin balance");
        self.collect(vec![h], state);
    }
    fn release(&mut self, h: u32, state: &mut ApiState) -> Result<u32, u32> {
        let o = self.objects.get_mut(&h).ok_or(INVALID)?;
        if o.refs == 0 {
            return Err(INVALID);
        }
        o.refs -= 1;
        let count = o.refs;
        let leases = if count == 0 {
            o.leases.clone()
        } else {
            Vec::new()
        };
        let mut work = vec![h];
        for p in leases {
            let o = self.objects.get_mut(&p).expect("D3D lease owner");
            o.pins = o.pins.checked_sub(1).expect("D3D lease balance");
            work.push(p);
        }
        self.collect(work, state);
        Ok(count)
    }
    #[allow(clippy::too_many_arguments)]
    fn object(
        &mut self,
        kind: &'static str,
        payload: Payload,
        refs: u32,
        parents: Vec<u32>,
        leases: Vec<u32>,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        if self.objects.len() >= 4096 {
            return Err(NOMEM);
        }
        let mut pins = BTreeMap::<u32, u32>::new();
        for &p in parents.iter().chain(leases.iter().filter(|_| refs != 0)) {
            *pins.entry(p).or_default() += 1;
        }
        for (&p, &n) in &pins {
            self.objects
                .get(&p)
                .ok_or(INVALID)?
                .pins
                .checked_add(n)
                .ok_or(NOMEM)?;
        }
        let table = if let Some(&p) = self.vtables.get(kind) {
            p
        } else {
            let entries: Vec<_> = ABI
                .lines()
                .filter(|l| l.starts_with(kind) && l.as_bytes().get(kind.len()) == Some(&b'.'))
                .map(|l| api.register("d3d9.dll", Some(l.split('\t').next().unwrap()), None))
                .collect();
            let p = alloc(mem, state, entries.len() as u32 * 4)?;
            put(mem, p, &entries)?;
            self.vtables.insert(kind, p);
            p
        };
        let h = alloc(mem, state, 4)?;
        put(mem, h, &[table])?;
        for (p, n) in pins {
            self.objects.get_mut(&p).unwrap().pins += n;
        }
        self.objects.insert(
            h,
            Object {
                kind,
                refs,
                pins: 0,
                parents,
                leases,
                children: Vec::new(),
                priority: 0,
                payload,
            },
        );
        Ok(h)
    }
    #[allow(clippy::too_many_arguments)]
    fn pixels(
        &mut self,
        w: u32,
        h: u32,
        format: u32,
        pool: u32,
        usage: u32,
        mem: &mut GuestMem,
        state: &mut ApiState,
    ) -> Result<Buffer, u32> {
        if !(matches!(format, 21 | 22) && usage & 2 == 0 || format == 75 && usage == 2) {
            return Err(UNAVAILABLE);
        }
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err(INVALID);
        }
        let size = w
            .checked_mul(h)
            .and_then(|n| n.checked_mul(4))
            .ok_or(NOMEM)?;
        if size > 64 * 1024 * 1024 || self.allocated + size as u64 > 256 * 1024 * 1024 {
            return Err(NOMEM);
        }
        let bits = alloc(mem, state, size)?;
        self.allocated += size as u64;
        Ok(Rc::new(RefCell::new(Pixels {
            width: w,
            height: h,
            format,
            pool,
            usage,
            bits,
            locked: false,
        })))
    }
    fn device(&self, h: u32) -> Result<&Device, u32> {
        match &self.objects.get(&h).ok_or(INVALID)?.payload {
            Payload::Device(d) => Ok(d),
            _ => Err(INVALID),
        }
    }
    fn device_mut(&mut self, h: u32) -> Result<&mut Device, u32> {
        match &mut self.objects.get_mut(&h).ok_or(INVALID)?.payload {
            Payload::Device(d) => Ok(d),
            _ => Err(INVALID),
        }
    }
    fn buffer(&self, h: u32) -> Result<Buffer, u32> {
        match &self.objects.get(&h).ok_or(INVALID)?.payload {
            Payload::Texture { data, .. } | Payload::Surface { data, .. } => Ok(data.clone()),
            _ => Err(INVALID),
        }
    }
    fn output_ref(&mut self, h: u32, mem: &GuestMem, p: u32) -> Result<u32, u32> {
        mem.ptr(p, 4, true).map_err(|_| INVALID)?;
        self.addref(h)?;
        put(mem, p, &[h])?;
        Ok(0)
    }
}
fn alloc(mem: &mut GuestMem, state: &mut ApiState, size: u32) -> Result<u32, u32> {
    let h = state
        .heap
        .alloc(mem, state.process_heap, size, true)
        .map_err(|_| NOMEM)?;
    if h == 0 {
        Err(NOMEM)
    } else {
        Ok(h)
    }
}
fn caps(mem: &GuestMem, p: u32) -> Result<(), u32> {
    let mut c = [0u32; 76];
    for (i, n) in [
        (0, 1),
        (3, 0x20000000),
        (5, 0x80000001),
        (7, 0x40),
        (11, 0x32),
        (12, 0x32),
        (13, 0xff),
        (15, 4),
        (16, 0x3000300),
        (19, 5),
        (22, 16384),
        (23, 16384),
        (25, 16384),
        (26, 16384),
        (27, 1),
        (28, 1f32.to_bits()),
        (35, 1),
        (36, 0xb),
        (37, 1),
        (38, 1),
        (45, 2),
        (46, 4),
        (47, 1),
        (48, 28),
        (49, 0xfffe0200),
        (50, 256),
        (51, 0xffff0200),
        (52, 1f32.to_bits()),
        (66, 1), // PS20 arbitrary swizzle
        (68, 12),
        (70, 96),
        (73, 96),
        (58, 1),
        (60, 1),
        (61, 0x3000300),
    ] {
        c[i] = n;
    }
    put(mem, p, &c)
}
fn desc(mem: &GuestMem, p: u32, b: &Pixels) -> Result<(), u32> {
    put(
        mem,
        p,
        &[b.format, 1, b.usage, b.pool, 0, 0, b.width, b.height],
    )
}
pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &Args,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
) -> Option<(u32, String, bool)> {
    if dll != "d3d9.dll" {
        return None;
    }
    let mut backend = std::mem::take(&mut state.d3d9);
    let result = backend.call(name, a, mem, api, state);
    let mut note = format!(
        "D3D9 {name}; frames={} draws={}",
        backend.frames, backend.draws
    );
    if name.ends_with(".CreatePixelShader") || name.ends_with(".CreateVertexShader") {
        note.push_str(&format!("; shader_error={:?}", backend.shader_error));
    }
    if name.ends_with(".Present") {
        note.push_str(&format!(
            "; Metal={} GPU_error={:?}",
            backend.gpu_draws(),
            backend.gpu_error()
        ));
    }
    if name == "IDirect3D9.CheckDeviceFormat" {
        note.push_str(&format!("; format={}", a[6]));
    }
    if name == "IDirect3D9.CreateDevice" {
        if let Ok(p) = words::<14>(mem, a[5]) {
            note.push_str(&format!("; params={p:?}"));
        }
    }
    if name == "IDirect3DDevice9.DrawPrimitiveUP" && result.is_err() {
        if let Ok(v) = words::<6>(mem, a[3]) {
            note.push_str(&format!("; first XYZRHW/UV {:?}", v.map(f32::from_bits)));
        }
    }
    if name.starts_with("IDirect3DDevice9.Draw") && result.is_err() {
        if let Ok(d) = backend.device(a[0]) {
            note.push_str(&format!(
                "; draw_args={a:x?} fvf={:#x} declaration={:#x} VS={:#x} PS={:#x} textures={:x?} viewport={:?} render={:?} sampler={:?} stage={:?}",
                d.fvf, d.declaration, d.vertex_shader, d.pixel_shader,
                [d.texture, d.texture1], d.viewport, d.render, d.sampler, d.stage
            ));
            if let Some(Object {
                payload: Payload::Vertices { bits, .. },
                ..
            }) = backend.objects.get(&d.stream[0])
            {
                let values: Vec<_> = (0..4)
                    .filter_map(|i| {
                        bits.checked_add(d.stream[1])?
                            .checked_add(i * d.stream[2])
                            .and_then(|p| words::<7>(mem, p).ok())
                    })
                    .collect();
                note.push_str(&format!("; first stream vertices={values:x?}"));
            }
        }
    }
    if name == "IDirect3DDevice9.SetViewport" {
        if let Ok(v) = words::<6>(mem, a[1]) {
            note.push_str(&format!("; viewport={v:?}"));
        }
    }
    if name == "IDirect3DDevice9.Reset" && result.is_ok() {
        if let Ok(d) = backend.device(a[0]) {
            note.push_str(&format!("; reset params={:?}", d.params));
        }
    }
    if name == "IDirect3DDevice9.DrawPrimitiveUP" && backend.draws <= 10 {
        if let Ok(d) = backend.device(a[0]) {
            note.push_str(&format!(
                "; fvf={:#x} viewport={:?} transforms={:?}",
                d.fvf, d.viewport, d.transforms
            ));
            if let Ok(layout) = backend.layout(d) {
                let n = if a[1] == 4 { a[2] * 3 } else { a[2] + 2 };
                let values: Vec<_> = (0..n.min(4))
                    .filter_map(|i| words::<7>(mem, a[3] + i * a[4]).ok())
                    .collect();
                note.push_str(&format!(
                    "; layout={} first vertices={values:x?}",
                    layout.size
                ));
            }
        }
    }
    state.d3d9 = backend;
    Some(match result {
        Ok(n) => (n, note, true),
        Err(e) => (
            if name == "Direct3DCreate9" { 0 } else { e },
            format!("{note}; error {e:#x}"),
            e != UNAVAILABLE,
        ),
    })
}
impl D3D9 {
    fn call(
        &mut self,
        name: &str,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        if name == "Direct3DCreate9" {
            // D3D9 SDK 31 predates the current SDK 32; both use this interface.
            if !matches!(a[0] & 0x7fff_ffff, 31 | 32) {
                return Err(INVALID);
            }
            return self.object(
                "IDirect3D9",
                Payload::Factory,
                1,
                vec![],
                vec![],
                mem,
                api,
                state,
            );
        }
        let (kind, method) = name.split_once('.').ok_or(UNAVAILABLE)?;
        let obj = self.objects.get(&a[0]).ok_or(INVALID)?;
        if obj.kind != kind {
            return Err(INVALID);
        }
        match method {
            "AddRef" => return self.addref(a[0]),
            "Release" => return self.release(a[0], state),
            "QueryInterface" => {
                mem.ptr(a[2], 4, true).map_err(|_| INVALID)?;
                put(mem, a[2], &[0])?;
                let iid = words::<4>(mem, a[1])?;
                let expected = match kind {
                    "IDirect3D9" => [0x81bdcbca, 0x426d64d4, 0x01ad8dae, 0x5c27f447],
                    "IDirect3DDevice9" => [0xd0223b96, 0x43fdbf7a, 0x3ba4bd92, 0xebb9820d],
                    "IDirect3DTexture9" => [0x85c31227, 0x4f003de5, 0x1af13a9b, 0xb5188c3c],
                    "IDirect3DSurface9" => [0x0cfbaf3a, 0x429a9ff6, 0x79a2b399, 0x9bb8f86a],
                    "IDirect3DVertexBuffer9" => [0xb64bb1b5, 0x4df6fd70, 0xd01991bf, 0xe35524a1],
                    "IDirect3DIndexBuffer9" => [0x7c9dd65e, 0x4529d3f7, 0x5878eeac, 0x35deac30],
                    "IDirect3DVertexShader9" => [0xefc5557e, 0x46136265, 0x8543948a, 0x36eb8978],
                    "IDirect3DPixelShader9" => [0x6d3bdbdc, 0x44155b02, 0x5ece52b8, 0x89b2cc8b],
                    "IDirect3DVertexDeclaration9" => {
                        [0xdd13c59c, 0x409836fa, 0xedc7fba8, 0x4685dc39]
                    }
                    "IDirect3DStateBlock9" => [0xb07c4fe5, 0x4ba8310d, 0x0f4f3ca2, 0x8b216f20],
                    _ => [0x794950f2, 0x458aadfc, 0xa1105e90, 0x3b500b0b],
                };
                if iid != expected && iid != [0, 0, 0xc0, 0x46000000] {
                    return Err(0x80004002);
                }
                return self.output_ref(a[0], mem, a[2]);
            }
            _ => {}
        }
        match kind {
            "IDirect3D9" => self.factory(method, a, mem, api, state),
            "IDirect3DDevice9" => self.recorded_device_call(method, a, mem, api, state),
            "IDirect3DTexture9" | "IDirect3DSurface9" => self.resource(method, a, mem),
            "IDirect3DSwapChain9" => self.swap(method, a, mem, state),
            "IDirect3DVertexBuffer9" | "IDirect3DIndexBuffer9" => self.vertex_call(method, a, mem),
            "IDirect3DVertexDeclaration9" => self.declaration_call(method, a, mem),
            "IDirect3DPixelShader9" | "IDirect3DVertexShader9" => self.shader_call(method, a, mem),
            "IDirect3DStateBlock9" => self.stateblock_call(method, a, mem, state),
            _ => Err(UNAVAILABLE),
        }
    }
    fn factory(
        &mut self,
        m: &str,
        a: &Args,
        mem: &mut GuestMem,
        api: &mut ApiTable,
        state: &mut ApiState,
    ) -> Result<u32, u32> {
        match m {
            "GetAdapterCount" => Ok(1),
            "GetAdapterMonitor" => Ok(if a[1] == 0 { 0x40000001 } else { 0 }),
            "GetAdapterModeCount" => Ok((a[1] == 0 && matches!(a[2], 21 | 22)) as u32),
            "EnumAdapterModes" => {
                if a[1] != 0 || a[3] != 0 || !matches!(a[2], 21 | 22) {
                    return Err(INVALID);
                }
                put(mem, a[4], &[1024, 768, 60, a[2]])?;
                Ok(0)
            }
            "GetAdapterDisplayMode" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                put(mem, a[2], &[1024, 768, 60, 22])?;
                Ok(0)
            }
            "GetAdapterIdentifier" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                let mut data = vec![0; 1100];
                for (off, text) in [
                    (0, "wire-cpu-raster"),
                    (512, "wire CPU 2D raster device"),
                    (1024, "\\\\.\\DISPLAY1"),
                ] {
                    data[off..off + text.len()].copy_from_slice(text.as_bytes());
                }
                mem.write_bytes(a[3], &data).map_err(|_| INVALID)?;
                Ok(0)
            }
            "CheckDeviceType" => {
                if a[1] == 0
                    && matches!(a[2], 1 | 2)
                    && matches!(a[3], 21 | 22)
                    && matches!(a[4], 21 | 22)
                {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "CheckDeviceFormat" => {
                if a[1] == 0
                    && matches!(a[2], 1 | 2)
                    && matches!(a[3], 21 | 22)
                    && ((a[4] & !0x201 == 0 && matches!(a[5], 1 | 3) && matches!(a[6], 21 | 22))
                        || (a[4] == 2 && a[5] == 1 && a[6] == 75))
                {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "CheckDepthStencilMatch" => {
                if a[1] == 0
                    && matches!(a[2], 1 | 2)
                    && matches!(a[3], 21 | 22)
                    && matches!(a[4], 21 | 22)
                    && a[5] == 75
                {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "CheckDeviceFormatConversion" => {
                if a[1] == 0 && matches!(a[3], 21 | 22) && matches!(a[4], 21 | 22) {
                    Ok(0)
                } else {
                    Err(UNAVAILABLE)
                }
            }
            "CheckDeviceMultiSampleType" => {
                if a[1] != 0 || a[5] != 0 {
                    return Err(UNAVAILABLE);
                }
                if a[6] != 0 {
                    put(mem, a[6], &[1])?;
                }
                Ok(0)
            }
            "GetDeviceCaps" => {
                if a[1] != 0 {
                    return Err(INVALID);
                }
                caps(mem, a[3])?;
                Ok(0)
            }
            "CreateDevice" => {
                mem.ptr(a[6], 4, true).map_err(|_| INVALID)?;
                put(mem, a[6], &[0])?;
                if a[1] != 0 || !matches!(a[2], 1 | 2) {
                    return Err(UNAVAILABLE);
                }
                mem.ptr(a[5], 56, true).map_err(|_| INVALID)?;
                let mut params = words::<14>(mem, a[5])?;
                if params[4] != 0
                    || params[9] > 1
                    || params[3] > 1
                    || (params[9] == 1 && params[10] != 75)
                {
                    return Err(UNAVAILABLE);
                }
                let hwnd = if params[7] == 0 { a[3] } else { params[7] };
                let w = state.user32.windows.get(&hwnd).ok_or(INVALID)?;
                let size = w.client_size();
                if params[0] == 0 {
                    params[0] = size[0].max(1) as u32;
                }
                if params[1] == 0 {
                    params[1] = size[1].max(1) as u32;
                }
                if params[2] == 0 {
                    params[2] = 22;
                }
                params[3] = 1;
                params[7] = hwnd;
                let data = self.pixels(params[0], params[1], params[2], 0, 1, mem, state)?;
                let device_result = self.object(
                    "IDirect3DDevice9",
                    Payload::Device(Box::new(Device {
                        factory: a[0],
                        hwnd,
                        flags: a[4],
                        params,
                        back: 0,
                        target: 0,
                        depth: 0,
                        stream: [0; 3],
                        indices: 0,
                        texture: 0,
                        texture1: 0,
                        fvf: 0,
                        declaration: 0,
                        vertex_shader: 0,
                        vertex_constants: [[0.; 4]; 256],
                        pixel_shader: 0,
                        pixel_constants: [[0.; 4]; 32],
                        transforms: BTreeMap::new(),
                        scene: false,
                        render: BTreeMap::new(),
                        sampler: BTreeMap::new(),
                        stage: BTreeMap::new(),
                        viewport: [0, 0, params[0], params[1], 0, 1f32.to_bits()],
                        scissor: [0, 0, params[0] as i32, params[1] as i32],
                        swap: 0,
                    })),
                    1,
                    vec![a[0]],
                    vec![],
                    mem,
                    api,
                    state,
                );
                let device = match device_result {
                    Ok(d) => d,
                    Err(e) => {
                        self.free_buffer(data, state);
                        return Err(e);
                    }
                };
                let result = self.surface(device, data.clone(), 0, 0, mem, api, state);
                let surface = match result {
                    Ok(s) => s,
                    Err(e) => {
                        self.release(device, state)?;
                        self.free_buffer(data, state);
                        return Err(e);
                    }
                };
                self.pin(surface)?;
                self.pin(surface)?;
                self.objects
                    .get_mut(&device)
                    .unwrap()
                    .children
                    .extend([surface, surface]);
                let d = self.device_mut(device)?;
                d.back = surface;
                d.target = surface;
                if params[9] == 1 {
                    let result = self
                        .create_depth(device, params[0], params[1], mem, api, state)
                        .and_then(|depth| {
                            let result = self.bind_depth(device, depth, state);
                            self.release(depth, state)?;
                            result
                        });
                    if let Err(e) = result {
                        self.release(device, state)?;
                        return Err(e);
                    }
                }
                put(mem, a[5], &params)?;
                put(mem, a[6], &[device])?;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
}
