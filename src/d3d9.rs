//! Bounded CPU D3D9 backend for pretransformed two-dimensional textured draws.
//! ABI data is generated from Hangover's pinned Wine header. No Wine renderer,
//! shaders, machine-code generator or platform driver is linked here.
use crate::{
    api::{ApiState, ApiTable, Args},
    mem::GuestMem,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
mod device;
mod raster;
mod resource;
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
struct Device {
    factory: u32,
    hwnd: u32,
    flags: u32,
    params: [u32; 14],
    back: u32,
    target: u32,
    texture: u32,
    fvf: u32,
    scene: bool,
    render: BTreeMap<u32, u32>,
    sampler: BTreeMap<u32, u32>,
    stage: BTreeMap<u32, u32>,
    viewport: [u32; 6],
    scissor: [i32; 4],
    swap: u32,
}
enum Payload {
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
    objects: BTreeMap<u32, Object>,
    vtables: BTreeMap<&'static str, u32>,
    allocated: u64,
    pub frames: u64,
    pub draws: u64,
    pub last_presented: u32,
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
            for p in o.parents.into_iter().chain(o.children) {
                if let Some(o) = self.objects.get_mut(&p) {
                    o.pins = o.pins.checked_sub(1).expect("D3D owned reference");
                    work.push(p);
                }
            }
            if let Payload::Surface { data, .. } | Payload::Texture { data, .. } = o.payload {
                self.free_buffer(data, state);
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
        if !matches!(format, 21 | 22) {
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
        "CPU D3D9 {name}; frames={} draws={}",
        backend.frames, backend.draws
    );
    if name == "IDirect3DDevice9.DrawPrimitiveUP" && result.is_err() {
        if let Ok(v) = words::<6>(mem, a[3]) {
            note.push_str(&format!("; first XYZRHW/UV {:?}", v.map(f32::from_bits)));
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
            if a[0] != 32 {
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
            "IDirect3DDevice9" => self.device_call(method, a, mem, api, state),
            "IDirect3DTexture9" | "IDirect3DSurface9" => self.resource(method, a, mem),
            "IDirect3DSwapChain9" => self.swap(method, a, mem, state),
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
                    && a[4] & !0x201 == 0
                    && matches!(a[5], 1 | 3)
                    && matches!(a[6], 21 | 22)
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
                if params[4] != 0 || params[9] != 0 || params[3] > 1 {
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
                        texture: 0,
                        fvf: 0,
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
                put(mem, a[5], &params)?;
                put(mem, a[6], &[device])?;
                Ok(0)
            }
            _ => Err(UNAVAILABLE),
        }
    }
}
