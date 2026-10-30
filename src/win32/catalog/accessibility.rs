use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityApi {
    Clients,
    HostProvider,
    Event,
    PropertyChanged,
    Notification,
    StructureChanged,
    ReturnProvider,
}

const fn declaration(
    id: AccessibilityApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Accessibility(id), name, parameters, returns,
        "No UI Automation client, provider, event delivery or cross-process token; host provider output is checked/cleared on failure, raise calls return E_NOTIMPL; by-value VARIANT snapshots follow both Windows ABIs",
        [ArchitecturePlan { dlls: &["uiautomationcore.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
use Parameter::{Pointer as P, Uint32 as D, Variant as V};
use ReturnConvention::{BoolLastError as B, Hresult as H, Pointer as R};
pub static ACCESSIBILITY_CONTRACTS: [Contract; 7] = [
    declaration(AccessibilityApi::Clients, "UiaClientsAreListening", &[], B),
    declaration(
        AccessibilityApi::HostProvider,
        "UiaHostProviderFromHwnd",
        &[P, P],
        H,
    ),
    declaration(
        AccessibilityApi::Event,
        "UiaRaiseAutomationEvent",
        &[P, D],
        H,
    ),
    declaration(
        AccessibilityApi::PropertyChanged,
        "UiaRaiseAutomationPropertyChangedEvent",
        &[P, D, V, V],
        H,
    ),
    declaration(
        AccessibilityApi::Notification,
        "UiaRaiseNotificationEvent",
        &[P, D, D, P, P],
        H,
    ),
    declaration(
        AccessibilityApi::StructureChanged,
        "UiaRaiseStructureChangedEvent",
        &[P, D, P, D],
        H,
    ),
    declaration(
        AccessibilityApi::ReturnProvider,
        "UiaReturnRawElementProvider",
        &[P, P, P, P],
        R,
    ),
];
