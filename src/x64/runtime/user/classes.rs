//! Checked Win64 class records; ownership is independent of the runtime adapter.
use super::{Memory, User, READ};
use crate::win32::{
    catalog::ClassApi,
    user::classes64::{Class, Key},
};

impl User {
    pub(in crate::x64::runtime) fn class_call(
        &mut self,
        mem: &mut Memory,
        id: ClassApi,
        args: &[u64],
        last_error: u64,
    ) -> Result<u64, String> {
        if id.register() {
            let ex = id.extended();
            let pointer = args[0];
            mem.check(pointer, if ex { 80 } else { 72 }, READ)?;
            if ex && mem.read_int(pointer, 4)? != 80 {
                mem.write_int(last_error, 4, 87)?;
                return Ok(0);
            }
            let style = mem.read_int(pointer + if ex { 4 } else { 0 }, 4)? as u32;
            let procedure = mem.read_int(pointer + 8, 8)?;
            let class_extra = mem.read_int(pointer + 16, 4)?;
            let window_extra = mem.read_int(pointer + 20, 4)?;
            let instance = mem.read_int(pointer + 24, 8)?;
            let class =
                crate::win32::x64::text::path_name(mem, mem.read_int(pointer + 64, 8)?, id.wide())?;
            if class.is_empty()
                || class.len() > 256
                || class_extra > 4096
                || window_extra > 4096
                || procedure == 0
            {
                mem.write_int(last_error, 4, 87)?;
                return Ok(0);
            }
            if class_extra != 0 || mem.read_int(pointer + 56, 8)? != 0 {
                mem.write_int(last_error, 4, 50)?;
                return Ok(0);
            }
            return match self.classes.register(Class {
                instance,
                name: class,
                procedure,
                style,
                window_extra: window_extra as usize,
            }) {
                Ok(atom) => Ok(atom as u64),
                Err(error) => {
                    mem.write_int(last_error, 4, error as u64)?;
                    Ok(0)
                }
            };
        }
        assert!(id.unregister(), "Win64 class queries have no binding");
        let key = super::super::resources::resource_key(mem, args[0], id.wide())?;
        let key = match &key {
            super::super::resources::Key::Id(id) => Key::Id(*id),
            super::super::resources::Key::Name(name) => Key::Name(name),
        };
        let atom = self.classes.find(args[1], key, false).map(|(atom, _)| atom);
        if let Some(atom) = atom {
            if self.windows.values().any(|w| w.class == atom) {
                mem.write_int(last_error, 4, 1412)?;
                return Ok(0);
            }
            self.classes.remove(atom);
            Ok(1)
        } else {
            mem.write_int(last_error, 4, 1411)?;
            Ok(0)
        }
    }
}
