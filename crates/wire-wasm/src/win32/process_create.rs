use super::{
    call::Call,
    context,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    environment::EnvironmentVariables,
    filesystem::Files,
    machine::{Machine, Mode, State},
    memory::{Memory, PAGE, WRITE},
    pe::{self, Image},
    process::{Launch, Loaded},
};

fn environment(memory: &Memory, pointer: u64, wide: bool) -> Result<EnvironmentVariables, Failure> {
    let mut variables = EnvironmentVariables::default();
    let mut offset = 0u64;
    loop {
        let mut words = Vec::new();
        loop {
            if offset >= 2 * 1024 * 1024 {
                return Err(Failure::Windows(8));
            }
            let at = pointer.checked_add(offset).ok_or(Failure::Windows(87))?;
            let value = memory.read_int(at, if wide { 2 } else { 1 })? as u16;
            offset += if wide { 2 } else { 1 };
            if value == 0 {
                break;
            }
            words.push(value);
        }
        if words.is_empty() {
            break;
        }
        let entry = if wide {
            String::from_utf16(&words).map_err(|_| Failure::Windows(1113))?
        } else {
            crate::codepage::decode(&words.iter().map(|&v| v as u8).collect::<Vec<_>>(), 932)
                .map_err(Failure::Windows)?
        };
        let at = entry[usize::from(entry.starts_with('='))..]
            .find('=')
            .map(|at| at + usize::from(entry.starts_with('=')))
            .ok_or(Failure::Windows(87))?;
        if at == 0 {
            return Err(Failure::Windows(87));
        }
        variables.set(entry[..at].into(), entry[at + 1..].into());
        if variables.len() > 1024 {
            return Err(Failure::Windows(8));
        }
    }
    Ok(variables)
}

fn executable(
    files: &Files,
    application: &str,
    image: &Loaded,
) -> Result<(String, std::rc::Rc<Vec<u8>>), Failure> {
    let mut application = application.to_owned();
    if !application
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .contains('.')
    {
        application.push_str(".exe");
    }
    if pe::search::qualified(&application) {
        let path = files.resolve(&application).map_err(Failure::Windows)?;
        let meta = files.metadata(&path).map_err(Failure::Windows)?;
        if meta.attributes & 0x10 != 0 {
            return Err(Failure::Windows(5));
        }
        return files.find(vec![path]).map_err(Failure::Windows);
    }
    let mut directories = vec![
        pe::search::parent(&image.search.executable).to_owned(),
        files.cwd().into(),
        image.environment.profile.system_directory().into(),
        image.environment.profile.windows_directory().into(),
    ];
    if let Some(path) = image.environment.get("PATH") {
        directories.extend(path.split(';').filter(|p| !p.is_empty()).map(str::to_owned));
    }
    let paths = directories
        .iter()
        .map(|dir| format!("{}\\{application}", dir.trim_end_matches('\\')))
        .collect();
    files
        .find(paths)
        .map_err(|code| Failure::Windows(if code == 126 { 2 } else { code }))
}

fn startup(
    child: &mut Machine,
    source_mode: Mode,
    source: &[u8],
    strings: &[Option<String>; 2],
    show: u16,
) -> Result<(), Failure> {
    let mode = child.memory.mode();
    let width = mode as usize / 8;
    let start = if width == 4 { 16 } else { 32 };
    let flags = if width == 4 { 44 } else { 60 };
    let source_start = if source_mode == Mode::X86 { 16 } else { 32 };
    let info = &mut child.image.as_mut().unwrap().startup_info;
    for record in info.iter_mut() {
        record[start..flags + 4].copy_from_slice(&source[source_start..source_start + 32]);
        record[flags + 4..flags + 6].copy_from_slice(&show.to_le_bytes());
    }
    for (n, string) in strings.iter().enumerate() {
        let Some(string) = string else {
            continue;
        };
        for wide in [false, true] {
            let bytes = if wide {
                string
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                let (mut bytes, _) =
                    crate::codepage::encode(string, 932).map_err(Failure::Windows)?;
                bytes.push(0);
                bytes
            };
            let base = child
                .memory
                .map_heap((bytes.len() as u64).div_ceil(PAGE) * PAGE, false)
                .map_err(Failure::Windows)?;
            child.memory.write(base, &bytes)?;
            let offset = width * (n + 2);
            child.image.as_mut().unwrap().startup_info[usize::from(wide)][offset..offset + width]
                .copy_from_slice(&base.to_le_bytes()[..width]);
        }
    }
    Ok(())
}

