use super::{catalog::FileApi as F, files::security, modules::Failure};
use crate::{file_sections::ViewMemory, memory::Memory, process::Loaded};

impl ViewMemory for Memory {
    fn file_view_address(&self, address: u64, size: u64) -> Result<u64, u32> {
        Memory::file_view_address(self, address, size)
    }
    fn map_file_view(&mut self, address: u64, bytes: &[u8]) -> Result<u64, u32> {
        Memory::map_file_view(self, address, bytes)
    }
    fn unmap_file_view(&mut self, base: u64, span: u64) -> Result<(), String> {
        self.unmap(base, span)
    }
    fn update_file_view(&mut self, base: u64, offset: u64, bytes: &[u8]) -> Result<(), String> {
        Memory::update_file_view(self, base, offset, bytes)
    }
}

pub(super) fn dispatch(
    id: F,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    match id {
        F::CreateFileMappingA | F::CreateFileMappingW => {
            let inherit = security(memory, a[1])?;
            if a[5] != 0 || a[2] != 2 || a[0] == memory.mode().address(u64::MAX) {
                return Err(Failure::Windows(50));
            }
            let node = image
                .open_files
                .mapping_data(a[0])
                .map_err(Failure::Windows)?;
            let identity = image.open_files.identity(a[0]).map_err(Failure::Windows)?;
            image
                .mappings
                .create(node, identity, a[3] << 32 | a[4], inherit)
                .map_err(Failure::Windows)
        }
        F::MapViewOfFile | F::MapViewOfFileEx => image
            .mappings
            .map(memory, a, if id == F::MapViewOfFileEx { a[5] } else { 0 })
            .map_err(Failure::Windows),
        F::UnmapViewOfFile => {
            image
                .mappings
                .unmap(memory, a[0])
                .map_err(|_| Failure::Windows(487))?;
            Ok(1)
        }
        F::FlushViewOfFile => {
            image.mappings.flush(a[0], a[1]).map_err(Failure::Windows)?;
            Ok(1)
        }
        _ => unreachable!(),
    }
}
