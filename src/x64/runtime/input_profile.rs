//! Checked adapters for host raw input and the absent IME context profile.
use super::{threads::Threads, user::User, Memory, Runtime64};
use crate::win32::{
    catalog::{ApiId, RawInputApi},
    user::input_profile::{ime, InputResult},
};
mod raw;
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
        &mut self,
        mem: &mut Memory,
        id: ApiId,
        a: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        let result = match id {
            ApiId::RawInput(id) => self.raw_input_call(mem, id, a, threads.active)?,
            ApiId::Ime(id) => ime(id, a, self.windows.contains_key(&a[0])),
            _ => unreachable!("input-profile adapter only accepts raw input and IME"),
        };
        if let Some(error) = result.error {
            threads.set_error(mem, error)?;
        }
        Ok(result.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x64::memory::{READ, WRITE};
    #[test]
    fn raw_device_queries_and_registration_validate_targets_and_checked_counts() {
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
            assert_eq!(
                m.read_int(DATA + 128, 4).unwrap(),
                if id == RawInputApi::DeviceList { 2 } else { 0 }
            );
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
