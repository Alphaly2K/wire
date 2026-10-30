//! Display queries describe one logical monitor without accessing the host desktop.
use super::{
    call::Call,
    catalog::DisplayApi as D,
    modules::{name, Failure},
    user, ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::Memory,
    process::Loaded,
};
#[allow(dead_code)]
#[path = "../../../../src/win32/user/desktop.rs"]
pub(super) mod desktop;
#[allow(dead_code)]
#[path = "../../../../src/win32/user/display.rs"]
pub(crate) mod profile;

pub(crate) fn monitor(mode: Mode) -> u64 {
    if mode == Mode::X86 {
        0x45000001
    } else {
        0x7ffd03000001
    }
}
pub(crate) fn bounds() -> [i32; 4] {
    [0, 0, profile::WIDTH as i32, profile::HEIGHT as i32]
}

pub(super) fn dispatch(
    id: D,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    let a = call.arguments;
    let result = invoke(id, &a, memory, image);
    match result {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => user::error(memory, state, code),
    }
}
fn invoke(
    id: D,
    a: &[u64; 16],
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, Failure> {
    if id.devices() || id.settings() {
        let device = if a[0] == 0 {
            String::new()
        } else {
            name(memory, a[0], id.wide())?
        };
        if id.devices() {
            let size = if id.wide() { 840 } else { 424 };
            if memory.read_int(a[2], 4)? != size || a[3] as u32 & !1 != 0 {
                return Err(Failure::Windows(87));
            }
            if a[1] as u32 != 0
                || (!device.is_empty() && !device.eq_ignore_ascii_case(profile::DEVICE))
            {
                return Ok(0);
            }
            let adapter = device.is_empty();
            // Interface-name requests require a device-interface provider.
            if a[3] as u32 != 0 {
                return Err(Failure::Windows(50));
            }
            memory.write(
                a[2],
                &profile::device(
                    id.wide(),
                    adapter,
                    if adapter {
                        "Wire virtual display"
                    } else {
                        "Wire virtual monitor"
                    },
                ),
            )?;
            return Ok(1);
        }
        if !device.is_empty() && !device.eq_ignore_ascii_case(profile::DEVICE)
            || !matches!(a[1] as u32, 0 | 0xffffffff | 0xfffffffe)
        {
            return Ok(0);
        }
        if id.extended() && a[3] as u32 & !6 != 0 {
            return Err(Failure::Windows(87));
        }
        let offset = if id.wide() { 68 } else { 36 };
        let pointer = a[2].checked_add(offset).ok_or(Failure::Windows(87))?;
        let size = memory.read_int(pointer, 2)? as usize;
        if !(if id.wide() { 188..=220 } else { 124..=156 }).contains(&size) {
            return Err(Failure::Windows(87));
        }
        memory.write(a[2], &profile::mode(id.wide(), size))?;
        return Ok(1);
    }
    let handle = monitor(memory.mode());
    Ok(match id {
        D::Metrics => u64::from(profile::metric(a[0] as u32)),
        D::InfoA | D::InfoW => {
            if a[0] != handle {
                return Err(Failure::Windows(1461));
            }
            let size = memory.read_int(a[1], 4)? as usize;
            if size != 40 && size != if id.wide() { 104 } else { 72 } {
                return Err(Failure::Windows(87));
            }
            memory.write(a[1], &profile::monitor(id.wide(), size))?;
            1
        }
        D::FromPoint | D::FromRect | D::FromWindow => {
            let flags = a[1] as u32;
            if flags > 2 {
                return Err(Failure::Windows(87));
            }
            let inside = match id {
                D::FromPoint => {
                    user::geometry::point(bounds(), a[0] as u32 as i32, (a[0] >> 32) as u32 as i32)
                }
                D::FromRect => user::geometry::intersection(bounds(), user::rect(memory, a[0])?).1,
                _ if a[0] == 0 => false,
                _ => {
                    let rect = image
                        .ok_or(Failure::Windows(1400))?
                        .user
                        .window_rect(a[0])?;
                    user::geometry::intersection(bounds(), rect).1
                }
            };
            if inside || flags != 0 {
                handle
            } else {
                0
            }
        }
        D::ConfigSizes | D::ConfigQuery | D::ConfigDeviceInfo => 50,
        D::Monitors => unreachable!("monitor enumeration uses a process callback"),
        _ => unreachable!("device/mode queries returned above"),
    })
}
