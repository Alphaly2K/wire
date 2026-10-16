use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarApi {
    SystemTime,
    LocalTime,
    SystemToFileTime,
    FileToSystemTime,
    CompareFileTime,
    FileToLocalFileTime,
    TimeZone,
    SystemToLocalTime,
    LocalToSystemTime,
    PreciseFileTime,
    SystemTimes,
}

const fn declaration(
    id: CalendarApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let pe32 = matches!(
        id,
        CalendarApi::SystemTime | CalendarApi::LocalTime | CalendarApi::TimeZone
    );
    let win64 = !matches!(id, CalendarApi::FileToLocalFileTime);
    Contract::new(
        ApiId::Calendar(id), name, parameters, returns,
        "existing host wall time, Windows calendar arithmetic, timezone rules and aggregate CPU counters; PE32 SYSTEMTIME and zero-zone output keep error 87 and legacy trace, conversions stay ABI-only; Win64 checked memory and saved return frame, validation/alias/error order, unsigned DWORD CompareFileTime carrier and real host DST rules remain unchanged",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if pe32 { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented },
            },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

use Parameter::Pointer as P;
use ReturnConvention::{BoolLastError as B, Uint32 as D, Void};

pub static CALENDAR_CONTRACTS: [Contract; 11] = [
    declaration(CalendarApi::SystemTime, "GetSystemTime", &[P], Void),
    declaration(CalendarApi::LocalTime, "GetLocalTime", &[P], Void),
    declaration(
        CalendarApi::SystemToFileTime,
        "SystemTimeToFileTime",
        &[P, P],
        B,
    ),
    declaration(
        CalendarApi::FileToSystemTime,
        "FileTimeToSystemTime",
        &[P, P],
        B,
    ),
    declaration(CalendarApi::CompareFileTime, "CompareFileTime", &[P, P], D),
    declaration(
        CalendarApi::FileToLocalFileTime,
        "FileTimeToLocalFileTime",
        &[P, P],
        B,
    ),
    declaration(CalendarApi::TimeZone, "GetTimeZoneInformation", &[P], D),
    declaration(
        CalendarApi::SystemToLocalTime,
        "SystemTimeToTzSpecificLocalTime",
        &[P, P, P],
        B,
    ),
    declaration(
        CalendarApi::LocalToSystemTime,
        "TzSpecificLocalTimeToSystemTime",
        &[P, P, P],
        B,
    ),
    declaration(
        CalendarApi::PreciseFileTime,
        "GetSystemTimePreciseAsFileTime",
        &[P],
        Void,
    ),
    declaration(CalendarApi::SystemTimes, "GetSystemTimes", &[P, P, P], B),
];
