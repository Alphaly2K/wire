//! Explicit unavailable audio result; only waveOpen writes guest memory.
use crate::{mem::GuestMem, win32::catalog::AudioApi};

pub(crate) struct Outcome {
    pub value: u32,
    pub note: &'static str,
    pub implemented: bool,
}
pub(crate) fn call(id: AudioApi, args: &[u32], mem: &GuestMem) -> Result<Outcome, String> {
    use AudioApi::*;
    Ok(match id {
        MciCommandA | MciCommandW | MciStringA | MciStringW => Outcome {
            value: 274,
            note: "MCI device backend unavailable",
            implemented: false,
        },
        WaveDevices => Outcome {
            value: 0,
            note: "no audio output backend",
            implemented: true,
        },
        WaveOpen => {
            if args[0] != 0 {
                mem.write_u32(args[0], 0).map_err(|e| e.to_string())?;
            }
            Outcome {
                value: 6, // MMSYSERR_NODRIVER
                note: "wave output backend unavailable",
                implemented: false,
            }
        }
    })
}
