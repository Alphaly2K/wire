//! Checked device discovery for the isolated profile, which has no PnP/HID
//! backend or owned HID report descriptors. Empty sets are real owned objects;
//! no physical device, interface path or report capability is fabricated.
//! Public ABI and errors reviewed against Hangover Wine devinst.c / hidd.c /
//! ddk/hidpi.h at 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
use super::{Memory, Runtime64, TEB, WRITE};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const HID_GUID: [u8; 16] = [
    0xb2, 0x55, 0x1e, 0x4d, 0x6f, 0xf1, 0xcf, 0x11, 0x88, 0xcb, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30,
];
pub(super) struct Devices {
    sets: BTreeSet<u64>,
    next: u64,
    notifications: BTreeMap<u64, (u64, Option<[u8; 16]>)>,
    next_notification: u64,
}
impl Default for Devices {
    fn default() -> Self {
        Self {
            sets: BTreeSet::new(),
            next: TEB + 0x680000,
            notifications: BTreeMap::new(),
            next_notification: TEB + 0x6a0000,
        }
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "HidD_GetHidGuid"
        | "HidD_FreePreparsedData"
        | "SetupDiDestroyDeviceInfoList"
        | "UnregisterDeviceNotification" => 1,
        "HidD_GetAttributes"
        | "HidD_GetPreparsedData"
        | "HidP_GetCaps"
        | "HidP_MaxDataListLength" => 2,
        "HidD_GetManufacturerString"
        | "HidD_GetProductString"
        | "HidD_GetSerialNumberString"
        | "SetupDiEnumDeviceInfo"
        | "RegisterDeviceNotificationA"
        | "RegisterDeviceNotificationW" => 3,
        "HidP_GetButtonCaps"
        | "HidP_GetValueCaps"
        | "SetupDiGetClassDevsA"
        | "SetupDiGetClassDevsW" => 4,
        "SetupDiEnumDeviceInterfaces" => 5,
        "HidP_GetData"
        | "SetupDiGetDeviceInterfaceDetailA"
        | "SetupDiGetDeviceInterfaceDetailW" => 6,
        "HidP_SetUsageValue" | "HidP_SetUsages" => 8,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    arity(name).is_some()
        && if name.starts_with("SetupDi") {
            dll.eq_ignore_ascii_case("setupapi.dll")
        } else if name.contains("DeviceNotification") {
            dll.eq_ignore_ascii_case("user32.dll")
        } else {
            dll.eq_ignore_ascii_case("hid.dll")
        }
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "HidD_GetManufacturerString" | "HidD_GetProductString" | "HidD_GetSerialNumberString" => {
            &[2]
        }
        "HidP_GetButtonCaps" | "HidP_GetValueCaps" | "HidP_MaxDataListLength" => &[0],
        "HidP_GetData" => &[0, 5],
        "HidP_SetUsageValue" => &[0, 1, 2, 3, 4, 7],
        "HidP_SetUsages" => &[0, 1, 2, 7],
        "SetupDiGetClassDevsA" | "SetupDiGetClassDevsW" => &[3],
        "SetupDiEnumDeviceInfo" => &[1],
        "RegisterDeviceNotificationA" | "RegisterDeviceNotificationW" => &[2],
        "SetupDiEnumDeviceInterfaces"
        | "SetupDiGetDeviceInterfaceDetailA"
        | "SetupDiGetDeviceInterfaceDetailW" => &[3],
        _ => &[],
    }
}
impl Runtime64 {
    pub(in crate::x64::runtime) fn remove_device_notifications(&mut self, window: u64) {
        self.devices
            .notifications
            .retain(|_, (owner, _)| *owner != window);
    }
    pub(super) fn device_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let failure = if name.starts_with("SetupDiGetClassDevs") {
            u64::MAX
        } else {
            0
        };
        let error = match name {
            "RegisterDeviceNotificationA" | "RegisterDeviceNotificationW" => {
                if a[2] & !5 != 0 {
                    87
                } else if a[2] & 1 != 0 {
                    50
                }
                // No service-control-manager profile.
                else if !self.user.windows.contains_key(&a[0]) {
                    87
                } else {
                    let mut class = None;
                    if a[1] != 0 {
                        let size = mem.read_int(a[1], 4)?;
                        let kind =
                            mem.read_int(a[1].checked_add(4).ok_or("device filter overflow")?, 4)?;
                        if size < 12 || (kind == 5 && a[2] & 4 == 0 && size < 28) {
                            self.set_error(mem, 87)?;
                            return Ok(0);
                        }
                        if kind != 5 {
                            self.set_error(mem, if kind == 6 { 50 } else { 13 })?;
                            return Ok(0);
                        }
                        if a[2] & 4 == 0 {
                            let mut filter = [0; 28];
                            mem.read(a[1], &mut filter)?;
                            class = Some(filter[12..].try_into().unwrap());
                        } else {
                            let mut header = [0; 12];
                            mem.read(a[1], &mut header)?;
                        }
                    }
                    // The current device database is static and empty. Keep a real
                    // subscription without inventing arrivals or physical devices.
                    if self.devices.notifications.len() >= 4096
                        || self.devices.next_notification >= TEB + 0x6b0000
                    {
                        8
                    } else {
                        let handle = self.devices.next_notification;
                        self.devices.notifications.insert(handle, (a[0], class));
                        self.devices.next_notification += 16;
                        return Ok(handle);
                    }
                }
            }
            "UnregisterDeviceNotification" => {
                if self.devices.notifications.contains_key(&a[0]) {
                    self.devices.notifications.remove(&a[0]);
                    return Ok(1);
                }
                6
            }
            "HidD_GetHidGuid" => {
                mem.write(a[0], &HID_GUID)?;
                return Ok(0); // VOID
            }
            "HidD_FreePreparsedData" if a[0] == 0 => return Ok(1),
            "HidD_FreePreparsedData" => 87, // No descriptor was issued by this profile.
            "HidD_GetAttributes"
            | "HidD_GetPreparsedData"
            | "HidD_GetManufacturerString"
            | "HidD_GetProductString"
            | "HidD_GetSerialNumberString" => 6,
            "HidP_MaxDataListLength" => return Ok(0),
            name if name.starts_with("HidP_") => return Ok(0xc0110001), // INVALID_PREPARSED_DATA, no LastError change.
            "SetupDiGetClassDevsA" | "SetupDiGetClassDevsW" => {
                if a[3] & !0x1f != 0 || (a[3] & 4 == 0 && a[0] == 0) {
                    87
                } else if a[3] & 9 != 0 {
                    50 // DEFAULT/PROFILE have no backend.
                } else {
                    if a[3] & 4 == 0 {
                        let mut guid = [0; 16];
                        mem.read(a[0], &mut guid)?;
                    }
                    if a[1] != 0 {
                        Self::string_bytes(
                            mem,
                            a[1],
                            u32::MAX as u64,
                            if name.ends_with('W') { 2 } else { 1 },
                        )?;
                    }
                    // Handles never alias guest allocators or exceed the reserved opaque range.
                    if self.devices.sets.len() >= 4096 || self.devices.next >= TEB + 0x690000 {
                        8
                    } else {
                        let handle = self.devices.next;
                        self.devices.sets.insert(handle);
                        self.devices.next += 16;
                        return Ok(handle);
                    }
                }
            }
            "SetupDiDestroyDeviceInfoList" => {
                if !self.devices.sets.contains(&a[0]) {
                    6
                } else {
                    self.set_error(mem, 0)?;
                    self.devices.sets.remove(&a[0]);
                    return Ok(1);
                }
            }
            "SetupDiEnumDeviceInfo" => {
                if !self.devices.sets.contains(&a[0]) {
                    6
                } else if a[2] == 0 {
                    87
                } else if mem.read_int(a[2], 4)? != 32 {
                    1784
                } else {
                    259
                } // ERROR_NO_MORE_ITEMS; no output record is produced.
            }
            "SetupDiEnumDeviceInterfaces" => {
                if a[4] == 0 || mem.read_int(a[4], 4)? != 32 {
                    87
                } else {
                    mem.check(a[4], 32, WRITE)?;
                    // Check all inputs before the documented output clear.
                    let error = if !self.devices.sets.contains(&a[0]) {
                        6
                    } else if a[1] != 0 {
                        let mut data = [0; 32];
                        mem.read(a[1], &mut data)?;
                        87 // No device record belongs to an empty set.
                    } else {
                        259
                    };
                    let mut output = [0; 32];
                    output[..4].copy_from_slice(&32u32.to_le_bytes());
                    self.set_error(mem, error)?;
                    mem.write(a[4], &output)?;
                    return Ok(0);
                }
            }
            "SetupDiGetDeviceInterfaceDetailA" | "SetupDiGetDeviceInterfaceDetailW" => {
                if !self.devices.sets.contains(&a[0]) {
                    6
                } else {
                    if a[1] != 0 && mem.read_int(a[1], 4)? == 32 {
                        let mut data = [0; 32];
                        mem.read(a[1], &mut data)?;
                    }
                    87 // No owned interface; preserve required-size and detail buffers.
                }
            }
            _ => unreachable!(),
        };
        self.set_error(mem, error)?;
        Ok(failure)
    }
}
