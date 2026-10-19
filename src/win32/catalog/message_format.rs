use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageFormatApi {
    Ansi,
    Wide,
}
impl MessageFormatApi {
    pub(crate) fn wide(self) -> bool {
        self == Self::Wide
    }
}
const fn declaration(id: MessageFormatApi, name: &'static str) -> Contract {
    use Parameter::{Pointer as P, Uint32 as D};
    Contract::new(ApiId::MessageFormat(id), name, &[D, P, D, D, P, D, P], ReturnConvention::CharacterCount,
        "architecture-specific existing message sets and flags; CP932/UTF-16 units, original output/allocation/LastError order; PE32 process-heap allocation and trim note, Win64 LocalFree ownership, checked return-frame preflight and restricted insert/language behavior retained",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }])
}
pub static MESSAGE_FORMAT_CONTRACTS: [Contract; 2] = [
    declaration(MessageFormatApi::Ansi, "FormatMessageA"),
    declaration(MessageFormatApi::Wide, "FormatMessageW"),
];
