//! Windows export contracts and provider identities shared with the loader.
use crate::environment::WindowsVersion;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/address_wait.rs"]
pub(crate) mod address_wait;
mod address_waits;
#[allow(dead_code)]
#[path = "../../../src/win32/com/apartment.rs"]
pub(crate) mod apartment;
#[path = "../../../src/win32/kernel/apcs.rs"]
pub(crate) mod apcs;
#[allow(dead_code)]
#[path = "../../../src/win32/audio.rs"]
mod audio;
mod automation;
#[path = "../../../src/win32/com/automation32.rs"]
pub(crate) mod automation32;
#[path = "../../../src/win32/com/automation64.rs"]
pub(crate) mod automation64;
#[path = "../../../src/win32/basic_strings.rs"]
mod basic_string_core;
mod basic_strings;
mod bcrypt;
#[path = "../../../src/win32/byte_streams.rs"]
pub(crate) mod byte_streams;
#[path = "../../../src/win32/kernel/calendar.rs"]
mod calendar;
mod calendar_services;
pub mod call;
#[allow(dead_code, unused_imports)]
#[path = "../../../src/win32/catalog/mod.rs"]
pub mod catalog;
pub(crate) mod class_services;
mod com;
#[path = "../../../src/win32/kernel/condition.rs"]
pub(crate) mod condition;
pub(crate) mod context;
mod critical_sections;
pub(crate) mod crt;
#[path = "../../../src/win32/crt_memory.rs"]
mod crt_memory;
#[path = "../../../src/win32/crt_sort.rs"]
pub(crate) mod crt_sort;
pub(crate) mod crt_startup;
pub(crate) mod crypto;
mod cursor;
pub(crate) mod d3d9;
#[path = "../../../src/graphics/d3d9.rs"]
#[allow(dead_code)]
mod d3d9_core;
pub(crate) mod data_pages;
mod desktop;
#[path = "../../../src/win32/device_state.rs"]
pub(crate) mod device_state;
mod devices;
mod directories;
pub(crate) mod display;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) mod dsound;
#[path = "../../../src/graphics/dsound.rs"]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod dsound_core;
mod endpoints;
mod enumeration;
pub(crate) mod environment;
mod etw;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/exception_registrations.rs"]
pub(crate) mod exception_registrations;
#[allow(dead_code)]
#[path = "../../../src/win32/file_cursor.rs"]
pub(crate) mod file_cursor;
#[path = "../../../src/win32/file_information.rs"]
pub(crate) mod file_information;
mod file_input;
mod file_mutations;
#[path = "../../../src/win32/file_rights.rs"]
pub(crate) mod file_rights;
#[allow(dead_code)]
#[path = "../../../src/win32/file_search.rs"]
pub(crate) mod file_search;
mod files;
pub mod gateways;
pub(crate) mod gdi;
pub(crate) mod gdiplus;
#[path = "../../../src/graphics/gdiplus.rs"]
mod gdiplus_core;
mod geometry;
mod graphics_probe;
mod guid;
#[path = "../../../src/win32/com/guid.rs"]
mod guid_values;
mod handles;
pub(crate) mod heaps;
#[path = "../../../src/win32/com/hstrings.rs"]
mod hstrings_core;
mod identity;
mod images;
mod ime;
pub(crate) mod inline_sync;
mod interlocked;
mod keyboard;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/locals64.rs"]
mod local_slots;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/nls.rs"]
pub(crate) mod locale;
pub(crate) mod locals;
mod mappings;
mod menus;
mod message_format;
#[path = "../../../src/win32/message_text.rs"]
mod message_text;
pub(crate) mod messages;
mod module_queries;
#[path = "../../../src/win32/module_queries.rs"]
mod module_query_values;
mod modules;
mod multimedia;
mod nls;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/objects64.rs"]
pub(crate) mod object_state;
pub(crate) mod objects;
mod overlapped;
pub(crate) mod painting;
#[path = "../../../src/win32/path_helpers.rs"]
mod path_helpers;
mod paths;
#[path = "../../../src/win32/kernel/multimedia.rs"]
pub(crate) mod period_requests;
#[path = "../../../src/win32/pipes.rs"]
pub(crate) mod pipes;
mod platform_endpoints;
mod pointer_helpers;
#[path = "../../../src/win32/pointer_values.rs"]
mod pointer_values;
mod process_control;
mod process_create;
mod process_memory;
mod process_options;
mod process_queries;
mod property_values;
pub mod provider_aliases;
mod raw_input;
pub(crate) mod registry;
pub(crate) mod resources;
#[path = "../../../src/win32/rng_state.rs"]
pub(crate) mod rng_state;
pub(crate) mod safe_arrays;
mod security;
mod services;
mod shell;
#[path = "../../../src/win32/sid_values.rs"]
pub(crate) mod sid_values;
mod sids;
#[path = "../../../src/win32/slist.rs"]
mod slist;
mod snapshots;
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/srw.rs"]
pub(crate) mod srw;
mod sspi;
#[path = "../../../src/win32/sspi_state.rs"]
pub(crate) mod sspi_state;
#[path = "../../../src/win32/kernel/standard_handles.rs"]
pub(crate) mod standard_handles;
mod startup;
mod system_paths;
mod system_profile;
#[path = "../../../src/win32/system_topology.rs"]
mod system_topology;
mod thread_descriptions;
pub(crate) mod threads;
#[path = "../../../src/win32/kernel/timezone.rs"]
mod timezone;
pub(crate) mod tokens;
#[path = "../../../src/win32/toolhelp.rs"]
pub(crate) mod toolhelp;
#[path = "../../../src/win32/tracing_state.rs"]
pub(crate) mod tracing_state;
mod unavailable_endpoints;
pub(crate) mod user;
mod user_objects;
#[path = "../../../src/win32/utf16.rs"]
mod utf16;
#[path = "../../../src/win32/com/variant.rs"]
pub(crate) mod variant;
mod version;
#[path = "../../../src/win32/kernel/version.rs"]
mod version_core;
#[path = "../../../src/win32/kernel/version_resources.rs"]
mod version_resource_core;
mod version_resources;
mod virtual_memory;
mod windows_printf;
#[path = "../../../src/win32/windows_printf.rs"]
mod windows_printf_core;
mod winhttp;
#[path = "../../../src/win32/winhttp.rs"]
mod winhttp_core;
pub(crate) mod winrt;
mod winsock;
#[path = "../../../src/win32/winsock.rs"]
pub(crate) mod winsock_core;
mod winsock_names;
pub(crate) mod wmi;
#[path = "../../../src/win32/com/wmi.rs"]
mod wmi_core;
pub use services::{ClockSource, ServiceError, ServiceSample, Services};
pub(crate) mod graphics;
#[path = "../../../src/win32/providers.rs"]
pub mod providers;
