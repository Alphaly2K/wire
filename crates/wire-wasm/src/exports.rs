//! Single-threaded host bridge. Module pins retain code and owner allocations
//! through installation and calls; staging pointers expire on input resizing.
use crate::{
    jit::CompileError,
    machine::{Machine, Mode, PrepareError, State},
};
use rustc_apfloat::ieee::{Double, Single};
use std::{
    cell::RefCell,
    mem::{offset_of, size_of},
};

struct Host {
    machine: Machine,
    closed: bool,
    input: Vec<u8>,
    fp_image: [u8; 512],
    extended: crate::extended::Output,
    paired: crate::extended::Pair,
    error: String,
    audio_fault: Option<crate::memory::Fault>,
}

thread_local! {
    static MACHINES: RefCell<Vec<Option<Box<Host>>>> = const { RefCell::new(Vec::new()) };
}

fn driver_bytes() -> &'static [u8] {
    static DRIVER: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    DRIVER.get_or_init(crate::jit::resident_driver)
}

#[no_mangle]
pub extern "C" fn wire_driver() -> u32 {
    driver_bytes().as_ptr() as u32
}

#[no_mangle]
pub extern "C" fn wire_driver_len() -> u32 {
    driver_bytes().len() as u32
}

fn with<T>(handle: u32, missing: T, f: impl FnOnce(&mut Host) -> T) -> T {
    let Some(index) = handle.checked_sub(1) else {
        return missing;
    };
    MACHINES.with(|machines| {
        machines
            .borrow_mut()
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .filter(|host| !host.closed)
            .map_or(missing, |host| f(host))
    })
}

