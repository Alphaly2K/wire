use super::{call::Call, catalog::SnapshotApi, context, ServiceError};
use crate::{machine::State, memory::Memory, process::Loaded};

pub(super) fn dispatch(
    id: SnapshotApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let result = if id == SnapshotApi::Create {
        let name = &image.modules.modules[&image.info[0]].path;
        image.snapshots.create(a[0], name, image.threads.snapshot())
    } else if !image.snapshots.contains(a[0]) {
        Err(6)
    } else if memory.read_int(a[1], 4)?
        < super::toolhelp::Snapshots::size(id, memory.mode() as usize / 8) as u64
    {
        Err(24)
    } else {
        match image.snapshots.entry(a[0], id, memory.mode() as usize / 8) {
            Ok(data) => {
                memory.write(a[1], &data)?;
                image.snapshots.advance(a[0], id);
                Ok(1)
            }
            Err(code) => Err(code),
        }
    };
    match result {
        Ok(value) => Ok(value),
        Err(code) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(if id == SnapshotApi::Create {
                u64::MAX
            } else {
                0
            })
        }
    }
}
