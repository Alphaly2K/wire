//! Posted message, timer and queue coordination contracts.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageApi {
    SetTimer,
    KillTimer,
    PostA,
    PostW,
    PostThreadA,
    PostThreadW,
    Quit,
    PeekA,
    PeekW,
    GetA,
    GetW,
    DispatchA,
    DispatchW,
    Translate,
    Wait,
    WaitObjects,
    WaitObjectsEx,
    Extra,
    SetExtra,
    Time,
    Position,
    QueueStatus,
}
impl MessageApi {
    pub(crate) const fn get(self) -> bool {
        matches!(self, Self::GetA | Self::GetW)
    }
    pub(crate) const fn peek(self) -> bool {
        matches!(self, Self::PeekA | Self::PeekW)
    }
    pub(crate) const fn dispatch(self) -> bool {
        matches!(self, Self::DispatchA | Self::DispatchW)
    }
    pub(crate) const fn service_sent(self) -> bool {
        self.peek()
            || self.get()
            || matches!(self, Self::Wait | Self::WaitObjects | Self::WaitObjectsEx)
    }
    const fn win64(self) -> bool {
        !matches!(
            self,
            Self::SetTimer
                | Self::KillTimer
                | Self::Wait
                | Self::WaitObjects
                | Self::WaitObjectsEx
                | Self::QueueStatus
        )
    }
    const fn x86_policy(self) -> ExecutionPolicy {
        if matches!(
            self,
            Self::Extra | Self::SetExtra | Self::Time | Self::Position | Self::QueueStatus
        ) {
            ExecutionPolicy::LegacyUnimplemented
        } else {
            ExecutionPolicy::Typed
        }
    }
}
const fn declaration(
    id: MessageApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Message(id), name, parameters, returns,
        "preserve queue/filter/output and metadata commit order, posted-message restrictions, quit returns, timer ownership and message/object waits; PE32 metadata/queue status remain ABI-only, omitted Win64 APIs absent; callback and blocking require explicit process control",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: id.x86_policy() },
         ArchitecturePlan { dlls: if id.win64() { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if id.win64() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if id.win64() { &["user32.dll"] } else { &[] }])
}
use Parameter::{Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D, Void};
pub static MESSAGE_CONTRACTS: [Contract; 22] = [
    declaration(MessageApi::SetTimer, "SetTimer", &[P, P, U, P], H),
    declaration(MessageApi::KillTimer, "KillTimer", &[P, P], B),
    declaration(MessageApi::PostA, "PostMessageA", &[P, U, P, P], B),
    declaration(MessageApi::PostW, "PostMessageW", &[P, U, P, P], B),
    declaration(
        MessageApi::PostThreadA,
        "PostThreadMessageA",
        &[U, U, P, P],
        B,
    ),
    declaration(
        MessageApi::PostThreadW,
        "PostThreadMessageW",
        &[U, U, P, P],
        B,
    ),
    declaration(MessageApi::Quit, "PostQuitMessage", &[U], Void),
    declaration(MessageApi::PeekA, "PeekMessageA", &[P, P, U, U, U], B),
    declaration(MessageApi::PeekW, "PeekMessageW", &[P, P, U, U, U], B),
    declaration(MessageApi::GetA, "GetMessageA", &[P, P, U, U], B),
    declaration(MessageApi::GetW, "GetMessageW", &[P, P, U, U], B),
    declaration(MessageApi::DispatchA, "DispatchMessageA", &[P], H),
    declaration(MessageApi::DispatchW, "DispatchMessageW", &[P], H),
    declaration(MessageApi::Translate, "TranslateMessage", &[P], B),
    declaration(MessageApi::Wait, "WaitMessage", &[], B),
    declaration(
        MessageApi::WaitObjects,
        "MsgWaitForMultipleObjects",
        &[U, P, U, U, U],
        D,
    ),
    declaration(
        MessageApi::WaitObjectsEx,
        "MsgWaitForMultipleObjectsEx",
        &[U, P, U, U, U],
        D,
    ),
    declaration(MessageApi::Extra, "GetMessageExtraInfo", &[], H),
    declaration(MessageApi::SetExtra, "SetMessageExtraInfo", &[P], H),
    declaration(MessageApi::Time, "GetMessageTime", &[], D),
    declaration(MessageApi::Position, "GetMessagePos", &[], D),
    declaration(MessageApi::QueueStatus, "GetQueueStatus", &[U], D),
];
