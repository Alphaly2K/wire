//! Process-owned ETW registrations for a profile with no enabled sessions.
use super::catalog::EtwApi;
use std::collections::BTreeMap;

pub(crate) trait Memory {
    type Error;
    fn check(&self, address: u64, length: usize, write: bool) -> Result<(), Self::Error>;
    fn read_guid(&self, address: u64) -> Result<[u8; 16], Self::Error>;
    fn write_handle(&mut self, address: u64, handle: u64) -> Result<(), Self::Error>;
}

pub(crate) struct Providers {
    registrations: BTreeMap<u64, ([u8; 16], u64, u64)>,
    next: u64,
}
impl Default for Providers {
    fn default() -> Self {
        Self {
            registrations: BTreeMap::new(),
            next: 0x7fff6000,
        }
    }
}
impl Providers {
    pub(crate) fn call<M: Memory>(
        &mut self,
        mem: &mut M,
        id: EtwApi,
        args: &[u64],
    ) -> Result<u64, M::Error> {
        if id == EtwApi::Register {
            mem.check(args[0], 16, false)?;
            mem.check(args[3], 8, true)?;
            let Some(handle) = self.vacant() else {
                return Ok(8);
            };
            let guid = mem.read_guid(args[0])?;
            mem.write_handle(args[3], handle)?;
            self.insert(handle, guid, args[1], args[2]);
            return Ok(0);
        }
        if !self.contains(args[0]) {
            return Ok(if matches!(id, EtwApi::Enabled | EtwApi::ProviderEnabled) {
                0
            } else {
                6
            });
        }
        Ok(match id {
            EtwApi::Unregister => {
                self.remove(args[0]);
                0
            }
            EtwApi::Enabled => {
                mem.check(args[1], 16, false)?;
                0
            }
            EtwApi::ProviderEnabled => 0,
            EtwApi::SetInformation => 50, // Provider traits/controller information are not implemented.
            EtwApi::WriteString => 0,
            EtwApi::Write | EtwApi::WriteTransfer => {
                mem.check(args[1], 16, false)?;
                let (count, data) = if id == EtwApi::Write {
                    (args[2], args[3])
                } else {
                    (args[4], args[5])
                };
                if count > 128 {
                    return Ok(87);
                }
                // Disabled events do not read caller payloads, matching the no-listener fast path.
                if count != 0 {
                    mem.check(data, count as usize * 16, false)?;
                }
                0
            }
            EtwApi::Register => unreachable!(),
        })
    }

    fn vacant(&self) -> Option<u64> {
        (self.registrations.len() < 64 && self.next < 0x7fff7000).then_some(self.next)
    }
    fn insert(&mut self, handle: u64, guid: [u8; 16], callback: u64, context: u64) {
        assert_eq!(self.vacant(), Some(handle));
        self.registrations.insert(handle, (guid, callback, context));
        self.next += 1;
    }
    fn contains(&self, handle: u64) -> bool {
        self.registrations.contains_key(&handle)
    }
    fn remove(&mut self, handle: u64) {
        self.registrations.remove(&handle);
    }
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.registrations.clear();
    }
}
