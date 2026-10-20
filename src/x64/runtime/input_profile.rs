//! The AppKit bridge currently delivers legacy mouse/keyboard messages only.
//! No raw device or HRAWINPUT handle is advertised without a raw input backend.
use super::{threads::Threads, user::User, Memory, Runtime64};
use crate::win32::{
    catalog::{ApiId, RawInputApi},
    user::input_profile::{ime, InputResult},
};
use crate::x64::memory::WRITE;
impl Runtime64 {
    pub(super) fn input_profile_call(
        &mut self,
        mem: &mut Memory,
        id: ApiId,
        a: &[u64],
    ) -> Result<u64, String> {
        self.user.input_profile_call(mem, id, a, &self.threads)
    }
}

impl User {
    fn input_profile_call(
        &self,
        mem: &mut Memory,
        id: ApiId,
        a: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        let result = match id {
            ApiId::RawInput(id) => raw_input(mem, id, a)?,
            ApiId::Ime(id) => ime(id, a, self.windows.contains_key(&a[0])),
            _ => unreachable!("input-profile adapter only accepts raw input and IME"),
        };
        if let Some(error) = result.error {
            threads.set_error(mem, error)?;
        }
        Ok(result.value)
    }
}

fn raw_input(mem: &mut Memory, id: RawInputApi, a: &[u64]) -> Result<InputResult, String> {
    use RawInputApi::*;
    let (error, failure) = match id {
        Register => {
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
                    return Ok(InputResult::value(1));
                }
                (50, 0) // Cannot register delivery that has no backend.
            }
        }
        DeviceList | Registered | Buffer => {
            let size = if id == Buffer { 24 } else { 16 };
            if a[1] == 0 || a[2] != size {
                (87, u32::MAX as u64)
            } else {
                mem.check(a[1], 4, WRITE)?;
                mem.write_int(a[1], 4, 0)?;
                return Ok(InputResult::value(0));
            }
        }
        Data => (if a[4] != 24 { 87 } else { 6 }, u32::MAX as u64),
        DeviceInfoA | DeviceInfoW => (6, u32::MAX as u64),
    };
    Ok(InputResult::error(failure, error))
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
            r.input_profile_call(
                &mut m,
                ApiId::RawInput(RawInputApi::Register),
                &[DATA, 1, 16]
            )
            .unwrap(),
            0
        );
        m.write_int(DATA + 4, 4, 1).unwrap();
        m.write_int(DATA + 8, 8, 0).unwrap();
        assert_eq!(
            r.input_profile_call(
                &mut m,
                ApiId::RawInput(RawInputApi::Register),
                &[DATA, 1, 16]
            )
            .unwrap(),
            1
        );
        for id in [RawInputApi::DeviceList, RawInputApi::Registered] {
            m.write_int(DATA + 128, 4, 0x1234).unwrap();
            assert_eq!(
                r.input_profile_call(&mut m, ApiId::RawInput(id), &[0, DATA + 128, 16])
                    .unwrap(),
                0
            );
            assert_eq!(m.read_int(DATA + 128, 4).unwrap(), 0);
            let bad = DATA + 4094;
            m.write_int(bad, 4, 0x55555555).unwrap();
            m.protect(DATA + 4096, 4096, READ).unwrap();
            assert!(r
                .input_profile_call(&mut m, ApiId::RawInput(id), &[0, bad, 16])
                .is_err());
            assert_eq!(m.read_int(bad, 4).unwrap(), 0x55555555);
            m.protect(DATA + 4096, 4096, READ | WRITE).unwrap();
        }
        assert_eq!(
            r.input_profile_call(
                &mut m,
                ApiId::RawInput(RawInputApi::Data),
                &[0x1234, 0x10000003, 0, DATA + 128, 24]
            )
            .unwrap(),
            u32::MAX as u64
        );
    }
}
