//! Checked device discovery for the isolated profile, which has no PnP/HID
//! backend or owned HID report descriptors. Empty sets are real owned objects;
//! no physical device, interface path or report capability is fabricated.
//! Public ABI and errors reviewed against Hangover Wine devinst.c / hidd.c /
//! ddk/hidpi.h at 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
use super::{Memory, TEB, WRITE};
use crate::win32::{catalog::DeviceApi, x64::text::string_bytes};
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
impl Devices {
    pub(super) fn remove_notifications(&mut self, window: u64) {
        self.notifications.retain(|_, (owner, _)| *owner != window);
    }
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: DeviceApi,
        a: &[u64],
        last_error: u64,
        window_exists: bool,
    ) -> Result<u64, String> {
        use DeviceApi::*;
        let failure = if matches!(id, ClassA | ClassW) {
            u64::MAX
        } else {
            0
        };
        let error = match id {
            RegisterA | RegisterW => {
                if a[2] & !5 != 0 {
                    87
                } else if a[2] & 1 != 0 {
                    50
                }
                // No service-control-manager profile.
                else if !window_exists {
                    87
                } else {
                    let mut class = None;
                    if a[1] != 0 {
                        let size = mem.read_int(a[1], 4)?;
                        let kind =
                            mem.read_int(a[1].checked_add(4).ok_or("device filter overflow")?, 4)?;
                        if size < 12 || (kind == 5 && a[2] & 4 == 0 && size < 28) {
                            mem.write_int(last_error, 4, 87)?;
                            return Ok(0);
                        }
                        if kind != 5 {
                            mem.write_int(last_error, 4, if kind == 6 { 50 } else { 13 })?;
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
                    if self.notifications.len() >= 4096 || self.next_notification >= TEB + 0x6b0000
                    {
                        8
                    } else {
                        let handle = self.next_notification;
                        self.notifications.insert(handle, (a[0], class));
                        self.next_notification += 16;
                        return Ok(handle);
                    }
                }
            }
            Unregister => {
                if self.notifications.contains_key(&a[0]) {
                    self.notifications.remove(&a[0]);
                    return Ok(1);
                }
                6
            }
            HidGuid => {
                mem.write(a[0], &HID_GUID)?;
                return Ok(0); // VOID
            }
            FreePreparsed if a[0] == 0 => return Ok(1),
            FreePreparsed => 87, // No descriptor was issued by this profile.
            Attributes | Preparsed | Manufacturer | Product | Serial => 6,
            MaxData => return Ok(0),
            Caps | ButtonCaps | ValueCaps | Data | SetUsage | SetUsages => return Ok(0xc0110001), // INVALID_PREPARSED_DATA, no LastError change.
            ClassA | ClassW => {
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
                        string_bytes(mem, a[1], u32::MAX as u64, if id == ClassW { 2 } else { 1 })?;
                    }
                    // Handles never alias guest allocators or exceed the reserved opaque range.
                    if self.sets.len() >= 4096 || self.next >= TEB + 0x690000 {
                        8
                    } else {
                        let handle = self.next;
                        self.sets.insert(handle);
                        self.next += 16;
                        return Ok(handle);
                    }
                }
            }
            DestroySet => {
                if !self.sets.contains(&a[0]) {
                    6
                } else {
                    mem.write_int(last_error, 4, 0)?;
                    self.sets.remove(&a[0]);
                    return Ok(1);
                }
            }
            EnumDevice => {
                if !self.sets.contains(&a[0]) {
                    6
                } else if a[2] == 0 {
                    87
                } else if mem.read_int(a[2], 4)? != 32 {
                    1784
                } else {
                    259
                } // ERROR_NO_MORE_ITEMS; no output record is produced.
            }
            EnumInterfaces => {
                if a[4] == 0 || mem.read_int(a[4], 4)? != 32 {
                    87
                } else {
                    mem.check(a[4], 32, WRITE)?;
                    // Check all inputs before the documented output clear.
                    let error = if !self.sets.contains(&a[0]) {
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
                    mem.write_int(last_error, 4, error)?;
                    mem.write(a[4], &output)?;
                    return Ok(0);
                }
            }
            DetailA | DetailW => {
                if !self.sets.contains(&a[0]) {
                    6
                } else {
                    if a[1] != 0 && mem.read_int(a[1], 4)? == 32 {
                        let mut data = [0; 32];
                        mem.read(a[1], &mut data)?;
                    }
                    87 // No owned interface; preserve required-size and detail buffers.
                }
            }
        };
        mem.write_int(last_error, 4, error)?;
        Ok(failure)
    }
}