fn create(
    zone: &std::rc::Rc<super::timezone::Profile>,
    call: &Call,
    memory: &mut Memory,
    image: &mut Loaded,
    files: &mut Files,
) -> Result<u64, Failure> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    let creation = a[5] as u32;
    let inherit = a[4] as u32 != 0;
    let base_length = if width == 4 { 68 } else { 104 };
    let length = base_length + if creation & 0x80000 != 0 { width } else { 0 };
    let mut info = [0; 112];
    memory.read(a[8], &mut info[..length])?;
    if u32::from_le_bytes(info[..4].try_into().unwrap()) as usize != length {
        return Err(Failure::Windows(87));
    }
    memory.check(a[9], width * 2 + 8, WRITE)?;
    if creation & !(4 | 0x20 | 0x400 | 0x80000) != 0 || !image.mappings.is_empty() {
        return Err(Failure::Windows(50));
    }
    let inherit_process = super::files::security(memory, a[2])?;
    let inherit_thread = super::files::security(memory, a[3])?;
    let flags_at = if width == 4 { 44 } else { 60 };
    let flags = u32::from_le_bytes(info[flags_at..flags_at + 4].try_into().unwrap());
    if flags & 0x100 != 0 && !inherit {
        return Err(Failure::Windows(87));
    }
    if memory.read_int(a[8] + width as u64, width)? != 0
        || memory.read_int(a[8] + (if width == 4 { 50 } else { 66 }), 2)? != 0
        || memory.read_int(a[8] + (if width == 4 { 52 } else { 72 }), width)? != 0
    {
        return Err(Failure::Windows(50));
    }
    let selected = if creation & 0x80000 != 0 {
        let pointer = memory.read_int(a[8] + base_length as u64, width)?;
        super::process_options::inherited_handles(memory, pointer)?
    } else {
        None
    };
    if let Some(handles) = &selected {
        if !inherit {
            return Err(Failure::Windows(87));
        }
        for &handle in handles {
            if super::handles::flags(image, handle).is_none_or(|flags| flags & 1 == 0) {
                return Err(Failure::Windows(87));
            }
        }
    }
    let select = |handle: u64, flags: u32| {
        inherit
            && flags & 1 != 0
            && selected
                .as_ref()
                .is_none_or(|handles| handles.contains(&handle))
    };
    if image.threads.selects_handles(select)
        || image.objects.selects_handles(select)
        || image.tokens.selects_handles(select)
        || image.snapshots.selects_handles(select)
    {
        return Err(Failure::Windows(50));
    }
    let mut strings = [None, None];
    for (n, value) in strings.iter_mut().enumerate() {
        let pointer = memory.read_int(a[8] + (width * (n + 2)) as u64, width)?;
        if pointer != 0 {
            *value = Some(name(memory, pointer, true)?);
        }
    }
    if strings[0]
        .as_ref()
        .is_some_and(|desktop| !desktop.is_empty())
    {
        return Err(Failure::Windows(50));
    }
    let command = if a[1] != 0 {
        name(memory, a[1], true)?
    } else if a[0] != 0 {
        name(memory, a[0], true)?
    } else {
        return Err(Failure::Windows(87));
    };
    let application = if a[0] != 0 {
        name(memory, a[0], true)?
    } else {
        let value = command.trim_start();
        if let Some(value) = value.strip_prefix('"') {
            value
                .split_once('"')
                .ok_or(Failure::Windows(87))?
                .0
                .to_owned()
        } else {
            value
                .split_whitespace()
                .next()
                .ok_or(Failure::Windows(87))?
                .to_owned()
        }
    };
    if command.encode_utf16().count() >= 2048 {
        return Err(Failure::Windows(206));
    }
    let (path, bytes) = executable(files, &application, image)?;
    image
        .open_files
        .check_share(&path, 1, 5)
        .map_err(Failure::Windows)?;
    let mode = Image::parse(&bytes)
        .map_err(|_| Failure::Windows(193))?
        .mode;
    let mut shared = files.fork_process();
    if a[7] != 0 {
        shared
            .set_cwd(&name(memory, a[7], true)?)
            .map_err(Failure::Windows)?;
    }
    let variables = if a[6] == 0 {
        image.environment.variables()
    } else {
        environment(memory, a[6], creation & 0x400 != 0)?
    };
    image.process.capacity(2).map_err(Failure::Windows)?;
    let process = image.process.child(mode).map_err(Failure::Windows)?;
    let mut child = Box::new(Machine::new(mode, 0, 0));
    child.services.time_zone = zone.clone();
    child.module_files = shared.clone();
    child
        .load_pe_process(
            &bytes,
            Launch {
                base: 0,
                command: &command,
                executable: Some(&path),
                diagnostic_imports: image.diagnostic_imports,
                process,
            },
        )
        .map_err(|error| Failure::Windows(error.code))?;
    child.module_files = shared;
    let loaded = child.image.as_mut().unwrap();
    loaded.open_files = if inherit {
        image.open_files.fork_process(select)
    } else {
        crate::file_handles::Handles::for_files(&child.module_files)
    };
    loaded.pipes = image.pipes.fork_process(select);
    loaded.streams = image.streams.clone();
    loaded.input = image.input.clone();
    loaded.process.inherit_handles(&image.process, select);
    loaded.process.error_mode = image.process.error_mode;
    if inherit {
        loaded.standard = image.standard.fork_process(select);
    }
    loaded.environment.inherit(&mut child.memory, variables)?;
    let show = u16::from_le_bytes(info[flags_at + 4..flags_at + 6].try_into().unwrap());
    startup(&mut child, memory.mode(), &info, &strings, show)?;
    let loaded = child.image.as_mut().unwrap();
    let child_width = mode as usize / 8;
    let (params_at, std_at) = if mode == Mode::X86 {
        (0x10, 0x18)
    } else {
        (0x20, 0x20)
    };
    let params = child
        .memory
        .read_int(loaded.info[5] + params_at, child_width)?;
    for n in 0..3 {
        let handle = if flags & 0x100 != 0 {
            let at = base_length - width * (3 - n);
            let mut value = [0; 8];
            value[..width].copy_from_slice(&info[at..at + width]);
            let value = u64::from_le_bytes(value);
            if value == memory.mode().address(u64::MAX) {
                mode.address(u64::MAX)
            } else {
                mode.address(value)
            }
        } else if inherit {
            let handle = image.standard.redirect(n);
            if super::handles::flags(loaded, handle).is_some() {
                handle
            } else {
                0
            }
        } else {
            loaded.standard.redirect(n)
        };
        loaded.standard.set_redirect(n, handle);
        child.memory.write_int(
            params + std_at + (n * child_width) as u64,
            child_width,
            handle,
        )?;
        if flags & 0x100 != 0 {
            let at = if child_width == 4 { 56 } else { 80 };
            for record in &mut loaded.startup_info {
                record[at + n * child_width..at + (n + 1) * child_width]
                    .copy_from_slice(&handle.to_le_bytes()[..child_width]);
            }
        }
    }
    loaded
        .process
        .record
        .suspended
        .set(u32::from(creation & 4 != 0));
    loaded.threads.primary().unwrap().suspended = loaded.process.record.suspended.get();
    let pid = loaded.process.id();
    let tid = loaded.process.first_thread();
    let (process, thread) = image
        .process
        .publish_child(&loaded.process)
        .map_err(Failure::Windows)?;
    image
        .process
        .set_flags(process, 1, u32::from(inherit_process))
        .expect("new process handle");
    image
        .process
        .set_flags(thread, 1, u32::from(inherit_thread))
        .expect("new primary handle");
    let mut output = [0; 24];
    output[..width].copy_from_slice(&process.to_le_bytes()[..width]);
    output[width..width * 2].copy_from_slice(&thread.to_le_bytes()[..width]);
    output[width * 2..width * 2 + 4].copy_from_slice(&pid.to_le_bytes());
    output[width * 2 + 4..width * 2 + 8].copy_from_slice(&tid.to_le_bytes());
    memory
        .write(a[9], &output[..width * 2 + 8])
        .expect("preflighted process information");
    image.children.push_back(child);
    Ok(1)
}

pub(super) fn dispatch(
    zone: &std::rc::Rc<super::timezone::Profile>,
    call: Call,
    state: &mut State,
    memory: &mut Memory,
    image: &mut Loaded,
    files: &mut Files,
) -> Result<(), ServiceError> {
    let result = match create(zone, &call, memory, image, files) {
        Ok(value) => value,
        Err(Failure::Fault(fault)) => return Err(fault.into()),
        Err(Failure::Windows(code)) => {
            if let Some(error) = super::file_input::suspend(files, Some(image), &call, state.ip) {
                return Err(error);
            }
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            0
        }
    };
    call.complete(state, result);
    state.work = state.work.wrapping_add(1);
    Ok(())
}
