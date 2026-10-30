use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleApi {
    Attach,
    ControlHandler,
    ReadA,
    ReadW,
    Free,
    SetMode,
    InputEvents,
    PeekInputA,
    PeekInputW,
    ReadInputA,
    ReadInputW,
}

const fn declaration(
    id: ConsoleApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(ApiId::Console(id), name, parameters, ReturnConvention::BoolLastError,
        "No console object, attachment or control-event backend; byte streams remain redirected endpoints; attach/control registration fail 50, console input fails 6 without changing buffers or invoking callbacks",
        [ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
use Parameter::{Pointer as P, Uint32 as D};
pub static CONSOLE_CONTRACTS: [Contract; 11] = [
    declaration(ConsoleApi::Free, "FreeConsole", &[]),
    declaration(ConsoleApi::SetMode, "SetConsoleMode", &[P, D]),
    declaration(
        ConsoleApi::InputEvents,
        "GetNumberOfConsoleInputEvents",
        &[P, P],
    ),
    declaration(ConsoleApi::PeekInputA, "PeekConsoleInputA", &[P, P, D, P]),
    declaration(ConsoleApi::PeekInputW, "PeekConsoleInputW", &[P, P, D, P]),
    declaration(ConsoleApi::ReadInputA, "ReadConsoleInputA", &[P, P, D, P]),
    declaration(ConsoleApi::ReadInputW, "ReadConsoleInputW", &[P, P, D, P]),
    declaration(ConsoleApi::Attach, "AttachConsole", &[D]),
    declaration(ConsoleApi::ControlHandler, "SetConsoleCtrlHandler", &[P, D]),
    declaration(ConsoleApi::ReadA, "ReadConsoleA", &[P, P, D, P, P]),
    declaration(ConsoleApi::ReadW, "ReadConsoleW", &[P, P, D, P, P]),
];
