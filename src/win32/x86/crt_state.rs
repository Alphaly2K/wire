//! PE32 CRT pointer identities; CPU/FPU/callback control remains with the process.
use super::scratch::Scratch32;
use crate::mem::GuestMem;
use std::collections::HashMap;

pub(crate) struct CrtState32 {
    errno: HashMap<u32, u32>,
    argv: u32,
    envp: u32,
    app_type: u32,
}
impl Default for CrtState32 {
    fn default() -> Self {
        Self {
            errno: HashMap::new(),
            argv: 0,
            envp: 0,
            app_type: 1,
        }
    }
}
impl CrtState32 {
    pub(crate) fn app_type(&self) -> u32 {
        self.app_type
    }
    pub(crate) fn set_app_type(&mut self, value: u32) {
        self.app_type = value;
    }
    pub(crate) fn errno_address(
        &mut self,
        mem: &mut GuestMem,
        tid: u32,
        scratch: &mut Scratch32,
        last_error: &mut u32,
    ) -> Result<u32, String> {
        if let Some(&p) = self.errno.get(&tid) {
            return Ok(p);
        }
        let p = scratch.alloc(mem, 4, 4, last_error);
        if p == 0 {
            return Err("CRT errno allocation failed".into());
        }
        mem.write_u32(p, 0).map_err(|e| e.to_string())?;
        self.errno.insert(tid, p);
        Ok(p)
    }
    pub(crate) fn build_argv(
        &mut self,
        mem: &mut GuestMem,
        command: u32,
        scratch: &mut Scratch32,
        last_error: &mut u32,
    ) -> (u32, u32, u32) {
        if self.argv == 0 {
            let argv = scratch.alloc(mem, 16, 16, last_error);
            mem.write_u32(argv, command).ok();
            mem.write_u32(argv + 4, 0).ok();
            self.argv = argv;
            self.envp = scratch.alloc(mem, 16, 16, last_error); // zero = empty
        }
        (1, self.argv, self.envp)
    }
}
