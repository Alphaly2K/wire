//! Typed local-slot execution; allocation markers belong to LocalSlots64, values to TEB memory.
use super::{threads::Threads, Memory, Runtime64, PAGE, WRITE};
use crate::win32::{catalog::LocalApi, kernel::locals64::LocalSlots64};

impl Runtime64 {
    pub(super) fn local_call(
        &mut self,
        id: LocalApi,
        args: &[u64],
        mem: &mut Memory,
    ) -> Result<u64, String> {
        self.locals.call(id, args, mem, &self.threads)
    }
}

impl LocalSlots64 {
    pub(super) fn call(
        &mut self,
        id: LocalApi,
        args: &[u64],
        mem: &mut Memory,
        threads: &Threads,
    ) -> Result<u64, String> {
        use LocalApi::*;
        Ok(match id {
            TlsAlloc => {
                if let Some(index) = self.vacant_tls() {
                    for teb in threads.live_tebs() {
                        mem.check(teb + 0x1480 + index as u64 * 8, 8, WRITE)?;
                    }
                    for teb in threads.live_tebs() {
                        mem.write_int(teb + 0x1480 + index as u64 * 8, 8, 0)?;
                    }
                    self.commit_tls(index, Some(0));
                    index as u64
                } else {
                    threads.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            FlsAlloc => {
                if let Some(index) = self.vacant_fls() {
                    for teb in threads.live_tebs() {
                        mem.check(teb + PAGE + index as u64 * 8, 8, WRITE)?;
                    }
                    for teb in threads.live_tebs() {
                        mem.write_int(teb + PAGE + index as u64 * 8, 8, 0)?;
                    }
                    self.commit_fls(index, Some((0, args[0])));
                    index as u64
                } else {
                    threads.set_error(mem, 8)?;
                    u32::MAX as u64
                }
            }
            FlsGet | FlsSet | FlsFree => {
                let index = args[0] as u32 as usize;
                if self.fls(index).is_none() {
                    threads.set_error(mem, 87)?;
                    0
                } else {
                    let value = mem.read_int(threads.teb() + PAGE + index as u64 * 8, 8)?;
                    let (_, callback) = self.fls(index).unwrap();
                    match id {
                        FlsGet => {
                            threads.set_error(mem, 0)?;
                            value
                        }
                        FlsSet => {
                            mem.write_int(threads.teb() + PAGE + index as u64 * 8, 8, args[1])?;
                            1
                        }
                        _ => {
                            if callback != 0 {
                                for teb in threads.live_tebs() {
                                    if mem.read_int(teb + PAGE + index as u64 * 8, 8)? != 0 {
                                        return Err(
                                            "Win64 FlsFree callbacks not implemented".into()
                                        );
                                    }
                                }
                            }
                            self.commit_fls(index, None);
                            1
                        }
                    }
                }
            }
            TlsGet | TlsSet | TlsFree => {
                let index = args[0] as u32 as usize;
                if self.tls(index).is_none() {
                    threads.set_error(mem, 87)?;
                    0
                } else if id == TlsGet {
                    let value = mem.read_int(threads.teb() + 0x1480 + index as u64 * 8, 8)?;
                    threads.set_error(mem, 0)?;
                    value
                } else {
                    let value = if id == TlsFree { 0 } else { args[1] };
                    mem.write_int(threads.teb() + 0x1480 + index as u64 * 8, 8, value)?;
                    self.commit_tls(index, if id == TlsFree { None } else { Some(value) });
                    1
                }
            }
        })
    }
}
