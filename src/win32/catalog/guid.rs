use super::{Architecture, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidApi {
    StringFromGuid2,
    ClsidFromString,
    IidFromString,
}

/// GUID scalar signatures use stdcall on x86 and positional Win64 arguments.
/// Known ABI and an implemented route do not imply ProgID/activation support.
#[derive(Debug)]
pub struct Contract {
    pub id: GuidApi,
    pub name: &'static str,
    pub parameters: &'static [Parameter],
    pub returns: ReturnConvention,
    pub limits: &'static str,
    x86_dlls: &'static [&'static str],
    x64_dlls: &'static [&'static str],
    dwords: [usize; 16],
    dword_count: usize,
}

impl Contract {
    const fn new(
        id: GuidApi,
        name: &'static str,
        parameters: &'static [Parameter],
        returns: ReturnConvention,
        limits: &'static str,
    ) -> Self {
        let mut dwords = [0; 16];
        let mut dword_count = 0;
        let mut i = 0;
        while i < parameters.len() {
            if matches!(parameters[i], Parameter::Int32) {
                dwords[dword_count] = i;
                dword_count += 1;
            }
            i += 1;
        }
        Self {
            id,
            name,
            parameters,
            returns,
            limits,
            x86_dlls: &["ole32.dll"],
            x64_dlls: &["ole32.dll", "combase.dll"],
            dwords,
            dword_count,
        }
    }
    pub fn dlls(&self, architecture: Architecture) -> &'static [&'static str] {
        match architecture {
            Architecture::X86 => self.x86_dlls,
            Architecture::X64 => self.x64_dlls,
        }
    }
    pub fn stdcall_bytes(&self) -> u32 {
        self.parameters.len() as u32 * 4
    }
    pub fn win64_dwords(&'static self) -> &'static [usize] {
        &self.dwords[..self.dword_count]
    }
}

pub static GUID_CONTRACTS: [Contract; 3] = [
    Contract::new(
        GuidApi::StringFromGuid2,
        "StringFromGUID2",
        &[Parameter::Pointer, Parameter::Pointer, Parameter::Int32],
        ReturnConvention::CharacterCount,
        "canonical 38-character braced GUID plus NUL",
    ),
    Contract::new(
        GuidApi::ClsidFromString,
        "CLSIDFromString",
        &[Parameter::Pointer, Parameter::Pointer],
        ReturnConvention::Hresult,
        "canonical GUID/null input; no activation-context or installed ProgIDs",
    ),
    Contract::new(
        GuidApi::IidFromString,
        "IIDFromString",
        &[Parameter::Pointer, Parameter::Pointer],
        ReturnConvention::Hresult,
        "canonical GUID/null input; architecture-specific legacy HRESULT and fault behavior",
    ),
];
