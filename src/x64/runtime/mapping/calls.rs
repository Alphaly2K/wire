//! Section adapters borrow only the mapping owner, memory and required services.
use super::Mappings;
use crate::{
    win32::catalog::FileApi,
    x64::runtime::{files::Files, threads::Threads, Memory, READ},
};

impl Mappings {
    pub(in crate::x64::runtime) fn create_call(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        files: &Files,
        threads: &Threads,
    ) -> Result<u64, String> {
        let inherit = if args[1] == 0 {
            false
        } else {
            mem.check(args[1], 24, READ)?;
            if mem.read_int(args[1], 4)? != 24 || mem.read_int(args[1] + 8, 8)? != 0 {
                return Err("Win64 mapping security descriptor not implemented".into());
            }
            mem.read_int(args[1] + 16, 4)? != 0
        };
        let result = if args[5] != 0 || args[2] != 2 || args[0] == u64::MAX {
            Err(50)
        } else {
            files
                .mapping_file(args[0])
                .and_then(|file| self.create(file, args[3] << 32 | args[4], inherit))
        };
        Ok(match result {
            Ok(handle) => handle,
            Err(error) => {
                threads.set_error(mem, error)?;
                0
            }
        })
    }

    pub(in crate::x64::runtime) fn map_call(
        &mut self,
        mem: &mut Memory,
        id: FileApi,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(
            match self.map(
                mem,
                args[0],
                args[1] as u32,
                args[2] << 32 | args[3],
                args[4],
                if id == FileApi::MapViewOfFileEx {
                    args[5]
                } else {
                    0
                },
            ) {
                Ok(base) => base,
                Err(error) => {
                    threads.set_error(mem, error)?;
                    0
                }
            },
        )
    }

    pub(in crate::x64::runtime) fn unmap_call(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(if self.contains_view(args[0]) {
            self.unmap(mem, args[0])?;
            1
        } else {
            threads.set_error(mem, 487)?;
            0
        })
    }

    pub(in crate::x64::runtime) fn flush_call(
        &self,
        mem: &mut Memory,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        Ok(match self.flush(args[0], args[1]) {
            Ok(()) => 1,
            Err(error) => {
                threads.set_error(mem, error)?;
                0
            }
        })
    }
}
