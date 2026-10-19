//! Existing unavailable MCI and wave endpoints; DirectSound is a separate family.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioApi {
    MciCommandA,
    MciCommandW,
    MciStringA,
    MciStringW,
    WaveDevices,
    WaveOpen,
}
const fn declaration(
    id: AudioApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(ApiId::Audio(id), name, parameters, ReturnConvention::Uint32,
        "PE32 winmm only; MCI retains error 274 and unimplemented diagnostics with ignored inputs; wave reports no devices, open clears non-NULL output before error 6/unimplemented trace, preserves LastError and unsaved return aliases; no host audio backend; Win64 absent",
        [ArchitecturePlan { dlls: &["winmm.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}
use Parameter::{Pointer as P, Uint32 as D};
pub static AUDIO_CONTRACTS: [Contract; 6] = [
    declaration(AudioApi::MciCommandA, "mciSendCommandA", &[D, D, P, P]),
    declaration(AudioApi::MciCommandW, "mciSendCommandW", &[D, D, P, P]),
    declaration(AudioApi::MciStringA, "mciSendStringA", &[P, P, D, P]),
    declaration(AudioApi::MciStringW, "mciSendStringW", &[P, P, D, P]),
    declaration(AudioApi::WaveDevices, "waveOutGetNumDevs", &[]),
    declaration(AudioApi::WaveOpen, "waveOutOpen", &[P, D, P, P, P, D]),
];
