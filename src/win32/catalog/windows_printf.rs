//! Windows formatting keeps fixed parameters separate from on-demand varargs.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsPrintfApi {
    Ansi,
    Wide,
    ListAnsi,
    ListWide,
}

impl WindowsPrintfApi {
    pub(crate) const fn list(self) -> bool {
        matches!(self, Self::ListAnsi | Self::ListWide)
    }

    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::Wide | Self::ListWide)
    }
}

const fn declaration(id: WindowsPrintfApi, name: &'static str) -> Contract {
    use Parameter::Pointer as P;
    let list = id.list();
    let contract = Contract::new(
        ApiId::WindowsPrintf(id), name, if list { &[P, P, P] } else { &[P, P] }, ReturnConvention::CharacterCount,
        "PE32 exact user32 provider; wsprintf cdecl reads two fixed arguments, wvsprintf stdcall reads three; formatter reads varargs only as required, retains CP932/UTF-16 rules, 1023-unit limit, checked output and legacy return-frame alias order; Win64 remains absent",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    ).with_exact_dlls(&["user32.dll"]);
    if list {
        contract
    } else {
        contract.with_x86_cdecl()
    }
}

pub static WINDOWS_PRINTF_CONTRACTS: [Contract; 4] = [
    declaration(WindowsPrintfApi::Ansi, "wsprintfA"),
    declaration(WindowsPrintfApi::Wide, "wsprintfW"),
    declaration(WindowsPrintfApi::ListAnsi, "wvsprintfA"),
    declaration(WindowsPrintfApi::ListWide, "wvsprintfW"),
];
