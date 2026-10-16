//! PE32 multimedia timer calls retain their stateless period policy.
use crate::{api::Args, mem::GuestMem, win32::catalog::MultimediaApi};

pub(crate) fn call(id: MultimediaApi, args: &Args, mem: &GuestMem) -> (u32, String) {
    let value = match id {
        MultimediaApi::Caps => {
            if args[1] < 8 {
                97
            } else {
                let bytes = [1u32.to_le_bytes(), u32::MAX.to_le_bytes()].concat();
                if mem.write_bytes(args[0], &bytes).is_ok() {
                    0
                } else {
                    97
                }
            }
        }
        MultimediaApi::BeginPeriod | MultimediaApi::EndPeriod => {
            if args[0] == 0 {
                97
            } else {
                0
            }
        }
    };
    (value, "x86 guest hardware / monotonic clock profile".into())
}
