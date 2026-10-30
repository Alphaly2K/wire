//! Logical desktop settings reuse the existing process-owned USER state.
use super::{call::Call, catalog::DesktopApi as D, display::desktop, user, ServiceError};
use crate::{machine::State, memory::Memory, process::Loaded};

pub(super) fn dispatch(
    id: D,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    match id {
        D::ProcessStation | D::ObjectInformationW | D::ObjectInformationA | D::ThreadDesktop => {
            user::error(memory, state, 120)
        }
        D::CaretBlinkTime => Ok(u64::from(desktop::CARET_BLINK_MS)),
        D::DoubleClickTime => Ok(u64::from(desktop::DOUBLE_CLICK_MS)),
        D::DialogBaseUnits => Ok(u64::from(desktop::DIALOG_BASE_UNITS)),
        D::CompositionEnabled => {
            if call.arguments[0] == 0 {
                return Ok(0x80070057);
            }
            memory.write_int(call.arguments[0], 4, 0)?;
            Ok(0)
        }
        D::WindowAttribute => {
            let a = call.arguments;
            let Some(window) = image.as_ref().and_then(|i| i.user.windows.get(&a[0])) else {
                return Ok(0x80070006);
            };
            if a[2] == 0 {
                return Ok(0x80070057);
            }
            match (a[1] as u32, a[3] as u32) {
                (9, 16) => memory.write(
                    a[2],
                    &window
                        .rect
                        .iter()
                        .flat_map(|x| x.to_le_bytes())
                        .collect::<Vec<_>>(),
                )?,
                (14, 4) => memory.write_int(a[2], 4, 0)?,
                _ => return Ok(0x80070057),
            }
            Ok(0)
        }
        D::ParametersA | D::ParametersW => {
            let settings = &mut image
                .ok_or(ServiceError::Unavailable(
                    "desktop settings require a loaded image",
                ))?
                .user
                .desktop;
            let a = call.arguments;
            match a[0] as u32 {
                0x46 => {
                    memory.write_int(a[2], 4, u64::from(settings.screen_reader()))?;
                    Ok(1)
                }
                0x47 if a[3] as u32 == 0 => {
                    settings.set_screen_reader(a[1] as u32);
                    Ok(1)
                }
                _ => user::error(memory, state, 50),
            }
        }
    }
}
