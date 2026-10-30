//! MCI and wave results when no audio device backend is available.
use crate::win32::catalog::AudioApi;

pub(crate) struct Outcome {
    pub value: u32,
    pub note: &'static str,
    pub implemented: bool,
}
pub(crate) fn call<E>(
    id: AudioApi,
    arguments: &[u64],
    pointer_width: usize,
    mut write: impl FnMut(u64, &[u8]) -> Result<(), E>,
) -> Result<Outcome, E> {
    use AudioApi::*;
    let a = arguments;
    Ok(match id {
        MciCommandA | MciCommandW | MciStringA | MciStringW => Outcome {
            value: 274,
            note: "MCI device backend unavailable",
            implemented: false,
        },
        WaveDevices | WaveInDevices => Outcome {
            value: 0,
            note: "no waveform audio devices",
            implemented: true,
        },
        WaveOpen | WaveInOpen => {
            if a[5] as u32 & 1 == 0 && a[0] != 0 {
                write(a[0], &[0; 8][..pointer_width])?;
            }
            Outcome {
                value: 6,
                note: "waveform audio backend unavailable",
                implemented: false,
            }
        }
        WaveOutCapsA | WaveOutCapsW | WaveInCapsA | WaveInCapsW => Outcome {
            value: if a[2] as u32 == 0 && matches!(id, WaveOutCapsA | WaveOutCapsW) {
                0
            } else {
                2
            },
            note: "no waveform audio device capabilities",
            implemented: true,
        },
        WaveOutErrorA | WaveOutErrorW | WaveInErrorA | WaveInErrorW => {
            let capacity = a[2] as u32 as usize;
            let value = if capacity == 0 {
                0
            } else if let Some(text) = error_text(a[0] as u32) {
                let text = text.bytes().take(capacity - 1).chain(Some(0));
                let bytes: Vec<_> = if matches!(id, WaveOutErrorW | WaveInErrorW) {
                    text.flat_map(|byte| u16::from(byte).to_le_bytes())
                        .collect()
                } else {
                    text.collect()
                };
                write(a[1], &bytes)?;
                0
            } else {
                9
            };
            Outcome {
                value,
                note: "portable English waveform error description",
                implemented: true,
            }
        }
        WaveOutClose | WaveInClose | WaveOutPrepare | WaveOutUnprepare | WaveOutWrite
        | WaveInPrepare | WaveInUnprepare | WaveInAdd | WaveOutPause | WaveOutRestart
        | WaveOutReset | WaveOutBreak | WaveInStart | WaveInStop | WaveInReset
        | WaveOutPosition | WaveInPosition | WaveOutGetId | WaveInGetId | WaveOutGetVolume
        | WaveOutSetVolume | WaveOutGetPitch | WaveOutSetPitch | WaveOutGetRate
        | WaveOutSetRate | WaveOutMessage | WaveInMessage => Outcome {
            value: 5,
            note: "no open waveform audio handle",
            implemented: true,
        },
    })
}

fn error_text(code: u32) -> Option<&'static str> {
    Some(match code {
        0 => "The operation completed successfully.",
        1 => "An unspecified multimedia error occurred.",
        2 => "The device identifier is out of range.",
        3 => "The driver is not enabled.",
        4 => "The requested resource is already allocated.",
        5 => "The device handle is invalid.",
        6 => "No device driver is present.",
        7 => "There is not enough memory for this operation.",
        8 => "The requested function is not supported.",
        9 => "The error number is out of range.",
        10 => "An invalid flag was specified.",
        11 => "An invalid parameter was specified.",
        12 => "The handle is busy.",
        13 => "The specified device alias is invalid.",
        14 => "The multimedia database is invalid.",
        15 => "The requested database key was not found.",
        16 => "The multimedia database could not be read.",
        17 => "The multimedia database could not be written.",
        18 => "The multimedia database entry could not be deleted.",
        19 => "The requested database value was not found.",
        20 => "The driver did not call the required callback.",
        21 => "More data is available.",
        32 => "The waveform audio format is not supported.",
        33 => "The waveform audio buffer is still playing.",
        34 => "The waveform audio buffer has not been prepared.",
        35 => "A synchronous waveform audio device requires WAVE_ALLOWSYNC.",
        _ => return None,
    })
}