#[no_mangle]
pub extern "C" fn wire_create(mode: u32, ip: u64, sp: u64) -> u32 {
    let mode = match mode {
        32 => Mode::X86,
        64 => Mode::X64,
        _ => return 0,
    };
    MACHINES.with(|machines| {
        let mut machines = machines.borrow_mut();
        let host = Box::new(Host {
            machine: Machine::new(mode, ip, sp),
            closed: false,
            input: Vec::new(),
            fp_image: [0; 512],
            extended: crate::extended::Output::default(),
            paired: crate::extended::Pair::default(),
            error: String::new(),
            audio_fault: None,
        });
        if let Some(index) = machines.iter().position(Option::is_none) {
            machines[index] = Some(host);
            index as u32 + 1
        } else {
            machines.push(Some(host));
            machines.len() as u32
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_destroy(handle: u32) {
    if let Some(index) = handle.checked_sub(1) {
        MACHINES.with(|machines| {
            if let Some(slot) = machines.borrow_mut().get_mut(index as usize) {
                if let Some(host) = slot {
                    host.closed = true;
                    host.machine.code_cache.clear();
                    if !host.machine.has_activity() {
                        *slot = None;
                    }
                }
            }
        });
    }
}

#[no_mangle]
pub extern "C" fn wire_child_mode(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .and_then(|i| i.children.front())
            .map_or(0, |child| child.memory.mode() as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_adopt_child(parent: u32, destination: u32) -> u32 {
    if parent == destination {
        return 1;
    }
    let mode = with(destination, 0, |host| {
        if host.machine.image.is_none()
            && !host.machine.has_activity()
            && host.machine.state.work == 0
        {
            host.machine.memory.mode() as u32
        } else {
            0
        }
    });
    if mode == 0 || mode != wire_child_mode(parent) {
        return 1;
    }
    let child = with(parent, None, |host| {
        if host.machine.has_activity() {
            return None;
        }
        host.machine.image.as_mut()?.children.pop_front()
    });
    let Some(child) = child else {
        return 1;
    };
    with(destination, 1, |host| {
        host.machine = *child;
        0
    })
}

#[no_mangle]
pub extern "C" fn wire_process_id(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.image.as_ref().map_or(0, |i| i.process.id())
    })
}

#[no_mangle]
pub extern "C" fn wire_finish_process_entry(handle: u32) -> u32 {
    with(handle, 2, |host| {
        let machine = &mut host.machine;
        if machine.has_activity() {
            return 2;
        }
        let Some(image) = machine.image.as_mut() else {
            return 2;
        };
        let code = machine.state.regs[0] as u32;
        match image.begin_process_exit(code, &mut machine.state, machine.memory.mode()) {
            Ok(()) => 0,
            Err(_) => {
                host.error = "process entry exit could not be started".into();
                2
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_state(handle: u32) -> u32 {
    with(handle, 0, |host| (&raw mut host.machine.state) as u32)
}

#[no_mangle]
pub extern "C" fn wire_cancel_repeat(handle: u32) {
    with(handle, (), |host| {
        host.machine.state.cancel_repeat();
        if let Some(image) = host.machine.image.as_mut() {
            image.clear_all_pending();
            image.threads.edited();
        }
    });
}

#[no_mangle]
pub extern "C" fn wire_state_layout() -> u32 {
    static LAYOUT: [u32; 11] = [
        size_of::<State>() as u32,
        offset_of!(State, regs) as u32,
        offset_of!(State, flags) as u32,
        offset_of!(State, ip) as u32,
        offset_of!(State, work) as u32,
        offset_of!(State, retired) as u32,
        offset_of!(State, fs_base) as u32,
        offset_of!(State, gs_base) as u32,
        offset_of!(State, exception) as u32,
        offset_of!(State, xmm) as u32,
        offset_of!(State, mxcsr) as u32,
    ];
    LAYOUT.as_ptr() as u32
}

#[no_mangle]
pub extern "C" fn wire_fpu_layout() -> u32 {
    static LAYOUT: [u32; 7] = [
        offset_of!(State, fpu.cw) as u32,
        offset_of!(State, fpu.sw) as u32,
        offset_of!(State, fpu.tags) as u32,
        offset_of!(State, fpu.regs) as u32,
        offset_of!(State, fpu.ip) as u32,
        offset_of!(State, fpu.dp) as u32,
        offset_of!(State, fpu.opcode) as u32,
    ];
    LAYOUT.as_ptr() as u32
}

#[no_mangle]
pub extern "C" fn wire_exception(handle: u32) -> u32 {
    with(handle, 0, |host| host.machine.state.exception)
}

#[no_mangle]
pub extern "C" fn wire_exception_code(handle: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .and_then(|i| i.threads.current().unhandled)
            .map_or(0, |code| u64::from(code) + 1)
    })
}

#[no_mangle]
pub extern "C" fn wire_deliver_exception(handle: u32, reason: u32) -> u32 {
    with(handle, 3, |host| {
        let reason = match reason {
            3 => crate::jit::Exit::Fault,
            5 => crate::jit::Exit::Exception,
            _ => {
                host.error = "invalid exception delivery reason".into();
                return 3;
            }
        };
        match host.machine.deliver_exception(reason) {
            Ok(handled) => u32::from(handled),
            Err(crate::process::StartupError::Fault(fault)) => {
                host.machine.memory.record_fault(fault);
                2
            }
            Err(crate::process::StartupError::Invalid(message)) => {
                host.error = message;
                3
            }
            _ => unreachable!(),
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_clock_frequency() -> u32 {
    crate::cpu::CLOCK_FREQUENCY
}

#[no_mangle]
pub extern "C" fn wire_boundary(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.boundary().map_or(0, |exit| exit as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_process_exit_code(handle: u32) -> u64 {
    with(handle, u64::MAX, |host| {
        host.machine.process_exit_code().map_or(u64::MAX, u64::from)
    })
}

#[no_mangle]
pub extern "C" fn wire_configure_timezone(handle: u32) -> u32 {
    with(handle, 1, |host| {
        let result = (|| {
            let bytes = host
                .input
                .as_slice()
                .try_into()
                .map_err(|_| "timezone profile must contain 172 bytes".to_owned())?;
            host.machine.configure_timezone(bytes)
        })();
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_quantum(handle: u32) -> u64 {
    with(handle, u64::MAX, |host| {
        host.machine
            .image
            .as_ref()
            .map_or(u64::MAX, |i| i.threads.quantum(host.machine.state.work))
    })
}

#[no_mangle]
pub extern "C" fn wire_schedule(handle: u32, flags: u32) -> u32 {
    with(handle, 2, |host| {
        if host.machine.has_activity() || flags > 3 {
            host.error = "invalid scheduler boundary".into();
            return 2;
        }
        match host.machine.schedule(
            match flags {
                1 => Some(crate::machine::WaitReason::Retry),
                3 => Some(crate::machine::WaitReason::Loader),
                _ => None,
            },
            flags == 2,
        ) {
            Err(crate::process::StartupError::FileInput(_)) => 6,
            Ok(None) => 0,
            Ok(Some(crate::jit::Exit::Waiting)) => 3,
            Ok(Some(crate::jit::Exit::ThreadReturned)) => 4,
            Ok(Some(crate::jit::Exit::ProcessExited)) => 5,
            Ok(Some(_)) => unreachable!(),
            Err(crate::process::StartupError::Fault(fault)) => {
                host.machine.memory.record_fault(fault);
                1
            }
            Err(crate::process::StartupError::Rejected) => {
                host.error = "DLL process attach returned FALSE".into();
                2
            }
            Err(crate::process::StartupError::Unhandled(_)) => {
                unreachable!("scheduler does not dispatch callbacks")
            }
            Err(crate::process::StartupError::Invalid(message)) => {
                host.error = message;
                2
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_entry_layout(mode: u32) -> u32 {
    static X86: [u64; 5] = crate::win32::gateways::layout(Mode::X86).words();
    static X64: [u64; 5] = crate::win32::gateways::layout(Mode::X64).words();
    match mode {
        32 => X86.as_ptr() as u32,
        64 => X64.as_ptr() as u32,
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn wire_bind(handle: u32) -> u64 {
    with(handle, 0, |host| {
        let result = std::str::from_utf8(&host.input)
            .ok()
            .and_then(|s| s.split_once('\0'))
            .ok_or_else(|| "invalid Windows export identity".to_owned())
            .and_then(|(dll, name)| host.machine.services.bind(dll, name));
        host.input.clear();
        match result {
            Ok(address) => address,
            Err(error) => {
                host.error = error;
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_service_clock(handle: u32) -> u32 {
    service_source(handle, false)
}

#[no_mangle]
pub extern "C" fn wire_service_source(handle: u32) -> u32 {
    service_source(handle, true)
}

fn service_source(handle: u32, include_random: bool) -> u32 {
    with(handle, 0, |host| {
        if host
            .machine
            .image
            .as_ref()
            .is_some_and(|image| image.is_delay(host.machine.state.ip))
        {
            return crate::win32::ClockSource::None as u32;
        }
        let source = host
            .machine
            .services
            .clock_source(
                &host.machine.state,
                &host.machine.memory,
                host.machine.image.as_ref(),
            )
            .map_or(0, |source| source as u32);
        let random = if include_random
            && source != 0
            && !host.machine.has_activity()
            && host.machine.process_exit_code().is_none()
        {
            host.machine.services.random_request(
                &host.machine.state,
                &host.machine.memory,
                host.machine.image.as_ref(),
            )
        } else {
            0
        };
        source | random
    })
}

#[no_mangle]
pub extern "C" fn wire_service(handle: u32, sample: u64) -> u32 {
    service(
        handle,
        crate::win32::ServiceSample {
            elapsed: sample,
            ..Default::default()
        },
    )
}

#[no_mangle]
pub extern "C" fn wire_service_timed(handle: u32, sample: u64, wall: u64) -> u32 {
    service(
        handle,
        crate::win32::ServiceSample {
            elapsed: sample,
            wall: Some(wall),
            ..Default::default()
        },
    )
}

#[no_mangle]
pub extern "C" fn wire_service_cpu(
    handle: u32,
    idle: u64,
    kernel: u64,
    user: u64,
    error: u32,
) -> u32 {
    service(
        handle,
        crate::win32::ServiceSample {
            cpu_times: Some(if error == 0 {
                Ok([idle, kernel, user])
            } else {
                Err(error)
            }),
            ..Default::default()
        },
    )
}

#[no_mangle]
pub extern "C" fn wire_service_random(handle: u32, low: u64, high: u64, available: u32) -> u32 {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&low.to_le_bytes());
    bytes[8..].copy_from_slice(&high.to_le_bytes());
    service(
        handle,
        crate::win32::ServiceSample {
            random: (available != 0).then_some(bytes.as_slice()),
            ..Default::default()
        },
    )
}

#[no_mangle]
pub extern "C" fn wire_service_random_bytes(handle: u32, available: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if available != 0 && host.input.len() > crate::win32::rng_state::LIMIT as usize {
            host.error = "random sample exceeds capacity".into();
            return 2;
        }
        let mut input = std::mem::take(&mut host.input);
        let result = service_host(
            host,
            crate::win32::ServiceSample {
                random: (available != 0).then_some(input.as_slice()),
                ..Default::default()
            },
        );
        input.clear();
        host.input = input;
        result
    })
}

fn service(handle: u32, platform: crate::win32::ServiceSample<'_>) -> u32 {
    with(handle, u32::MAX, |host| service_host(host, platform))
}

fn service_host(host: &mut Host, platform: crate::win32::ServiceSample<'_>) -> u32 {
    if host.machine.has_activity() {
        host.error = "machine execution is already active".into();
        return 2;
    }
    let machine = &mut host.machine;
    if machine.process_exit_code().is_some() {
        machine.retire_process_code();
        return 5;
    }
    machine.memory.clear_fault();
    let result = machine.services.dispatch(
        &mut machine.state,
        &mut machine.memory,
        machine.image.as_mut(),
        &mut machine.module_files,
        platform,
    );
    if let Some(image) = machine.image.as_mut() {
        image.refresh_loader();
        if result.is_ok() {
            image.threads.completed();
            image.reconcile_pending(&machine.state);
        }
    }
    machine.retire_process_code();
    if !matches!(result, Err(crate::win32::ServiceError::FileInput(_))) {
        machine.module_files.finish_input();
    }
    match result {
        Err(crate::win32::ServiceError::FileInput(_)) => 8,
        Ok(()) if machine.process_exit_code().is_some() => 5,
        Ok(())
            if machine
                .image
                .as_ref()
                .is_some_and(|image| !image.children.is_empty()) =>
        {
            6
        }
        Ok(()) => 0,
        Err(crate::win32::ServiceError::Fault(fault)) => {
            machine.memory.record_fault(fault);
            1
        }
        Err(crate::win32::ServiceError::Unbound) => {
            host.error = "unbound Windows service gateway".into();
            2
        }
        Err(crate::win32::ServiceError::ImportTrap { dll, symbol }) => {
            host.error = format!("unimplemented diagnostic import: {dll}!{symbol}");
            2
        }
        Err(crate::win32::ServiceError::Unavailable(message)) => {
            host.error = message.into();
            2
        }
        Err(crate::win32::ServiceError::Graphics) => 7,
        Err(crate::win32::ServiceError::GraphicsFailed(message)) => {
            host.error = message;
            2
        }
        Err(crate::win32::ServiceError::Blocked) => 3,
        Err(crate::win32::ServiceError::LoaderBlocked) => 4,
    }
}

#[no_mangle]
pub extern "C" fn wire_graphics_abi() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn wire_audio_enable(handle: u32, now: u64) -> u32 {
    with(handle, 2, |host| {
        if host.machine.has_activity() || host.machine.memory.mode() != Mode::X86 {
            return 2;
        }
        let Some(image) = host.machine.image.as_mut() else {
            return 2;
        };
        if image.audio.enabled || image.process.exit.is_some() {
            return 2;
        }
        image.audio.enable(now);
        0
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_enabled(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(host.machine.image.as_ref().is_some_and(|i| i.audio.enabled))
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_required(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(host.machine.services.requires_audio(host.machine.state.ip))
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_pump(handle: u32, now: u64) -> u32 {
    with(handle, 2, |host| {
        let machine = &mut host.machine;
        if machine.has_activity() {
            return 2;
        }
        let Some(image) = machine.image.as_mut() else {
            return 2;
        };
        host.audio_fault = None;
        match image
            .audio
            .pump(&mut machine.memory, &mut image.objects, now)
        {
            Ok(()) => 0,
            Err(crate::win32::ServiceError::Fault(fault)) => {
                host.audio_fault = Some(fault);
                1
            }
            Err(error) => {
                host.error = format!("audio render failed: {error:?}");
                2
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_audio_fault(handle: u32, part: u32) -> u64 {
    with(handle, 0, |host| {
        host.audio_fault.map_or(0, |f| match part {
            0 => f.kind as u64,
            1 => f.address,
            2 => u64::from(f.access),
            _ => 0,
        })
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_samples(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .map_or(0, |i| i.audio.pcm.as_ptr() as u32)
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_sample_count(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .map_or(0, |i| i.audio.pcm.len() as u32)
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_ack(handle: u32) {
    with(handle, (), |host| {
        if !host.machine.has_activity() {
            if let Some(image) = host.machine.image.as_mut() {
                image.audio.pcm.clear();
            }
        }
    })
}
#[no_mangle]
pub extern "C" fn wire_audio_stat(handle: u32, index: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .map_or(0, |i| i.audio.stat(index))
    })
}

#[no_mangle]
pub extern "C" fn wire_graphics_pending(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(
            host.machine
                .image
                .as_ref()
                .is_some_and(|i| i.graphics.pending()),
        )
    })
}

#[no_mangle]
pub extern "C" fn wire_graphics_attached(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(
            host.machine
                .image
                .as_ref()
                .is_some_and(|image| image.graphics.attached()),
        )
    })
}

#[no_mangle]
pub extern "C" fn wire_graphics_required(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(
            host.machine
                .services
                .requires_graphics(host.machine.state.ip),
        )
    })
}

#[no_mangle]
pub extern "C" fn wire_attach_graphics(handle: u32, slot: u32) -> u32 {
    with(handle, 2, |host| {
        let result = (|| {
            if host.machine.has_activity() || host.machine.memory.mode() != Mode::X64 {
                return Err("graphics setup requires a suspended AMD64 image".to_owned());
            }
            let image = host
                .machine
                .image
                .as_mut()
                .ok_or("graphics setup requires a loaded image")?;
            let (info, formats) = crate::gpu::Backend::profile(&host.input)?;
            image
                .graphics
                .attach(crate::gpu::Backend::from_table(info, formats, slot)?)
        })();
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                2
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_window_pixels(handle: u32, window: u64) -> u32 {
    with(handle, u32::MAX, |host| {
        match host.machine.window_pixels(window) {
            Ok(Some(frame)) => {
                host.input.clear();
                if host.input.try_reserve_exact(16 + frame.rgba.len()).is_err() {
                    host.error = "pixel output allocation failed".into();
                    return u32::MAX;
                }
                host.input.extend_from_slice(&frame.width.to_le_bytes());
                host.input.extend_from_slice(&frame.height.to_le_bytes());
                host.input
                    .extend_from_slice(&(frame.width * 4).to_le_bytes());
                host.input.extend_from_slice(&1u32.to_le_bytes());
                host.input.extend_from_slice(&frame.rgba);
                host.input.len() as u32
            }
            Ok(None) => {
                host.input.clear();
                0
            }
            Err(error) => {
                host.error = error;
                u32::MAX
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_window_origin(handle: u32, window: u64) -> u32 {
    with(handle, u32::MAX, |host| {
        if host.machine.has_activity() {
            host.error = "window query requires a suspended machine".into();
            return u32::MAX;
        }
        host.input.clear();
        let origin = host
            .machine
            .image
            .as_ref()
            .and_then(|image| image.user.client_origin(window).ok());
        if let Some(origin) = origin {
            for coordinate in origin {
                host.input.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        host.input.len() as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_windows(handle: u32) -> u32 {
    with(handle, u32::MAX, |host| match host.machine.window_info() {
        Ok(windows) => {
            host.input.clear();
            for window in windows {
                host.input.extend_from_slice(&window.handle.to_le_bytes());
                host.input.extend_from_slice(&window.width.to_le_bytes());
                host.input.extend_from_slice(&window.height.to_le_bytes());
                let flags = u32::from(window.visible) | (u32::from(window.topmost) << 1);
                host.input.extend_from_slice(&flags.to_le_bytes());
                host.input
                    .extend_from_slice(&(window.title.len() as u32).to_le_bytes());
                host.input.extend_from_slice(window.title.as_bytes());
            }
            host.input.len() as u32
        }
        Err(error) => {
            host.error = error;
            u32::MAX
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_queue_window_events(handle: u32) -> u32 {
    with(handle, 1, |host| {
        let result = (|| {
            if !host.input.len().is_multiple_of(48) || host.input.len() / 48 > 256 {
                return Err("invalid host event batch framing".into());
            }
            let mut events = Vec::with_capacity(host.input.len() / 48);
            for bytes in host.input.as_chunks::<48>().0 {
                let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                let qword = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
                let flags = dword(12);
                if flags & !1 != 0 || dword(44) != 0 {
                    return Err("invalid host event flags".into());
                }
                events.push(crate::machine::WindowInput {
                    event: crate::WindowEvent {
                        window: qword(0),
                        message: dword(8),
                        wparam: qword(16),
                        lparam: qword(24),
                    },
                    time: dword(32),
                    point: (flags != 0).then(|| [dword(36) as i32, dword(40) as i32]),
                });
            }
            host.machine.queue_window_events(&events)
        })();
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_write_input(handle: u32, end: u32) -> u32 {
    with(handle, 1, |host| {
        let result = if end > 1 {
            Err("invalid input end flag".into())
        } else {
            host.machine.write_input(&host.input, end != 0)
        };
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_read_output(handle: u32, endpoint: u32, limit: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if limit > 1024 * 1024 {
            host.error = "output batch exceeds capacity".into();
            return u32::MAX;
        }
        if host
            .input
            .try_reserve((limit as usize).saturating_sub(host.input.len()))
            .is_err()
        {
            host.error = "output staging allocation failed".into();
            return u32::MAX;
        }
        host.input.resize(limit as usize, 0);
        match host.machine.read_output(endpoint, &mut host.input) {
            Ok(length) => {
                host.input.truncate(length);
                length as u32
            }
            Err(error) => {
                host.error = error;
                u32::MAX
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_load_pe(handle: u32, base: u64, command_len: u32) -> u32 {
    load_pe(handle, base, command_len, false)
}

#[no_mangle]
pub extern "C" fn wire_load_pe_diagnostic(handle: u32, base: u64, command_len: u32) -> u32 {
    load_pe(handle, base, command_len, true)
}

#[no_mangle]
pub extern "C" fn wire_import_trap_count(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.services.import_trap_count() as u32
    })
}

fn load_pe(handle: u32, base: u64, command_len: u32, diagnostic_imports: bool) -> u32 {
    with(handle, u32::MAX, |host| {
        let result = host
            .input
            .split_at_checked(command_len as usize)
            .ok_or_else(|| "invalid PE input framing".to_owned())
            .and_then(|(command, bytes)| {
                std::str::from_utf8(command)
                    .map_err(|_| "invalid command line UTF-8".to_owned())
                    .and_then(|command| {
                        if diagnostic_imports {
                            host.machine.load_pe_diagnostic(bytes, base, command)
                        } else {
                            host.machine.load_pe(bytes, base, command)
                        }
                    })
            });
        host.input.clear();
        if result.is_err() && host.machine.module_files.pending_input().is_some() {
            return 3;
        }
        host.machine.module_files.finish_input();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_image_info(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .image
            .as_ref()
            .map_or(0, |image| image.info.as_ptr() as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_startup_pending(handle: u32) -> u32 {
    with(handle, 0, |host| u32::from(host.machine.startup_pending()))
}

#[no_mangle]
pub extern "C" fn wire_startup(handle: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if host.machine.has_activity() {
            host.error = "machine execution is already active".into();
            return 2;
        }
        let result = host.machine.advance_startup();
        // Scheduler/startup checks also run before retrying a suspended service.
        // Its transient input must survive until that service consumes it.
        let service_input = host
            .machine
            .image
            .as_ref()
            .and_then(|image| image.pending())
            .is_some_and(|pending| {
                matches!(
                    pending.kind,
                    crate::process::threads::PendingKind::FileInput
                )
            });
        if !service_input && !matches!(result, Err(crate::process::StartupError::FileInput(_))) {
            host.machine.module_files.finish_input();
        }
        match result {
            Err(crate::process::StartupError::FileInput(_)) => 4,
            Ok(()) => 0,
            Err(crate::process::StartupError::Unhandled(_)) => 3,
            Err(crate::process::StartupError::Fault(fault)) => {
                host.machine.memory.record_fault(fault);
                1
            }
            Err(crate::process::StartupError::Rejected) => {
                host.error = "DLL process attach returned FALSE".into();
                2
            }
            Err(crate::process::StartupError::Invalid(message)) => {
                host.error = message;
                2
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_supply_module(handle: u32, name_len: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if host.machine.has_activity() {
            host.error = "module input requires an idle VM".into();
            return 1;
        }
        let result = host
            .input
            .split_at_checked(name_len as usize)
            .ok_or_else(|| "invalid module input framing".to_owned())
            .and_then(|(name, bytes)| {
                std::str::from_utf8(name)
                    .map_err(|_| "invalid module identity UTF-8".to_owned())
                    .and_then(|name| host.machine.module_files.supply(name, bytes))
            });
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_supply_file_source(handle: u32, path_len: u32, size: u64) -> u32 {
    with(handle, 0, |host| {
        let result = (|| {
            if host.machine.has_activity() || host.input.len() != path_len as usize {
                return Err("invalid file source input".into());
            }
            let path = std::str::from_utf8(&host.input)
                .map_err(|_| "invalid file source path UTF-8".to_owned())?;
            host.machine.module_files.insert_source(path, size)
        })();
        host.input.clear();
        match result {
            Ok(id) => id,
            Err(error) => {
                host.error = error;
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_file_pending(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(host.machine.module_files.pending_input().is_some())
    })
}

#[no_mangle]
pub extern "C" fn wire_file_request(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.input.clear();
        let Some(request) = host.machine.module_files.pending_input() else {
            return 0;
        };
        for value in [
            u64::from(request.source),
            request.offset,
            request.length as u64,
        ] {
            host.input.extend_from_slice(&value.to_le_bytes());
        }
        24
    })
}

#[no_mangle]
pub extern "C" fn wire_supply_file_input(handle: u32, source: u32, offset: u64) -> u32 {
    with(handle, 1, |host| {
        let result = (|| {
            if host.machine.has_activity() {
                return Err("file input requires an idle VM".into());
            }
            let request = host
                .machine
                .module_files
                .pending_input()
                .filter(|r| r.source == source && r.offset == offset)
                .ok_or_else(|| "file input has no matching request".to_owned())?;
            host.machine
                .module_files
                .supply_input(request, std::mem::take(&mut host.input))
        })();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_file_cache_bytes(handle: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine.module_files.input_cache_bytes() as u64
    })
}

#[no_mangle]
pub extern "C" fn wire_file_owned_bytes(handle: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine.module_files.owned_bytes() as u64
    })
}

#[no_mangle]
pub extern "C" fn wire_supply_directory(handle: u32, path_len: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if host.machine.has_activity() {
            host.error = "directory input requires an idle VM".into();
            return 1;
        }
        let result = (|| {
            if host.input.len() != path_len as usize {
                return Err("invalid directory input framing".to_owned());
            }
            let path = std::str::from_utf8(&host.input)
                .map_err(|_| "invalid directory path UTF-8".to_owned())?;
            host.machine
                .module_files
                .create_directory_all(path)
                .map_err(|code| format!("directory input error {code}"))
        })();
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_supply_file(handle: u32, path_len: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        if host.machine.has_activity() {
            host.error = "file input requires an idle VM".into();
            return 1;
        }
        let result = host
            .input
            .split_at_checked(path_len as usize)
            .ok_or_else(|| "invalid file input framing".to_owned())
            .and_then(|(path, bytes)| {
                let path =
                    std::str::from_utf8(path).map_err(|_| "invalid file path UTF-8".to_owned())?;
                host.machine.module_files.insert_slice(path, bytes)
            });
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_image_base(handle: u32) -> u64 {
    with(handle, 0, |host| {
        let value = std::str::from_utf8(&host.input)
            .ok()
            .and_then(|name| {
                host.machine
                    .image
                    .as_ref()
                    .map(|image| image.modules.base_at(name, host.machine.module_files.cwd()))
            })
            .unwrap_or(0);
        host.input.clear();
        value
    })
}

#[no_mangle]
pub extern "C" fn wire_crc8(crc: u32, value: u64) -> u32 {
    crate::integer::crc32::<1>(crc, value)
}
#[no_mangle]
pub extern "C" fn wire_crc16(crc: u32, value: u64) -> u32 {
    crate::integer::crc32::<2>(crc, value)
}
#[no_mangle]
pub extern "C" fn wire_crc32(crc: u32, value: u64) -> u32 {
    crate::integer::crc32::<4>(crc, value)
}
#[no_mangle]
pub extern "C" fn wire_crc64(crc: u32, value: u64) -> u32 {
    crate::integer::crc32::<8>(crc, value)
}

#[no_mangle]
pub extern "C" fn wire_mul_high(a: u64, b: u64) -> u64 {
    crate::integer::mul_high(a, b)
}

#[no_mangle]
pub extern "C" fn wire_imul_high(a: u64, b: u64) -> u64 {
    crate::integer::imul_high(a, b)
}

#[no_mangle]
pub extern "C" fn wire_div64(handle: u32, low: u64, high: u64, divisor: u64) -> u32 {
    with(handle, 0, |host| {
        if let Some(result) = crate::integer::div64(low, high, divisor) {
            host.machine.numeric = result;
            host.machine.numeric.as_ptr() as u32
        } else {
            0
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_idiv64(handle: u32, low: u64, high: u64, divisor: u64) -> u32 {
    with(handle, 0, |host| {
        if let Some(result) = crate::integer::idiv64(low, high, divisor) {
            host.machine.numeric = result;
            host.machine.numeric.as_ptr() as u32
        } else {
            0
        }
    })
}

fn floating_result(handle: u32, result: crate::floating::Output) -> u32 {
    with(handle, 0, |host| {
        host.machine.floating = result;
        (&raw const host.machine.floating) as u32
    })
}

macro_rules! extended {
    ($name:ident,$operation:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            cw: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            sw: u32,
        ) -> u32 {
            let a = u128::from(a0) | (u128::from(a1) << 64);
            let b = u128::from(b0) | (u128::from(b1) << 64);
            let result = ($operation)(cw, sw, a, b);
            with(owner, 0, |host| {
                host.extended = result;
                (&raw const host.extended) as u32
            })
        }
    };
}
extended!(
    wire_ext_load32,
    |cw, sw, _, b| crate::extended::load_real::<Single>(cw, sw, b)
);
extended!(
    wire_ext_load64,
    |cw, sw, _, b| crate::extended::load_real::<Double>(cw, sw, b)
);
extended!(wire_ext_integer, |cw, sw, _, b| {
    crate::extended::load_integer(cw, sw, b as i64)
});
extended!(wire_ext_store32, |cw, sw, a, _| {
    crate::extended::store_single(cw, sw, a)
});
extended!(wire_ext_store64, |cw, sw, a, _| {
    crate::extended::store_double(cw, sw, a)
});
extended!(
    wire_ext_i16,
    |cw, sw, a, _| crate::extended::store_integer::<16, false>(cw, sw, a)
);
extended!(
    wire_ext_i32,
    |cw, sw, a, _| crate::extended::store_integer::<32, false>(cw, sw, a)
);
extended!(
    wire_ext_i64,
    |cw, sw, a, _| crate::extended::store_integer::<64, false>(cw, sw, a)
);
extended!(
    wire_ext_t16,
    |cw, sw, a, _| crate::extended::store_integer::<16, true>(cw, sw, a)
);
extended!(
    wire_ext_t32,
    |cw, sw, a, _| crate::extended::store_integer::<32, true>(cw, sw, a)
);
extended!(
    wire_ext_t64,
    |cw, sw, a, _| crate::extended::store_integer::<64, true>(cw, sw, a)
);
extended!(wire_ext_one, |cw, sw, _, _| crate::extended::constant::<0>(
    cw, sw
));
extended!(wire_ext_l2t, |cw, sw, _, _| crate::extended::constant::<1>(
    cw, sw
));
extended!(wire_ext_l2e, |cw, sw, _, _| crate::extended::constant::<2>(
    cw, sw
));
extended!(wire_ext_pi, |cw, sw, _, _| crate::extended::constant::<3>(
    cw, sw
));
extended!(wire_ext_lg2, |cw, sw, _, _| crate::extended::constant::<4>(
    cw, sw
));
extended!(wire_ext_ln2, |cw, sw, _, _| crate::extended::constant::<5>(
    cw, sw
));
extended!(
    wire_ext_zero,
    |cw, sw, _, _| crate::extended::constant::<6>(cw, sw)
);
extended!(wire_ext_add, crate::extended::binary::<0>);
extended!(wire_ext_mul, crate::extended::binary::<1>);
extended!(wire_ext_sub, crate::extended::binary::<4>);
extended!(wire_ext_div, crate::extended::binary::<6>);
macro_rules! extended_memory {
    ($name:ident,$kind:literal,$size:literal,$integer:literal,$reverse:literal) => {
        extended!($name, |cw, sw, a, b| crate::extended::binary_memory::<
            $kind,
            $size,
            $integer,
            $reverse,
        >(cw, sw, a, b as u64));
    };
}
extended_memory!(wire_ext_add32, 0, 4, false, false);
extended_memory!(wire_ext_add64, 0, 8, false, false);
extended_memory!(wire_ext_addi16, 0, 2, true, false);
extended_memory!(wire_ext_addi32, 0, 4, true, false);
extended_memory!(wire_ext_mul32, 1, 4, false, false);
extended_memory!(wire_ext_mul64, 1, 8, false, false);
extended_memory!(wire_ext_muli16, 1, 2, true, false);
extended_memory!(wire_ext_muli32, 1, 4, true, false);
extended_memory!(wire_ext_sub32, 4, 4, false, false);
extended_memory!(wire_ext_sub64, 4, 8, false, false);
extended_memory!(wire_ext_subi16, 4, 2, true, false);
extended_memory!(wire_ext_subi32, 4, 4, true, false);
extended_memory!(wire_ext_rsub32, 4, 4, false, true);
extended_memory!(wire_ext_rsub64, 4, 8, false, true);
extended_memory!(wire_ext_rsubi16, 4, 2, true, true);
extended_memory!(wire_ext_rsubi32, 4, 4, true, true);
extended_memory!(wire_ext_div32, 6, 4, false, false);
extended_memory!(wire_ext_div64, 6, 8, false, false);
extended_memory!(wire_ext_divi16, 6, 2, true, false);
extended_memory!(wire_ext_divi32, 6, 4, true, false);
extended_memory!(wire_ext_rdiv32, 6, 4, false, true);
extended_memory!(wire_ext_rdiv64, 6, 8, false, true);
extended_memory!(wire_ext_rdivi16, 6, 2, true, true);
extended_memory!(wire_ext_rdivi32, 6, 4, true, true);
extended!(wire_ext_sqrt, |cw, sw, a, _| crate::extended::sqrt(
    cw, sw, a
));
extended!(wire_ext_cmp, crate::extended::compare::<false>);
extended!(wire_ext_ucmp, crate::extended::compare::<true>);
extended!(wire_ext_cmpi, crate::extended::compare_flags::<false>);
extended!(wire_ext_ucmpi, crate::extended::compare_flags::<true>);
extended!(wire_ext_round, |cw, sw, a, _| {
    crate::extended::round_integral(cw, sw, a)
});
extended!(wire_ext_rem, crate::extended::remainder::<false>);
extended!(wire_ext_rem1, crate::extended::remainder::<true>);
extended!(wire_ext_scale, crate::extended::scale);
extended!(wire_ext_bcd_load, |cw, sw, _, b| crate::extended::load_bcd(
    cw, sw, b
));
extended!(wire_ext_bcd_store, |cw, sw, a, _| {
    crate::extended::store_bcd(cw, sw, a)
});
extended!(wire_ext_exp, |cw, sw, a, _| crate::extended::exp2m1(
    cw, sw, a
));
extended!(wire_ext_sin, |cw, sw, a, _| crate::extended::sin(cw, sw, a));
extended!(wire_ext_cos, |cw, sw, a, _| crate::extended::cos(cw, sw, a));
extended!(wire_ext_log, crate::extended::log_multiply::<false>);
extended!(wire_ext_log1p, crate::extended::log_multiply::<true>);
extended!(wire_ext_atan, crate::extended::atan);
macro_rules! paired {
    ($name:ident,$operation:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            cw: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            sw: u32,
        ) -> u32 {
            let a = u128::from(a0) | (u128::from(a1) << 64);
            let b = u128::from(b0) | (u128::from(b1) << 64);
            let result = ($operation)(cw, sw, a, b);
            with(owner, 0, |host| {
                host.paired = result;
                (&raw const host.paired) as u32
            })
        }
    };
}
paired!(wire_ext_extract, |cw, sw, a, _| crate::extended::extract(
    cw, sw, a
));
paired!(wire_ext_sincos, |cw, sw, a, _| crate::extended::sincos(
    cw, sw, a
));
paired!(wire_ext_tan, |cw, sw, a, _| crate::extended::tan(cw, sw, a));
extended!(wire_ext_cmp32, |cw, sw, a, b| {
    crate::extended::compare_memory::<4, false>(cw, sw, a, b as u64)
});
extended!(wire_ext_cmp64, |cw, sw, a, b| {
    crate::extended::compare_memory::<8, false>(cw, sw, a, b as u64)
});
extended!(wire_ext_cmpi16, |cw, sw, a, b| {
    crate::extended::compare_memory::<2, true>(cw, sw, a, b as u64)
});
extended!(wire_ext_cmpi32, |cw, sw, a, b| {
    crate::extended::compare_memory::<4, true>(cw, sw, a, b as u64)
});

macro_rules! floating_arithmetic {
    ($name:ident,$format:ty,$width:literal,$op:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::map::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    |state, a, b, exception| state.simd_arithmetic::<$format>($op, a, b, exception),
                ),
            )
        }
    };
}
floating_arithmetic!(wire_f32_add, Single, 4, iced_x86::Mnemonic::Addss);
floating_arithmetic!(wire_f64_add, Double, 8, iced_x86::Mnemonic::Addsd);
floating_arithmetic!(wire_f32_sub, Single, 4, iced_x86::Mnemonic::Subss);
floating_arithmetic!(wire_f64_sub, Double, 8, iced_x86::Mnemonic::Subsd);
floating_arithmetic!(wire_f32_mul, Single, 4, iced_x86::Mnemonic::Mulss);
floating_arithmetic!(wire_f64_mul, Double, 8, iced_x86::Mnemonic::Mulsd);
floating_arithmetic!(wire_f32_div, Single, 4, iced_x86::Mnemonic::Divss);
floating_arithmetic!(wire_f64_div, Double, 8, iced_x86::Mnemonic::Divsd);
floating_arithmetic!(wire_f32_min, Single, 4, iced_x86::Mnemonic::Minss);
floating_arithmetic!(wire_f64_min, Double, 8, iced_x86::Mnemonic::Minsd);
floating_arithmetic!(wire_f32_max, Single, 4, iced_x86::Mnemonic::Maxss);
floating_arithmetic!(wire_f64_max, Double, 8, iced_x86::Mnemonic::Maxsd);

macro_rules! floating_horizontal {
    ($name:ident,$format:ty,$width:literal,$op:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            _count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::horizontal::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    |state, a, b, exception| state.simd_arithmetic::<$format>($op, a, b, exception),
                ),
            )
        }
    };
}
floating_horizontal!(wire_f32_hadd, Single, 4, iced_x86::Mnemonic::Addss);
floating_horizontal!(wire_f64_hadd, Double, 8, iced_x86::Mnemonic::Addsd);
floating_horizontal!(wire_f32_hsub, Single, 4, iced_x86::Mnemonic::Subss);
floating_horizontal!(wire_f64_hsub, Double, 8, iced_x86::Mnemonic::Subsd);

macro_rules! floating_alternating {
    ($name:ident,$format:ty,$width:literal,$add:expr,$sub:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            let mut even = false;
            floating_result(
                owner,
                crate::floating::map::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    |state, a, b, exception| {
                        even = !even;
                        state.simd_arithmetic::<$format>(
                            if even { $sub } else { $add },
                            a,
                            b,
                            exception,
                        )
                    },
                ),
            )
        }
    };
}
floating_alternating!(
    wire_f32_addsub,
    Single,
    4,
    iced_x86::Mnemonic::Addss,
    iced_x86::Mnemonic::Subss
);
floating_alternating!(
    wire_f64_addsub,
    Double,
    8,
    iced_x86::Mnemonic::Addsd,
    iced_x86::Mnemonic::Subsd
);

macro_rules! floating_round {
    ($name:ident,$format:ty,$width:literal) => {
        #[no_mangle]
        #[allow(clippy::too_many_arguments)]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
            control: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::round::<$format, $width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    control,
                ),
            )
        }
    };
}
floating_round!(wire_f32_round, Single, 4);
floating_round!(wire_f64_round, Double, 8);

macro_rules! floating_dot {
    ($name:ident,$format:ty,$width:literal) => {
        #[no_mangle]
        #[allow(clippy::too_many_arguments)]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            _count: u32,
            control: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::dot::<$format, $width>(mxcsr, [a0, a1], [b0, b1], control),
            )
        }
    };
}
floating_dot!(wire_f32_dot, Single, 4);
floating_dot!(wire_f64_dot, Double, 8);

macro_rules! floating_sqrt {
    ($name:ident,$format:ty,$width:literal,$fraction:literal,$bias:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::map::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    |state, _a, b, exception| {
                        state.simd_sqrt::<$format>(b, $fraction, $bias, exception)
                    },
                ),
            )
        }
    };
}
floating_sqrt!(wire_f32_sqrt, Single, 4, 23, 127);
floating_sqrt!(wire_f64_sqrt, Double, 8, 52, 1023);

macro_rules! floating_predicate {
    ($name:ident,$format:ty,$width:literal) => {
        #[no_mangle]
        #[allow(clippy::too_many_arguments)]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
            predicate: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::map::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    |state, a, b, exception| {
                        state
                            .simd_predicate::<$format>(a, b, predicate as u8, exception)
                            .map(|value| if value { u64::MAX } else { 0 })
                    },
                ),
            )
        }
    };
}
floating_predicate!(wire_f32_predicate, Single, 4);
floating_predicate!(wire_f64_predicate, Double, 8);

macro_rules! floating_compare {
    ($name:ident,$format:ty,$width:literal,$op:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::map::<$width>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    |state, a, b, exception| {
                        state
                            .simd_compare::<$format>($op, a, b, exception)
                            .map(|()| a)
                    },
                ),
            )
        }
    };
}
floating_compare!(wire_f32_comi, Single, 4, iced_x86::Mnemonic::Comiss);
floating_compare!(wire_f64_comi, Double, 8, iced_x86::Mnemonic::Comisd);
floating_compare!(wire_f32_ucomi, Single, 4, iced_x86::Mnemonic::Ucomiss);
floating_compare!(wire_f64_ucomi, Double, 8, iced_x86::Mnemonic::Ucomisd);

macro_rules! floating_convert {
    ($name:ident,$from:ty,$to:ty,$source:literal,$dest:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::convert::<$from, $to, $source, $dest>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                ),
            )
        }
    };
}
floating_convert!(wire_f32_f64, Single, Double, 4, 8);
floating_convert!(wire_f64_f32, Double, Single, 8, 4);

macro_rules! integer_to_floating {
    ($name:ident,$to:ty,$dest:literal,$scalar:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            a0: u64,
            a1: u64,
            b0: u64,
            b1: u64,
            count: u32,
        ) -> u32 {
            floating_result(
                owner,
                crate::floating::integer_to_float::<$to, $dest>(
                    mxcsr,
                    [a0, a1],
                    [b0, b1],
                    count,
                    $scalar,
                ),
            )
        }
    };
}
integer_to_floating!(wire_i64_f32, Single, 4, true);
integer_to_floating!(wire_i64_f64, Double, 8, true);
integer_to_floating!(wire_i32x_f32, Single, 4, false);
integer_to_floating!(wire_i32x_f64, Double, 8, false);

macro_rules! floating_to_integer {
    ($name:ident,$from:ty,$source:literal,$truncate:literal,$scalar:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(
            owner: u32,
            mxcsr: u32,
            _a0: u64,
            _a1: u64,
            b0: u64,
            b1: u64,
            shape: u32,
        ) -> u32 {
            let (count, bits) = if $scalar { (1, shape) } else { (shape, 32) };
            floating_result(
                owner,
                crate::floating::float_to_integer::<$from, $source, $truncate>(
                    mxcsr,
                    [b0, b1],
                    count,
                    bits,
                    $scalar,
                ),
            )
        }
    };
}
floating_to_integer!(wire_f32_integer, Single, 4, false, true);
floating_to_integer!(wire_f64_integer, Double, 8, false, true);
floating_to_integer!(wire_f32_truncate, Single, 4, true, true);
floating_to_integer!(wire_f64_truncate, Double, 8, true, true);
floating_to_integer!(wire_f32_i32x, Single, 4, false, false);
floating_to_integer!(wire_f64_i32x, Double, 8, false, false);
floating_to_integer!(wire_f32_i32xt, Single, 4, true, false);
floating_to_integer!(wire_f64_i32xt, Double, 8, true, false);

#[no_mangle]
pub extern "C" fn wire_input(handle: u32, length: u32) -> u32 {
    with(handle, 0, |host| {
        if length > 256 * 1024 * 1024 {
            host.error = "host input exceeds mapping capacity".into();
            return 0;
        }
        host.input.resize(length as usize, 0);
        host.input.as_mut_ptr() as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_map(handle: u32, address: u64, length: u64, permissions: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        let result = u8::try_from(permissions)
            .map_err(|_| "invalid guest permissions".into())
            .and_then(|permissions| host.machine.memory.map(address, length, permissions));
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_write(handle: u32, address: u64) -> u32 {
    with(handle, u32::MAX, |host| {
        let result = host.machine.memory.write(address, &host.input);
        host.input.clear();
        match result {
            Ok(()) => 0,
            Err(error) => {
                host.error = error.to_string();
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_compile(handle: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        host.error.clear();
        match host.machine.prepare() {
            Ok(_) => 0,
            Err(error) => {
                let status = match error {
                    PrepareError::Compile(CompileError::Fetch(_)) => 1,
                    PrepareError::Compile(CompileError::InvalidInstruction(_)) => 2,
                    PrepareError::Cache(_) => 3,
                    PrepareError::ProcessExited => 4,
                };
                host.error = error.to_string();
                status
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_ticket(handle: u32) -> u64 {
    with(handle, 0, |host| host.machine.current().unwrap_or(0))
}

#[no_mangle]
pub extern "C" fn wire_fetch_fault(handle: u32) -> u32 {
    with(handle, 0, |host| {
        u32::from(host.machine.resolve_fetch_fault())
    })
}

// Tickets identify immutable modules, including across handle reuse. Cached
// modules remain independently valid after another module is prepared.
#[no_mangle]
pub extern "C" fn wire_ready(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| u32::from(host.machine.ready(ticket)))
}

#[no_mangle]
pub extern "C" fn wire_module(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .code(ticket)
            .map_or(0, |code| code.bytes.as_ptr() as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_module_len(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .code(ticket)
            .map_or(0, |code| code.bytes.len() as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_live(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        let machine = &mut host.machine;
        u32::from(machine.code_cache.live(&machine.memory, ticket))
    })
}

#[no_mangle]
pub extern "C" fn wire_pin(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        let machine = &mut host.machine;
        u32::from(machine.code_cache.pin(&machine.memory, ticket))
    })
}

#[no_mangle]
pub extern "C" fn wire_enter(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        let machine = &mut host.machine;
        u32::from(machine.ready(ticket) && machine.code_cache.pin(&machine.memory, ticket))
    })
}

#[no_mangle]
pub extern "C" fn wire_release(handle: u32, ticket: u64) {
    let Some(index) = handle.checked_sub(1) else {
        return;
    };
    MACHINES.with(|machines| {
        if let Some(slot) = machines.borrow_mut().get_mut(index as usize) {
            if let Some(host) = slot {
                let machine = &mut host.machine;
                machine.code_cache.release(&machine.memory, ticket);
                if host.closed && !machine.has_activity() {
                    *slot = None;
                }
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn wire_publish(handle: u32, ticket: u64, slot: u32) -> u32 {
    with(handle, 0, |host| {
        let machine = &mut host.machine;
        u32::from(machine.code_cache.publish(&machine.memory, ticket, slot))
    })
}

#[no_mangle]
pub extern "C" fn wire_begin(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| match host.machine.begin_run(ticket) {
        Ok(ready) => u32::from(ready),
        Err(error) => {
            host.error = error.to_string();
            3
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_links(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.code_cache.links() as *const _ as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_link(handle: u32, pc: u64) -> u32 {
    with(handle, 0, |host| host.machine.link(pc))
}

#[no_mangle]
pub extern "C" fn wire_end(handle: u32) {
    let Some(index) = handle.checked_sub(1) else {
        return;
    };
    MACHINES.with(|machines| {
        if let Some(slot) = machines.borrow_mut().get_mut(index as usize) {
            if let Some(host) = slot {
                host.machine.end_run();
                if host.closed && !host.machine.has_activity() {
                    *slot = None;
                }
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn wire_code_epoch(handle: u32) -> u64 {
    with(handle, 0, |host| host.machine.code_cache.epoch())
}

#[no_mangle]
pub extern "C" fn wire_code_retired(handle: u32) -> u32 {
    with(handle, 0, |host| {
        let machine = &mut host.machine;
        machine.code_cache.take_retired(&machine.memory).as_ptr() as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_code_used(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.code_cache.charged_bytes() as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_code_count(handle: u32) -> u32 {
    with(handle, 0, |host| host.machine.code_cache.len() as u32)
}

#[no_mangle]
pub extern "C" fn wire_code_charge(handle: u32, ticket: u64) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .code(ticket)
            .map_or(0, |code| code.storage_charge() as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_code_limit(handle: u32, bytes: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        match host.machine.code_cache.set_limit(bytes as usize) {
            Ok(()) => 0,
            Err(error) => {
                host.error = error.to_string();
                3
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_error(handle: u32) -> u32 {
    with(handle, 0, |host| host.error.as_ptr() as u32)
}

#[no_mangle]
pub extern "C" fn wire_error_len(handle: u32) -> u32 {
    with(handle, 0, |host| host.error.len() as u32)
}

#[no_mangle]
pub extern "C" fn wire_cache(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine.memory.translations() as *const _ as u32
    })
}

#[no_mangle]
pub extern "C" fn wire_resolve(handle: u32, address: u64, size: u32, access: u32) -> u32 {
    with(handle, 0, |host| {
        let memory = &host.machine.memory;
        memory.clear_fault();
        match memory.resolve(address, size as usize, access as u8) {
            Ok(pointer) => pointer as u32,
            Err(fault) => {
                memory.record_fault(fault);
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_probe(handle: u32, address: u64, size: u32, access: u32) -> u32 {
    with(handle, 1, |host| {
        host.machine
            .memory
            .probe_checked(address, size as usize, access as u8)
    })
}

#[no_mangle]
pub extern "C" fn wire_load(handle: u32, address: u64, size: u32, access: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine
            .memory
            .load_checked(address, size as usize, access as u8)
    })
}

#[no_mangle]
pub extern "C" fn wire_store(handle: u32, address: u64, size: u32, value: u64) -> u32 {
    with(handle, 1, |host| {
        host.machine
            .memory
            .store_checked(address, size as usize, value)
    })
}

#[no_mangle]
pub extern "C" fn wire_store128(handle: u32, address: u64, low: u64, high: u64) -> u32 {
    with(handle, 1, |host| {
        host.machine.memory.store128_checked(address, low, high)
    })
}

fn fp_length(address: u64, length: u32, access: u8) -> Result<usize, crate::memory::Fault> {
    if (1..=512).contains(&length) && matches!(access, crate::memory::READ | crate::memory::WRITE) {
        Ok(length as usize)
    } else {
        Err(crate::memory::Fault {
            address,
            length: length as usize,
            access,
            kind: crate::memory::FaultKind::Width,
        })
    }
}

#[no_mangle]
pub extern "C" fn wire_fp_area(handle: u32, address: u64, length: u32, access: u32) -> u32 {
    with(handle, 0, |host| {
        use crate::memory::{READ, WRITE};
        let memory = &host.machine.memory;
        memory.clear_fault();
        let result = fp_length(address, length, access as u8).and_then(|length| {
            if access == READ as u32 {
                memory.read(address, &mut host.fp_image[..length])
            } else {
                memory.check(address, length, WRITE)?;
                host.fp_image[..length].fill(0);
                Ok(())
            }
        });
        match result {
            Ok(()) => host.fp_image.as_mut_ptr() as u32,
            Err(fault) => {
                memory.record_fault(fault);
                0
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_fp_commit(handle: u32, address: u64, length: u32) -> u32 {
    with(handle, 1, |host| {
        match fp_length(address, length, crate::memory::WRITE) {
            Ok(length) => host
                .machine
                .memory
                .store_bytes_checked(address, &host.fp_image[..length]),
            Err(fault) => {
                host.machine.memory.record_fault(fault);
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_store_masked(
    handle: u32,
    address: u64,
    low: u64,
    high: u64,
    mask: u32,
) -> u32 {
    with(handle, 1, |host| {
        host.machine
            .memory
            .store_masked_checked(address, low, high, mask as u16)
    })
}

#[no_mangle]
pub extern "C" fn wire_fault_kind(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .memory
            .fault()
            .map_or(0, |fault| fault.kind as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_fault_address(handle: u32) -> u64 {
    with(handle, 0, |host| {
        host.machine.memory.fault().map_or(0, |fault| fault.address)
    })
}

#[no_mangle]
pub extern "C" fn wire_fault_access(handle: u32) -> u32 {
    with(handle, 0, |host| {
        host.machine
            .memory
            .fault()
            .map_or(0, |fault| fault.access as u32)
    })
}

#[no_mangle]
pub extern "C" fn wire_protect(handle: u32, address: u64, size: u64, protection: u32) -> u32 {
    with(handle, u32::MAX, |host| {
        match host
            .machine
            .memory
            .virtual_protect(address, size, protection)
        {
            Ok(_) => 0,
            Err(error) => error,
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_unmap(handle: u32, address: u64, size: u64) -> u32 {
    with(handle, u32::MAX, |host| {
        match host.machine.memory.unmap(address, size) {
            Ok(()) => 0,
            Err(error) => {
                host.error = error;
                1
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn wire_address_fault(handle: u32, address: u64) {
    with(handle, (), |host| {
        host.machine.memory.record_fault(crate::memory::Fault {
            address,
            length: 0,
            access: crate::memory::EXECUTE,
            kind: crate::memory::FaultKind::Address,
        })
    });
}
