//! The guest's virtual desktop output. No native monitor, exclusive fullscreen,
//! gamma ramp, duplication or vblank synchronization is claimed.
//! Public Win64 layouts/slots reviewed against DXVK Native c8dc91fa dxgi*.h.
use super::*;
use crate::x64::runtime::display;
pub(super) const IIDS: [Guid; 2] = [
    guid(
        0xae02eedb,
        0xc735,
        0x4690,
        [0x8d, 0x52, 0x5a, 0x8d, 0xc2, 0x02, 0x13, 0xaa],
    ),
    guid(
        0x00cddea8,
        0x939b,
        0x4b83,
        [0xa3, 0x40, 0xa6, 0x85, 0x22, 0x66, 0x66, 0xcc],
    ),
];
pub(super) const METHODS: &[MethodAbi] = &[
    ("Output.GetDesc", 2, &[]),
    ("Output.GetDisplayModeList", 5, &[1, 2]),
    ("Output.FindClosestMatchingMode", 4, &[]),
    ("Output.WaitForVBlank", 1, &[]),
    ("Output.TakeOwnership", 3, &[2]),
    ("Output.ReleaseOwnership", 1, &[]),
    ("Output.GetGammaControlCapabilities", 2, &[]),
    ("Output.SetGammaControl", 2, &[]),
    ("Output.GetGammaControl", 2, &[]),
    ("Output.SetDisplaySurface", 2, &[]),
    ("Output.GetDisplaySurfaceData", 2, &[]),
    ("Output.GetFrameStatistics", 2, &[]),
    ("Output.GetDisplayModeList1", 5, &[1, 2]),
    ("Output.FindClosestMatchingMode1", 4, &[]),
    ("Output.GetDisplaySurfaceData1", 2, &[]),
    ("Output.DuplicateOutput", 3, &[]),
];
fn mode(format: u32, extended: bool) -> Vec<u8> {
    let mut fields = vec![
        display::WIDTH,
        display::HEIGHT,
        display::REFRESH_HZ,
        1,
        format,
        1,
        0,
    ];
    if extended {
        fields.push(0);
    } // Stereo FALSE.
    fields.into_iter().flat_map(u32::to_le_bytes).collect()
}
impl Runtime64 {
    pub(super) fn dxgi_output(
        &mut self,
        mem: &mut Memory,
        adapter: u64,
        out: u64,
    ) -> Result<u64, String> {
        let cached = self
            .graphics
            .objects
            .iter()
            .find_map(|(&p, o)| (o.kind == Kind::Output && o.parent == adapter).then_some(p));
        let address = if let Some(p) = cached {
            self.graphics.retain(p)?;
            p
        } else {
            if self.graphics.objects.len() >= OBJECT_LIMIT {
                mem.write_int(out, 8, 0)?;
                return Ok(OUT_OF_MEMORY);
            }
            self.graphics.object(
                mem,
                &mut self.gateways,
                &mut self.next_alloc,
                Kind::Output,
                adapter,
            )?
        };
        mem.write_int(out, 8, address)?;
        Ok(0)
    }
    pub(super) fn output_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let extended = name.ends_with('1');
        match name {
            "Output.GetDesc" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                let mut bytes = [0; 96];
                for (i, c) in display::DEVICE.encode_utf16().enumerate() {
                    bytes[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
                }
                bytes[72..76].copy_from_slice(&display::WIDTH.to_le_bytes());
                bytes[76..80].copy_from_slice(&display::HEIGHT.to_le_bytes());
                bytes[80..84].copy_from_slice(&1u32.to_le_bytes()); // Attached virtual desktop.
                bytes[84..88].copy_from_slice(&1u32.to_le_bytes()); // Identity rotation.
                bytes[88..96].copy_from_slice(&display::MONITOR.to_le_bytes());
                mem.write(a[1], &bytes)?;
                Ok(0)
            }
            "Output.GetDisplayModeList" | "Output.GetDisplayModeList1" => {
                if a[3] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[3], 4, READ | WRITE)?;
                if a[2] & !(if extended { 15 } else { 3 }) != 0 || a[1] == 0 {
                    return Ok(INVALID);
                }
                let capacity = mem.read_int(a[3], 4)?;
                // The current presentation profile is a single 32-bit mono mode.
                let supported = matches!(a[1], 28 | 87);
                if supported && a[4] != 0 && capacity >= 1 {
                    let bytes = mode(a[1] as u32, extended);
                    mem.check(a[4], bytes.len(), WRITE)?;
                    mem.write(a[4], &bytes)?;
                }
                mem.write_int(a[3], 4, supported as u64)?;
                Ok(if supported && a[4] != 0 && capacity == 0 {
                    MORE_DATA
                } else {
                    0
                })
            }
            "Output.FindClosestMatchingMode" | "Output.FindClosestMatchingMode1" => {
                if a[1] == 0 || a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                let size = if extended { 32 } else { 28 };
                let mut bytes = vec![0; size];
                mem.read(a[1], &mut bytes)?;
                mem.check(a[2], size, WRITE)?;
                let fields: Vec<_> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect();
                if (fields[0] == 0) != (fields[1] == 0)
                    || (fields[2] != 0 && fields[3] == 0)
                    || fields[5] > 3
                    || fields[6] > 2
                {
                    return Ok(INVALID);
                }
                if a[3] != 0 {
                    let Some(device) = self.graphics.objects.get(&a[3]) else {
                        return Ok(INVALID_ARG);
                    };
                    let owner = if device.kind == Kind::DxgiDevice {
                        device.parent
                    } else {
                        a[3]
                    };
                    if self.graphics.objects.get(&owner).is_none_or(|o| {
                        o.kind != Kind::Device || o.parent != self.graphics.objects[&a[0]].parent
                    }) {
                        return Ok(INVALID_ARG);
                    }
                }
                if fields[4] == 0 && a[3] == 0 {
                    return Ok(INVALID);
                }
                if (fields[4] != 0 && !matches!(fields[4], 28 | 87)) || (extended && fields[7] != 0)
                {
                    return Ok(NOT_FOUND);
                }
                mem.write(
                    a[2],
                    &mode(if fields[4] == 0 { 28 } else { fields[4] }, extended),
                )?;
                Ok(0)
            }
            "Output.ReleaseOwnership" => Ok(0), // No exclusive owner can be acquired.
            "Output.DuplicateOutput" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[2], 8, 0)?;
                Ok(UNSUPPORTED)
            }
            _ => Ok(UNSUPPORTED),
        }
    }
}
