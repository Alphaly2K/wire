//! Optional backend queries share existing identities and allocate no graphics objects.
use super::{call::Call, catalog::GraphicsProbeApi as G, user, ServiceError};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

fn zero_outputs(memory: &mut Memory, outputs: &[(u64, usize)]) -> Result<(), ServiceError> {
    for &(pointer, width) in outputs {
        if pointer != 0 {
            memory.check(pointer, width, WRITE)?;
        }
    }
    for &(pointer, width) in outputs {
        if pointer != 0 {
            memory.write_int(pointer, width, 0)?;
        }
    }
    Ok(())
}
pub(super) fn dispatch(
    id: G,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    match id {
        G::CurrentContext | G::CurrentDc => Ok(0),
        // With a null context the DC parameter is ignored, including an invalid DC.
        G::MakeCurrent if a[1] == 0 => Ok(1),
        G::MakeCurrent | G::DeleteContext | G::CopyContext | G::ShareLists | G::Extension => {
            user::error(memory, state, 6)
        }
        G::CreateContext
        | G::CreateLayerContext
        | G::ChooseFormat
        | G::SetFormat
        | G::DescribeFormat
        | G::SwapBuffers
        | G::SwapLayers => {
            if image.is_none_or(|image| image.user.gdi.check_dc(a[0]).is_err()) {
                user::error(memory, state, 6)
            } else {
                user::error(memory, state, 50)
            }
        }
        G::DxgiDebug | G::DxgiDebug1 => {
            let first = usize::from(id == G::DxgiDebug1);
            let iid = a[first];
            let out = a[first + 1];
            if iid == 0 || out == 0 {
                return Ok(0x80070057);
            }
            memory.check(out, width, WRITE)?;
            memory.read(iid, &mut [0; 16])?;
            zero_outputs(memory, &[(out, width)])?;
            Ok(0x80004002)
        }
        G::D3d11On12 => {
            if a[0] == 0 {
                return Ok(0x80070057);
            }
            zero_outputs(memory, &[(a[7], width), (a[8], width), (a[9], 4)])?;
            Ok(0x887a0004)
        }
        G::SetWindowAttribute => Ok(
            if image.is_some_and(|image| image.user.windows.contains_key(&a[0])) {
                0x80263001
            } else {
                0x80070006
            },
        ),
    }
}
