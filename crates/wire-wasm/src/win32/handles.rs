use super::{catalog::HandleApi as H, modules::Failure, standard_handles};
use crate::{
    machine::Mode,
    memory::{Fault, FaultKind, Memory, WRITE},
    process::Loaded,
};

pub(super) fn flags(image: &Loaded, handle: u64) -> Option<u32> {
    image
        .open_files
        .flags(handle)
        .or_else(|| image.mappings.flags(handle))
        .or_else(|| image.standard.flags(handle))
        .or_else(|| image.pipes.flags(handle))
        .or_else(|| image.threads.flags(handle))
        .or_else(|| image.process.flags(handle))
        .or_else(|| image.objects.flags(handle))
        .or_else(|| image.snapshots.flags(handle))
        .or_else(|| image.tokens.flags(handle))
}
fn close(image: &mut Loaded, handle: u64) -> Result<(), u32> {
    if image.open_files.flags(handle).is_some() {
        image.open_files.close(handle)
    } else if image.mappings.flags(handle).is_some() {
        image.mappings.close(handle)
    } else if image.standard.flags(handle).is_some() {
        image.standard.close(handle)
    } else if image.threads.flags(handle).is_some() {
        image.threads.close(handle)
    } else if image.process.flags(handle).is_some() {
        image.process.close(handle)
    } else if image.snapshots.flags(handle).is_some() {
        image.snapshots.close(handle)
    } else if image.tokens.flags(handle).is_some() {
        image.tokens.close(handle)
    } else if image.objects.flags(handle).is_some() {
        if image.objects.flags(handle).unwrap() & 2 != 0 {
            return Err(6);
        }
        assert!(image.objects.close(handle));
        Ok(())
    } else {
        image.pipes.close(handle)
    }
}
fn set_flags(image: &mut Loaded, handle: u64, mask: u32, value: u32) -> Result<(), u32> {
    if image.open_files.flags(handle).is_some() {
        image.open_files.set_flags(handle, mask, value)
    } else if image.mappings.flags(handle).is_some() {
        image.mappings.set_flags(handle, mask, value)
    } else if image.standard.flags(handle).is_some() {
        image
            .standard
            .set_flags(handle, u64::from(mask), u64::from(value))
            .unwrap()
    } else if image.threads.flags(handle).is_some() {
        image.threads.set_flags(handle, mask, value)
    } else if image.process.flags(handle).is_some() {
        image.process.set_flags(handle, mask, value)
    } else if image.snapshots.flags(handle).is_some() {
        image
            .snapshots
            .set_flags(handle, u64::from(mask), u64::from(value))
            .unwrap()
    } else if image.tokens.flags(handle).is_some() {
        image
            .tokens
            .set_flags(handle, mask.into(), value.into())
            .unwrap()
    } else if image.objects.flags(handle).is_some() {
        image.objects.set_flags(handle, mask, value)
    } else {
        image.pipes.set_flags(handle, mask, value)
    }
}

pub(super) fn dispatch(
    id: H,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    match id {
        H::GetStdHandle => image
            .standard
            .redirect_request(id, a)
            .map_err(Failure::Windows),
        H::SetStdHandle => {
            let slot = standard_handles::slot(a[0] as u32).map_err(Failure::Windows)?;
            let ptr = memory.mode() as usize / 8;
            let (params_offset, std_offset) = if memory.mode() == Mode::X86 {
                (0x10, 0x18)
            } else {
                (0x20, 0x20)
            };
            let params = memory.read_int(image.info[5] + params_offset, ptr)?;
            let offset = std_offset + slot as u64 * ptr as u64;
            let address = if memory.mode() == Mode::X86 {
                memory.mode().address(params.wrapping_add(offset))
            } else {
                params.checked_add(offset).ok_or(Failure::Fault(Fault {
                    address: params,
                    length: ptr,
                    access: WRITE,
                    kind: FaultKind::Address,
                }))?
            };
            memory.write_int(address, ptr, a[1])?;
            image.standard.set_redirect(slot, a[1]);
            Ok(1)
        }
        H::SetHandleCount => Ok(a[0]),
        H::CloseHandle => {
            close(image, a[0]).map_err(Failure::Windows)?;
            Ok(1)
        }
        H::GetHandleInformation => {
            let flags = flags(image, a[0]).ok_or(Failure::Windows(6))?;
            memory.write_int(a[1], 4, u64::from(flags))?;
            Ok(1)
        }
        H::SetHandleInformation => {
            set_flags(image, a[0], a[1] as u32, a[2] as u32).map_err(Failure::Windows)?;
            Ok(1)
        }
        H::DuplicateHandle => {
            let width = memory.mode() as usize / 8;
            memory.check(a[3], width, WRITE)?;
            image
                .process
                .check_current(a[0], crate::process::current::DUP_HANDLE)
                .map_err(Failure::Windows)?;
            image
                .process
                .check_current(a[2], crate::process::current::DUP_HANDLE)
                .map_err(Failure::Windows)?;
            if a[6] & !3 != 0 {
                return Err(Failure::Windows(87));
            }
            let pseudo_process = image.process.pseudo(a[1]);
            let pseudo_thread = a[1] == memory.mode().address(u64::MAX - 1);
            let pseudo = pseudo_process || pseudo_thread;
            let source_flags = if pseudo {
                0
            } else {
                flags(image, a[1]).ok_or(Failure::Windows(6))?
            };
            if a[6] & 1 != 0 && source_flags & 2 != 0 {
                return Err(Failure::Windows(6));
            }
            let desired = (a[6] & 2 == 0).then_some(a[4] as u32);
            let alias = if image.open_files.flags(a[1]).is_some() {
                image.open_files.duplicate(a[1], desired, a[5] != 0)
            } else if image.mappings.flags(a[1]).is_some() {
                image.mappings.duplicate(a[1], desired, a[5] != 0)
            } else if image.standard.flags(a[1]).is_some() {
                image
                    .standard
                    .duplicate(a[1], desired, a[5] != 0, a[6] & 1 != 0)
                    .unwrap()
            } else if pseudo_process || image.process.flags(a[1]).is_some() {
                image.process.duplicate(a[1], desired, a[5] != 0)
            } else if pseudo_thread || image.threads.flags(a[1]).is_some() {
                image.threads.duplicate(a[1], desired, a[5] != 0)
            } else if image.objects.flags(a[1]).is_some() {
                image
                    .objects
                    .duplicate(a[1], desired, a[5] != 0, a[6] & 1 != 0)
            } else if image.snapshots.flags(a[1]).is_some() {
                Err(50)
            } else if image.tokens.flags(a[1]).is_some() {
                image.tokens.duplicate(a[1], desired, a[5] != 0)
            } else {
                image.pipes.duplicate(a[1], desired, a[5] != 0)
            }
            .map_err(Failure::Windows)?;
            memory.write_int(a[3], width, alias)?;
            if a[6] & 1 != 0 && !pseudo {
                close(image, a[1]).expect("validated source close");
            }
            Ok(1)
        }
    }
}
