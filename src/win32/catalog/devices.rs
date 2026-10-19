//! Empty guest device discovery and real process-owned notification subscriptions.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceApi {
    HidGuid,
    FreePreparsed,
    Attributes,
    Preparsed,
    Manufacturer,
    Product,
    Serial,
    Caps,
    MaxData,
    ButtonCaps,
    ValueCaps,
    Data,
    SetUsage,
    SetUsages,
    ClassA,
    ClassW,
    DestroySet,
    EnumDevice,
    EnumInterfaces,
    DetailA,
    DetailW,
    RegisterA,
    RegisterW,
    Unregister,
}

const H: &[&str] = &["hid.dll"];
const S: &[&str] = &["setupapi.dll"];
const W: &[&str] = &["user32.dll"];

const fn declaration(
    id: DeviceApi,
    name: &'static str,
    dlls: &'static [&'static str],
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let notification = matches!(
        id,
        DeviceApi::RegisterA | DeviceApi::RegisterW | DeviceApi::Unregister
    );
    Contract::new(
        ApiId::Device(id), name, parameters, returns,
        "PE32 HID/setupapi absent, user32 notification ABI-only; Win64 precise hid/setupapi/user32 static/dynamic providers, empty device sets and notifications own full-width monotonic handles, no PnP/HID backend or report descriptors, original flags/window/filter/string/handle/output validation and lifetime limits, HIDP status versus BOOL/LastError and VOID trace differences preserved, window destruction removes its subscriptions without fabricating device events",
        [ArchitecturePlan { dlls: if notification { W } else { &[] }, execution: if notification { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Absent } },
         ArchitecturePlan { dlls, execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Ntstatus as N, Pointer as R, Uint32 as U, Void as V};
pub static DEVICE_CONTRACTS: [Contract; 24] = [
    declaration(DeviceApi::HidGuid, "HidD_GetHidGuid", H, &[P], V),
    declaration(
        DeviceApi::FreePreparsed,
        "HidD_FreePreparsedData",
        H,
        &[P],
        B,
    ),
    declaration(DeviceApi::Attributes, "HidD_GetAttributes", H, &[P, P], B),
    declaration(DeviceApi::Preparsed, "HidD_GetPreparsedData", H, &[P, P], B),
    declaration(
        DeviceApi::Manufacturer,
        "HidD_GetManufacturerString",
        H,
        &[P, P, D],
        B,
    ),
    declaration(
        DeviceApi::Product,
        "HidD_GetProductString",
        H,
        &[P, P, D],
        B,
    ),
    declaration(
        DeviceApi::Serial,
        "HidD_GetSerialNumberString",
        H,
        &[P, P, D],
        B,
    ),
    declaration(DeviceApi::Caps, "HidP_GetCaps", H, &[P, P], N),
    declaration(DeviceApi::MaxData, "HidP_MaxDataListLength", H, &[D, P], U),
    declaration(
        DeviceApi::ButtonCaps,
        "HidP_GetButtonCaps",
        H,
        &[D, P, P, P],
        N,
    ),
    declaration(
        DeviceApi::ValueCaps,
        "HidP_GetValueCaps",
        H,
        &[D, P, P, P],
        N,
    ),
    declaration(DeviceApi::Data, "HidP_GetData", H, &[D, P, P, P, P, D], N),
    declaration(
        DeviceApi::SetUsage,
        "HidP_SetUsageValue",
        H,
        &[D, D, D, D, D, P, P, D],
        N,
    ),
    declaration(
        DeviceApi::SetUsages,
        "HidP_SetUsages",
        H,
        &[D, D, D, P, P, P, P, D],
        N,
    ),
    declaration(
        DeviceApi::ClassA,
        "SetupDiGetClassDevsA",
        S,
        &[P, P, P, D],
        R,
    ),
    declaration(
        DeviceApi::ClassW,
        "SetupDiGetClassDevsW",
        S,
        &[P, P, P, D],
        R,
    ),
    declaration(
        DeviceApi::DestroySet,
        "SetupDiDestroyDeviceInfoList",
        S,
        &[P],
        B,
    ),
    declaration(
        DeviceApi::EnumDevice,
        "SetupDiEnumDeviceInfo",
        S,
        &[P, D, P],
        B,
    ),
    declaration(
        DeviceApi::EnumInterfaces,
        "SetupDiEnumDeviceInterfaces",
        S,
        &[P, P, P, D, P],
        B,
    ),
    declaration(
        DeviceApi::DetailA,
        "SetupDiGetDeviceInterfaceDetailA",
        S,
        &[P, P, P, D, P, P],
        B,
    ),
    declaration(
        DeviceApi::DetailW,
        "SetupDiGetDeviceInterfaceDetailW",
        S,
        &[P, P, P, D, P, P],
        B,
    ),
    declaration(
        DeviceApi::RegisterA,
        "RegisterDeviceNotificationA",
        W,
        &[P, P, D],
        R,
    ),
    declaration(
        DeviceApi::RegisterW,
        "RegisterDeviceNotificationW",
        W,
        &[P, P, D],
        R,
    ),
    declaration(
        DeviceApi::Unregister,
        "UnregisterDeviceNotification",
        W,
        &[P],
        B,
    ),
];
