//! Typed local-slot execution; allocation markers belong to LocalSlots64, values to TEB memory.
use super::*;
use crate::win32::catalog::LocalApi;

impl Runtime64 {
    pub(super) fn local_call(
        &mut self,
        id: LocalApi,
        args: &[u64],
        mem: &mut Memory,
    ) -> Result<u64, String> {
        use LocalApi::*;
        Ok(match id {
            TlsAlloc => {
                if let Some(index) = self.locals.vacant_tls() {
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.check(t.teb + 0x1480 + index as u64 * 8, 8, WRITE)?;
                    }
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.write_int(t.teb + 0x1480 + index as u64 * 8, 8, 0)?;
                    }
                    self.locals.commit_tls(index, Some(0));
                    index as u64
                } else {
                    self.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            FlsAlloc => {
                if let Some(index) = self.locals.vacant_fls() {
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.check(t.teb + PAGE + index as u64 * 8, 8, WRITE)?;
                    }
                    for t in self.threads.entries.values().filter(|t| t.exit.is_none()) {
                        mem.write_int(t.teb + PAGE + index as u64 * 8, 8, 0)?;
                    }
                    self.locals.commit_fls(index, Some((0, args[0])));
                    index as u64
                } else {
                    self.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            FlsGet | FlsSet | FlsFree => {
                let index = args[0] as u32 as usize;
                if self.locals.fls(index).is_none() {
                    self.set_error(mem, 87)?;
                    0
                } else {
                    let value = mem.read_int(self.teb() + PAGE + index as u64 * 8, 8)?;
                    let (_, callback) = self.locals.fls(index).unwrap();
                    match id {
                        FlsGet => {
                            self.set_error(mem, 0)?;
                            value
                        }
                        FlsSet => {
                            mem.write_int(self.teb() + PAGE + index as u64 * 8, 8, args[1])?;
                            1
                        }
                        _ => {
                            if callback != 0 {
                                for t in self.threads.entries.values().filter(|t| t.exit.is_none())
                                {
                                    if mem.read_int(t.teb + PAGE + index as u64 * 8, 8)? != 0 {
                                        return Err(
                                            "Win64 FlsFree callbacks not implemented".into()
                                        );
                                    }
                                }
                            }
                            self.locals.commit_fls(index, None);
                            1
                        }
                    }
                }
            }
            TlsGet | TlsSet | TlsFree => {
                let index = args[0] as u32 as usize;
                if self.locals.tls(index).is_none() {
                    self.set_error(mem, 87)?;
                    0
                } else if id == TlsGet {
                    let value = mem.read_int(self.teb() + 0x1480 + index as u64 * 8, 8)?;
                    self.set_error(mem, 0)?;
                    value
                } else {
                    let value = if id == TlsFree { 0 } else { args[1] };
                    mem.write_int(self.teb() + 0x1480 + index as u64 * 8, 8, value)?;
                    self.locals
                        .commit_tls(index, if id == TlsFree { None } else { Some(value) });
                    1
                }
            }
        })
    }
}
