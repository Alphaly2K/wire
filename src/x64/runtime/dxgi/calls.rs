//! DXGI factory and scalar object semantics; presentation stays with the runtime coordinator.
use super::*;
use crate::x64::runtime::{gateways::Gateways, user::User};

impl Graphics {
    pub(super) fn export_call(
        &mut self,
        mem: &mut Memory,
        gateways: &mut Gateways,
        next: &mut u64,
        id: GraphicsExportApi,
        a: &[u64],
    ) -> Result<u64, String> {
        if matches!(
            id,
            GraphicsExportApi::D3d12Device | GraphicsExportApi::D3d12Debug
        ) {
            // Unity probes D3D12 before falling back to D3D11. There is no
            // D3D12 backend; report actual lack of support, including when
            // ppDevice is NULL (a capability query), rather than issue a device.
            let (iid, out) = if id == GraphicsExportApi::D3d12Device {
                (a[2], a[3])
            } else {
                (a[0], a[1])
            };
            if iid == 0 {
                return Ok(INVALID_ARG);
            }
            if out != 0 {
                mem.check(out, 8, WRITE)?;
            }
            read_guid(mem, iid)?;
            if out != 0 {
                mem.write_int(out, 8, 0)?;
            }
            return Ok(if id == GraphicsExportApi::D3d12Debug {
                NO_INTERFACE
            } else {
                UNSUPPORTED
            });
        }
        let (flags, iid, out) = if id == GraphicsExportApi::Factory2 {
            (a[0], a[1], a[2])
        } else {
            (0, a[0], a[1])
        };
        if out == 0 || iid == 0 {
            return Ok(INVALID_ARG);
        }
        mem.check(out, 8, WRITE)?;
        let iid = read_guid(mem, iid)?;
        let result = if flags & !1 != 0 {
            INVALID
        } else if flags != 0 {
            0x887a002d
        }
        // debug layer not installed
        else if !supports(Kind::Factory, &iid) {
            NO_INTERFACE
        } else if self.objects.len() >= OBJECT_LIMIT {
            OUT_OF_MEMORY
        } else {
            0
        };
        if result != 0 {
            mem.write_int(out, 8, 0)?;
            return Ok(result);
        }
        self.info()?;
        let object = self.object(mem, gateways, next, Kind::Factory, 0)?;
        mem.write_int(out, 8, object)?;
        Ok(0)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn scalar_call(
        &mut self,
        mem: &mut Memory,
        gateways: &mut Gateways,
        next: &mut u64,
        user: &User,
        parent: u64,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        match name {
            "Adapter.EnumOutputs" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                if a[1] != 0 {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NOT_FOUND);
                }
                self.output(mem, gateways, next, a[0], a[2])
            }
            "Object.QueryInterface" | "Object.GetParent" => {
                if a[1] == 0 || a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                let iid = read_guid(mem, a[1])?;
                let target = if name == "Object.GetParent" {
                    parent
                } else {
                    a[0]
                };
                if target == 0 {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NO_INTERFACE);
                }
                if !supports(self.objects[&target].kind, &iid) {
                    mem.write_int(a[2], 8, 0)?;
                    return Ok(NO_INTERFACE);
                }
                self.retain(target)?;
                mem.write_int(a[2], 8, target)?;
                Ok(0)
            }
            "Object.AddRef" => self.retain(a[0]),
            "Object.Release" => self.release(mem, a[0]),
            "Object.SetPrivateData"
            | "Object.SetPrivateDataInterface"
            | "Object.GetPrivateData" => self.private_call(mem, name, a),
            "Factory.EnumAdapters" | "Factory.EnumAdapters1" | "Factory.EnumAdapterByLuid" => {
                let out = a[if name.ends_with("Luid") { 3 } else { 2 }];
                if out == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(out, 8, WRITE)?;
                let iid = if name.ends_with("Luid") {
                    read_guid(mem, a[2])?
                } else {
                    ADAPTERS[1]
                };
                let found = if name.ends_with("Luid") {
                    a[1] == self.luid()?
                } else {
                    a[1] == 0
                };
                if !found {
                    mem.write_int(out, 8, 0)?;
                    return Ok(NOT_FOUND);
                }
                self.adapter(mem, gateways, next, a[0], out, &iid)
            }
            "Factory.EnumWarpAdapter" => {
                if a[2] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.check(a[2], 8, WRITE)?;
                read_guid(mem, a[1])?;
                mem.write_int(a[2], 8, 0)?;
                Ok(NOT_FOUND)
            }
            "Factory.MakeWindowAssociation" => {
                if a[2] & !7 != 0 || (a[1] != 0 && !user.windows.contains_key(&a[1])) {
                    return Ok(INVALID);
                }
                // No native default Alt-Enter/PrintScreen handling is installed.
                self.objects.get_mut(&a[0]).unwrap().association = a[1];
                Ok(0)
            }
            "Factory.GetWindowAssociation" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                mem.write_int(a[1], 8, self.objects[&a[0]].association)?;
                Ok(0)
            }
            "Factory.IsCurrent" => Ok(1), // fixed attached device inventory; no hotplug enumeration
            "Factory.IsWindowedStereoEnabled" | "Factory.GetCreationFlags" => Ok(0),
            "Adapter.GetDesc" | "Adapter.GetDesc1" => {
                if a[1] == 0 {
                    return Ok(INVALID_ARG);
                }
                let mut bytes = vec![0; if name.ends_with('1') { 312 } else { 304 }];
                let info = self.info()?;
                for (i, c) in info.name.encode_utf16().take(127).enumerate() {
                    bytes[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
                }
                bytes[256..260].copy_from_slice(&info.vendor.to_le_bytes());
                bytes[260..264].copy_from_slice(&info.device.to_le_bytes());
                for (at, value) in [
                    (272, info.dedicated_video),
                    (280, info.dedicated_system),
                    (288, info.shared_system),
                    (296, self.luid()?),
                ] {
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(a[1], &bytes)?;
                Ok(0)
            }
            // No UMD/D3D interface is implemented. Never report a fabricated version.
            "Adapter.CheckInterfaceSupport" => {
                read_guid(mem, a[1])?;
                Ok(UNSUPPORTED)
            }
            _ => Err(format!("unimplemented Win64 DXGI method: {name}")),
        }
    }
}
