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

    pub(crate) fn startup_call(
        &mut self,
        name: &str,
        args: &crate::api::Args,
        mem: &mut GuestMem,
        command: u32,
        scratch: &mut Scratch32,
        last_error: &mut u32,
    ) -> Result<Option<u32>, String> {
        let value = match name {
            "__acrt_iob_func" => scratch
                .alloc(mem, 0x100, 16, last_error)
                .wrapping_add(args[0].wrapping_mul(0x40)),
            "__set_app_type" => {
                self.set_app_type(args[0]);
                0
            }
            "__p___argv" | "__p___wargv" => {
                let p = scratch.alloc(mem, 4, 4, last_error);
                let (_, argv, _) = self.build_argv(mem, command, scratch, last_error);
                mem.write_u32(p, argv).map_err(|e| e.to_string())?;
                p
            }
            "__p__environ" | "__p__wenviron" | "__p___initenv" | "__p___winitenv" => {
                let p = scratch.alloc(mem, 4, 4, last_error);
                let (_, _, envp) = self.build_argv(mem, command, scratch, last_error);
                mem.write_u32(p, envp).map_err(|e| e.to_string())?;
                p
            }
            "__p__pgmptr" | "__p__wpgmptr" => {
                let p = scratch.alloc(mem, 4, 4, last_error);
                mem.write_u32(p, command).map_err(|e| e.to_string())?;
                p
            }
            "__getmainargs" => {
                let (argc, argv, envp) = self.build_argv(mem, command, scratch, last_error);
                mem.write_u32(args[0], argc).map_err(|e| e.to_string())?;
                mem.write_u32(args[1], argv).map_err(|e| e.to_string())?;
                mem.write_u32(args[2], envp).map_err(|e| e.to_string())?;
                0
            }
            "_initialize_narrow_environment"
            | "_initialize_wide_environment"
            | "_configure_narrow_argv"
            | "_configure_wide_argv"
            | "_initialize_onexit_table"
            | "_register_onexit_function"
            | "_crt_atexit"
            | "_crt_at_quick_exit"
            | "_set_invalid_parameter_handler"
            | "_set_app_type"
            | "_set_fmode"
            | "_set_new_mode"
            | "setvbuf"
            | "_crt_debugger_hook"
            | "_controlfp_s"
            | "_lock"
            | "_unlock"
            | "__setusermatherr"
            | "_register_thread_local_exe_atexit_callback" => 0,
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    pub(crate) fn strtoul(
        &mut self,
        args: &crate::api::Args,
        mem: &mut GuestMem,
        tid: u32,
        scratch: &mut Scratch32,
        last_error: &mut u32,
    ) -> Result<u32, String> {
        let mut bytes = Vec::new();
        let mut terminated = false;
        for i in 0..1 << 20 {
            let b = mem
                .read_u8(args[0].checked_add(i).ok_or("strtoul address overflow")?)
                .map_err(|e| e.to_string())?;
            if b == 0 {
                terminated = true;
                break;
            }
            bytes.push(b);
        }
        if !terminated {
            return Err("unterminated strtoul source".into());
        }
        let digit = |b: u8| match b {
            b'0'..=b'9' => Some((b - b'0') as u32),
            b'a'..=b'z' => Some((b - b'a' + 10) as u32),
            b'A'..=b'Z' => Some((b - b'A' + 10) as u32),
            _ => None,
        };
        let mut pos = 0;
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        let negative = bytes.get(pos) == Some(&b'-');
        if bytes.get(pos).is_some_and(|b| matches!(b, b'+' | b'-')) {
            pos += 1;
        }
        let mut base = args[2];
        let prefix = bytes.get(pos) == Some(&b'0')
            && bytes.get(pos + 1).is_some_and(|b| matches!(b, b'x' | b'X'))
            && bytes
                .get(pos + 2)
                .and_then(|&b| digit(b))
                .is_some_and(|n| n < 16);
        if (base == 0 || base == 16) && prefix {
            base = 16;
            pos += 2;
        }
        if base == 0 {
            base = if bytes.get(pos) == Some(&b'0') { 8 } else { 10 };
        }
        let start = pos;
        let mut n = 0u32;
        let mut overflow = false;
        if (2..=36).contains(&base) {
            while let Some(d) = bytes.get(pos).and_then(|&b| digit(b)).filter(|&d| d < base) {
                if let Some(value) = n.checked_mul(base).and_then(|v| v.checked_add(d)) {
                    n = value;
                } else {
                    overflow = true;
                }
                pos += 1;
            }
        }
        if args[1] != 0 {
            mem.write_u32(
                args[1],
                args[0]
                    .checked_add(if pos == start { 0 } else { pos as u32 })
                    .ok_or("strtoul end pointer overflow")?,
            )
            .map_err(|e| e.to_string())?;
        }
        if overflow || !(2..=36).contains(&base) {
            let p = self.errno_address(mem, tid, scratch, last_error)?;
            mem.write_u32(p, if overflow { 34 } else { 22 })
                .map_err(|e| e.to_string())?;
        }
        Ok(if overflow {
            u32::MAX
        } else if negative {
            n.wrapping_neg()
        } else {
            n
        })
    }
}
