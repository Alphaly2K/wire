use super::{
    call::Call,
    catalog::ShellApi,
    com, context,
    heaps::Error,
    modules::{self, Failure},
    ServiceError,
};
use crate::{
    filesystem::Files,
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

fn arguments(call: &Call, memory: &mut Memory, image: &mut Loaded) -> Result<u64, Error> {
    let a = &call.arguments;
    memory.check(a[1], 4, WRITE)?;
    let bytes = modules::string(memory, a[0], true, 32768).map_err(|failure| match failure {
        Failure::Windows(code) => Error::Windows(code),
        Failure::Fault(fault) => Error::Fault(fault),
    })?;
    let words = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&word| u16::from_le_bytes(word))
        .collect::<Vec<_>>();
    let args = if words.is_empty() {
        vec![image.search.executable.encode_utf16().collect()]
    } else {
        crate::process::command_line::arguments_wide(&words)
    };
    let width = memory.mode() as usize / 8;
    let mut payload = vec![0; (args.len() + 1) * width];
    let mut offsets = Vec::with_capacity(args.len());
    for arg in &args {
        offsets.push(payload.len());
        payload.extend(
            arg.iter()
                .copied()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes),
        );
    }
    let pointer = image
        .heaps
        .allocate_local(memory, payload.len() as u64, &payload)?;
    for (n, offset) in offsets.into_iter().enumerate() {
        memory
            .write_int(pointer + (n * width) as u64, width, pointer + offset as u64)
            .expect("preflighted argv allocation");
    }
    memory
        .write_int(a[1], 4, args.len() as u64)
        .expect("preflighted argument count");
    Ok(pointer)
}

fn known_folder(
    call: &Call,
    memory: &mut Memory,
    image: &mut Loaded,
    files: &mut Files,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    memory.check(a[3], width, WRITE)?;
    let mut guid = [0; 16];
    memory.read(a[0], &mut guid)?;
    let result = (|| -> Result<&'static str, u32> {
        if a[2] != 0 || a[1] & !0xc000 != 0 {
            return Err(50);
        }
        let path = image.environment.profile.known_folder(guid).ok_or(50u32)?;
        if a[1] & 0x8000 != 0 {
            files.create_directory_all(path)?;
        }
        if a[1] & 0x4000 == 0 && !files.metadata(path).is_ok_and(|m| m.attributes & 0x10 != 0) {
            return Err(3);
        }
        Ok(path)
    })();
    let (pointer, status) = match result {
        Err(code) => (0, 0x80070000 | u64::from(code)),
        Ok(path) => {
            let bytes: Vec<_> = path
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            let pointer = com::allocation(image.heaps.allocate_task(
                memory,
                bytes.len() as u64,
                Some(&bytes),
            ))?;
            (pointer, if pointer == 0 { 0x8007000e } else { 0 })
        }
    };
    memory.write_int(a[3], width, pointer)?;
    Ok(status)
}

fn folder_failure(
    id: ShellApi,
    output: u64,
    code: u32,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let unit = if id.wide() { 2 } else { 1 };
    if output != 0 {
        memory.check(output, unit, WRITE)?;
    }
    let error = context::error_address(state, memory.mode());
    if !id.hresult() {
        memory.check(error, 4, WRITE)?;
    }
    if output != 0 {
        memory.write(output, &[0; 2][..unit])?;
    }
    if id.hresult() {
        Ok(0x80070000 | u64::from(code))
    } else {
        memory.write_int(error, 4, code.into())?;
        Ok(0)
    }
}
fn folder(
    id: ShellApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &Loaded,
    files: &mut Files,
) -> Result<u64, ServiceError> {
    let a = call.arguments;
    let special = matches!(id, ShellApi::SpecialFolderA | ShellApi::SpecialFolderW);
    let (output, csidl) = if special {
        (a[1], a[2] as u32)
    } else {
        (a[4], a[1] as u32)
    };
    let path = (|| -> Result<&str, u32> {
        if csidl & !0xc0ff != 0 || !special && a[3] > 1 {
            return Err(87);
        }
        if !special && a[2] != 0 {
            return Err(50);
        }
        image.environment.profile.csidl(csidl & 0xff).ok_or(50)
    })();
    let path = match path {
        Ok(path) => path,
        Err(code) => return folder_failure(id, output, code, state, memory),
    };
    let mut bytes = if id.wide() {
        path.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        crate::codepage::encode(path, image.environment.profile.codepage())
            .map_err(|_| ServiceError::Unavailable("invalid shell profile encoding"))?
            .0
    };
    let unit = if id.wide() { 2 } else { 1 };
    bytes.resize(bytes.len() + unit, 0);
    if bytes.len() / unit > 260 {
        return folder_failure(id, output, 206, state, memory);
    }
    if output != 0 {
        memory.check(output, bytes.len(), WRITE)?;
    }
    let create = csidl & 0x8000 != 0 || special && a[3] != 0;
    let result = if create {
        files.create_directory_all(path)
    } else if csidl & 0x4000 != 0 || files.metadata(path).is_ok_and(|m| m.attributes & 0x10 != 0) {
        Ok(())
    } else {
        Err(3)
    };
    if let Err(code) = result {
        return folder_failure(id, output, code, state, memory);
    }
    if output != 0 {
        memory
            .write(output, &bytes)
            .expect("preflighted shell folder output");
    }
    Ok(u64::from(special))
}

pub(super) fn dispatch(
    id: ShellApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    files: &mut Files,
) -> Result<u64, ServiceError> {
    if matches!(id, ShellApi::ExecuteExA | ShellApi::ExecuteExW) {
        memory.write_int(context::error_address(state, memory.mode()), 4, 1155)?;
        return Ok(0);
    }
    if matches!(id, ShellApi::ExecuteA | ShellApi::ExecuteW) {
        memory.write_int(context::error_address(state, memory.mode()), 4, 1155)?;
        return Ok(31);
    }
    if matches!(id, ShellApi::FileOperationA | ShellApi::FileOperationW) {
        return Ok(120);
    }
    let image = image.ok_or(ServiceError::Unavailable(
        "shell allocation requires a loaded image",
    ))?;
    if id == ShellApi::KnownFolder {
        return known_folder(call, memory, image, files);
    }
    if id != ShellApi::Arguments {
        return folder(id, call, state, memory, image, files);
    }
    match arguments(call, memory, image) {
        Ok(pointer) => Ok(pointer),
        Err(Error::Fault(fault)) => Err(fault.into()),
        Err(Error::Backing) => Err(ServiceError::Unavailable(
            "argument allocation requires available process-heap backing",
        )),
        Err(Error::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
