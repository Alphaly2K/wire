//! The AppKit bridge currently delivers legacy mouse/keyboard messages only.
//! No raw device or HRAWINPUT handle is advertised without a raw input backend.
use super::{Memory, Runtime64};
use crate::x64::memory::WRITE;
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "RegisterRawInputDevices"
        | "GetRegisteredRawInputDevices"
        | "GetRawInputDeviceList"
        | "GetRawInputBuffer" => 3,
        "GetRawInputDeviceInfoA" | "GetRawInputDeviceInfoW" => 4,
        "GetRawInputData" => 5,
        _ => return None,
    })
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "RegisterRawInputDevices" => &[1, 2],
        "GetRawInputData" => &[1, 4],
        "GetRawInputDeviceInfoA" | "GetRawInputDeviceInfoW" => &[1],
        _ => &[2],
    }
}
impl Runtime64 {
    pub(in crate::x64::runtime) fn raw_input_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        let (error, failure) = match name {
            "RegisterRawInputDevices" => {
                if a[2] != 16 || a[1] == 0 || a[1] > 1024 {
                    (87, 0)
                } else {
                    let mut data = vec![0; a[1] as usize * 16];
                    mem.read(a[0], &mut data)?;
                    let removal = data.as_chunks::<16>().0.iter().all(|d| {
                        u16::from_le_bytes(d[..2].try_into().unwrap()) != 0
                            && u32::from_le_bytes(d[4..8].try_into().unwrap()) == 1
                            && u64::from_le_bytes(d[8..16].try_into().unwrap()) == 0
                    });
                    if removal {
                        return Ok(1);
                    }
                    (50, 0) // Cannot register delivery that has no backend.
                }
            }
            "GetRawInputDeviceList" | "GetRegisteredRawInputDevices" | "GetRawInputBuffer" => {
                let size = if name == "GetRawInputBuffer" { 24 } else { 16 };
                if a[1] == 0 || a[2] != size {
                    (87, u32::MAX as u64)
                } else {
                    mem.check(a[1], 4, WRITE)?;
                    mem.write_int(a[1], 4, 0)?;
                    return Ok(0);
                }
            }
            "GetRawInputData" => (if a[4] != 24 { 87 } else { 6 }, u32::MAX as u64),
            "GetRawInputDeviceInfoA" | "GetRawInputDeviceInfoW" => (6, u32::MAX as u64),
            _ => unreachable!(),
        };
        self.set_error(mem, error)?;
        Ok(failure)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn raw_device_queries_and_registration_do_not_advertise_absent_delivery() {
        let mut m = Memory::default();
        const DATA: u64 = 0x700000000;
        m.map(DATA, 8192, READ | WRITE).unwrap();
        let mut r = Runtime64::new(&mut m, 0x140000000, DATA, 8192, "probe.exe").unwrap();
        m.write_int(DATA, 2, 1).unwrap();
        m.write_int(DATA + 2, 2, 2).unwrap();
        m.write_int(DATA + 8, 8, 0x7ffd00630000).unwrap();
        assert_eq!(
            r.raw_input_call(&mut m, "RegisterRawInputDevices", &[DATA, 1, 16])
                .unwrap(),
            0
        );
        m.write_int(DATA + 4, 4, 1).unwrap();
        m.write_int(DATA + 8, 8, 0).unwrap();
        assert_eq!(
            r.raw_input_call(&mut m, "RegisterRawInputDevices", &[DATA, 1, 16])
                .unwrap(),
            1
        );
        for name in ["GetRawInputDeviceList", "GetRegisteredRawInputDevices"] {
            m.write_int(DATA + 128, 4, 0x1234).unwrap();
            assert_eq!(
                r.raw_input_call(&mut m, name, &[0, DATA + 128, 16])
                    .unwrap(),
                0
            );
            assert_eq!(m.read_int(DATA + 128, 4).unwrap(), 0);
            let bad = DATA + 4094;
            m.write_int(bad, 4, 0x55555555).unwrap();
            m.protect(DATA + 4096, 4096, READ).unwrap();
            assert!(r.raw_input_call(&mut m, name, &[0, bad, 16]).is_err());
            assert_eq!(m.read_int(bad, 4).unwrap(), 0x55555555);
            m.protect(DATA + 4096, 4096, READ | WRITE).unwrap();
        }
        assert_eq!(
            r.raw_input_call(
                &mut m,
                "GetRawInputData",
                &[0x1234, 0x10000003, 0, DATA + 128, 24]
            )
            .unwrap(),
            u32::MAX as u64
        );
    }
}
