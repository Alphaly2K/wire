//! Behavioral tests for the Win32 subset, independent of a proprietary EXE.
use wire::api::{self, ApiTable};
use wire::cpu::Cpu;
use wire::loader::{STACK_BASE, STACK_SIZE, TEB_ADDR};
use wire::mem::GuestMem;
#[path = "support/x86.rs"]
mod support;
use support::{d3d_com, dll_export, test_dll, test_window, Vm, CODE, DATA, RET};

#[path = "win32/audio.rs"]
mod audio;
#[path = "win32/clocks.rs"]
mod clocks;
#[path = "win32/com.rs"]
mod com;
#[path = "win32/context.rs"]
mod context;
#[path = "win32/critical_sections.rs"]
mod critical_sections;
#[path = "win32/crt.rs"]
mod crt;
#[path = "win32/d3d9.rs"]
mod d3d9;
#[path = "win32/exceptions.rs"]
mod exceptions;
#[path = "win32/files.rs"]
mod files;
#[path = "win32/gdi.rs"]
mod gdi;
#[path = "win32/gdiplus.rs"]
mod gdiplus;
#[path = "win32/handles.rs"]
mod handles;
#[path = "win32/interlocked.rs"]
mod interlocked;
#[path = "win32/kernel.rs"]
mod kernel;
#[path = "win32/locals.rs"]
mod locals;
#[path = "win32/memory.rs"]
mod memory;
#[path = "win32/modules.rs"]
mod modules;
#[path = "win32/paths.rs"]
mod paths;
#[path = "win32/sections.rs"]
mod sections;
#[path = "win32/sync.rs"]
mod sync;
#[path = "win32/thread_properties.rs"]
mod thread_properties;
#[path = "win32/user.rs"]
mod user;
