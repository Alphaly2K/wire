//! CRT globals have provider identity; argument and environment strings belong to the process.
use super::{crt, data_pages, ServiceError, Services};
use crate::{
    environment::WindowsVersion,
    memory::{Memory, PAGE, WRITE},
    process::Loaded,
};

const CELLS: &[&str] = &[
    "__argc",
    "_acmdln",
    "_wcmdln",
    "_fmode",
    "_commode",
    "_adjust_fdiv",
    "_timezone",
    "_daylight",
    "_winmajor",
    "_winminor",
    "_winver",
    "_osver",
    "__argv",
    "__wargv",
    "_environ",
    "_wenviron",
    "__initenv",
    "__winitenv",
    "_pgmptr",
    "_wpgmptr",
];
#[derive(Clone, Copy)]
pub(crate) struct Data {
    pub base: u64,
    initialized: bool,
    ready: bool,
}
impl Data {
    pub fn cell(self, name: &str) -> Option<u64> {
        CELLS
            .iter()
            .position(|&n| n == name)
            .map(|n| self.base + n as u64 * 4)
    }
}
pub(crate) fn data_export(name: &str) -> bool {
    CELLS[..12].contains(&name)
}
#[derive(Clone, Copy)]
struct Startup {
    argv: u64,
    wide_argv: u64,
    envp: u64,
    wide_envp: u64,
    program: u64,
    wide_program: u64,
}
pub(crate) struct State {
    arguments: Vec<Vec<u16>>,
    startup: Option<Startup>,
}
impl Default for State {
    fn default() -> Self {
        Self::new("")
    }
}
impl State {
    pub(crate) fn new(command: &str) -> Self {
        Self {
            arguments: crate::process::command_line::arguments_wide(
                &command.encode_utf16().collect::<Vec<_>>(),
            ),
            startup: None,
        }
    }
    pub fn argc(&self) -> u32 {
        self.arguments.len() as u32
    }
}
impl Services {
    pub(crate) fn bind_for_loader(
        &mut self,
        dll: &str,
        name: &str,
        memory: &mut Memory,
        existing: &Memory,
        blocked: &[(u64, u64)],
    ) -> Result<u64, String> {
        if memory.mode() == crate::machine::Mode::X86
            && crt::provider(dll).is_some()
            && data_export(name)
        {
            return Ok(self
                .ensure_crt_data(dll, memory, existing, blocked)?
                .cell(name)
                .unwrap());
        }
        self.bind(dll, name)
    }
    pub(super) fn ensure_crt_data(
        &mut self,
        dll: &str,
        memory: &mut Memory,
        existing: &Memory,
        blocked: &[(u64, u64)],
    ) -> Result<Data, String> {
        let dll = crt::provider(dll).ok_or("invalid CRT provider")?;
        if let Some(data) = self.crt_data.get(&dll) {
            return Ok(*data);
        }
        self.register_provider(&dll)?;
        let base =
            crate::process::allocate_process_data_avoiding(memory, existing, PAGE, &[], blocked)?;
        let data = Data {
            base,
            initialized: false,
            ready: false,
        };
        self.crt_data.insert(dll, data);
        Ok(data)
    }
    pub(crate) fn initialize_crt_data(
        &mut self,
        memory: &mut Memory,
        command: [u64; 2],
        argc: u32,
        version: WindowsVersion,
    ) {
        let [timezone, daylight] = self
            .time_zone
            .crt_values()
            .expect("validated timezone profile");
        for data in self.crt_data.values_mut().filter(|d| !d.initialized) {
            let initial = [
                u64::from(argc),
                command[0],
                command[1],
                0x4000,
                0,
                0,
                u64::from(timezone),
                u64::from(daylight),
                u64::from(version.major),
                u64::from(version.minor),
                u64::from((version.major << 8) | version.minor),
                u64::from(version.build),
            ];
            let bytes: Vec<_> = initial
                .iter()
                .flat_map(|&v| (v as u32).to_le_bytes())
                .collect();
            memory
                .write(data.base, &bytes)
                .expect("fresh staged CRT data");
            data.initialized = true;
        }
    }
    pub(crate) fn own_crt_data(&self, pages: &mut data_pages::Pages) {
        for data in self.crt_data.values() {
            pages.adopt(data.base, PAGE);
        }
    }
}
fn narrow(wide: &[u16]) -> Result<Vec<u8>, ServiceError> {
    let text = String::from_utf16(wide)
        .map_err(|_| ServiceError::Unavailable("invalid CRT Unicode argument"))?;
    let mut bytes = crate::codepage::encode(&text, 932)
        .map_err(|_| ServiceError::Unavailable("CRT argument conversion failed"))?
        .0;
    bytes.push(0);
    Ok(bytes)
}
fn wide(units: &[u16]) -> Vec<u8> {
    units
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn list(
    bytes: &mut Vec<u8>,
    pointers: &mut Vec<(usize, usize)>,
    strings: impl Iterator<Item = Vec<u8>>,
) -> usize {
    let strings: Vec<_> = strings.collect();
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    let table = bytes.len();
    bytes.resize(table + (strings.len() + 1) * 4, 0);
    for (n, text) in strings.into_iter().enumerate() {
        if !bytes.len().is_multiple_of(2) {
            bytes.push(0);
        }
        pointers.push((table + n * 4, bytes.len()));
        bytes.extend(text);
    }
    table
}
impl Loaded {
    pub(crate) fn initialize_crt_data(&self, services: &mut Services, memory: &mut Memory) {
        services.initialize_crt_data(
            memory,
            [self.info[6], self.info[7]],
            self.crt.startup.argc(),
            self.environment.profile.version(),
        );
    }
    pub(crate) fn crt_data(
        &mut self,
        services: &mut Services,
        memory: &mut Memory,
        dll: &str,
    ) -> Result<Data, ServiceError> {
        // A live service has no separate staging address space.
        let existing = Memory::new(memory.mode());
        let data = services
            .ensure_crt_data(dll, memory, &existing, &[])
            .map_err(|_| ServiceError::Unavailable("CRT data allocation failed"))?;
        self.initialize_crt_data(services, memory);
        services.own_crt_data(&mut self.data_pages);
        Ok(data)
    }
    pub(crate) fn crt_startup(
        &mut self,
        services: &mut Services,
        memory: &mut Memory,
        dll: &str,
    ) -> Result<Data, ServiceError> {
        let data = self.crt_data(services, memory, dll)?;
        if data.ready {
            return Ok(data);
        }
        memory.check(data.base + 48, 32, WRITE)?;
        let startup = if let Some(startup) = self.crt.startup.startup {
            startup
        } else {
            let arguments = &self.crt.startup.arguments;
            let block = self.environment.variables().wide_block();
            let units: Vec<_> = block
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect();
            let env: Vec<_> = units
                .split(|&u| u == 0)
                .filter(|s| !s.is_empty())
                .map(<[u16]>::to_vec)
                .collect();
            let narrow_args: Vec<_> = arguments
                .iter()
                .map(|a| narrow(a))
                .collect::<Result<_, _>>()?;
            let narrow_env: Vec<_> = env.iter().map(|a| narrow(a)).collect::<Result<_, _>>()?;
            let program: Vec<_> = self.search.executable.encode_utf16().collect();
            let mut bytes = Vec::new();
            let mut pointers = Vec::new();
            let argv = list(&mut bytes, &mut pointers, narrow_args.into_iter());
            let wide_argv = list(&mut bytes, &mut pointers, arguments.iter().map(|a| wide(a)));
            let envp = list(&mut bytes, &mut pointers, narrow_env.into_iter());
            let wide_envp = list(&mut bytes, &mut pointers, env.iter().map(|a| wide(a)));
            let program_at = bytes.len();
            bytes.extend(narrow(&program)?);
            if !bytes.len().is_multiple_of(2) {
                bytes.push(0);
            }
            let wide_program = bytes.len();
            bytes.extend(wide(&program));
            let base = self.data_pages.allocate(memory, bytes.len() as u64);
            if base == 0 {
                return Err(ServiceError::Unavailable("CRT startup allocation failed"));
            }
            for (at, offset) in pointers {
                bytes[at..at + 4].copy_from_slice(&((base + offset as u64) as u32).to_le_bytes());
            }
            memory.write(base, &bytes).expect("fresh CRT startup data");
            Startup {
                argv: base + argv as u64,
                wide_argv: base + wide_argv as u64,
                envp: base + envp as u64,
                wide_envp: base + wide_envp as u64,
                program: base + program_at as u64,
                wide_program: base + wide_program as u64,
            }
        };
        let values = [
            startup.argv,
            startup.wide_argv,
            startup.envp,
            startup.wide_envp,
            startup.envp,
            startup.wide_envp,
            startup.program,
            startup.wide_program,
        ];
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|&p| (p as u32).to_le_bytes())
            .collect();
        memory.write(data.base + 48, &bytes)?;
        self.crt.startup.startup = Some(startup);
        services
            .crt_data
            .get_mut(&crt::provider(dll).unwrap())
            .unwrap()
            .ready = true;
        Ok(data)
    }
}

pub(super) fn dispatch(
    name: &str,
    a: &[u64; 16],
    dll: &str,
    image: &mut Loaded,
    services: &mut Services,
    memory: &mut Memory,
) -> Result<Option<u64>, ServiceError> {
    if let Some(cell) = name.strip_prefix("__p_") {
        let data = if CELLS[12..].contains(&cell) {
            image.crt_startup(services, memory, dll)?
        } else {
            image.crt_data(services, memory, dll)?
        };
        return data
            .cell(cell)
            .map(Some)
            .ok_or(ServiceError::Unavailable("unknown CRT pointer accessor"));
    }
    Ok(Some(match name {
        "__getmainargs" => {
            if a[3] != 0 {
                return super::crt::failure(image, memory, 22, 22).map(Some);
            }
            if a[4] != 0 && memory.read_int(a[4], 4)? != 0 {
                return super::crt::failure(image, memory, 22, 22).map(Some);
            }
            for &p in &a[..3] {
                memory.check(p, 4, WRITE)?;
            }
            let data = image.crt_startup(services, memory, dll)?;
            let values = [
                memory.read_int(data.cell("__argc").unwrap(), 4)?,
                memory.read_int(data.cell("__argv").unwrap(), 4)?,
                memory.read_int(data.cell("_environ").unwrap(), 4)?,
            ];
            for (&p, v) in a[..3].iter().zip(values) {
                memory.write_int(p, 4, v)?;
            }
            0
        }
        "_initialize_narrow_environment" | "_initialize_wide_environment" => {
            image.crt_startup(services, memory, dll)?;
            0
        }
        "_configure_narrow_argv" | "_configure_wide_argv" => {
            if a[0] > 1 {
                return super::crt::failure(image, memory, 22, 22).map(Some);
            }
            let data = image.crt_startup(services, memory, dll)?;
            let argc = data.cell("__argc").unwrap();
            let pointer = data
                .cell(if name == "_configure_narrow_argv" {
                    "__argv"
                } else {
                    "__wargv"
                })
                .unwrap();
            memory.check(argc, 4, WRITE)?;
            memory.check(pointer, 4, WRITE)?;
            let startup = image.crt.startup.startup.unwrap();
            memory.write_int(
                argc,
                4,
                if a[0] == 0 {
                    0
                } else {
                    u64::from(image.crt.startup.argc())
                },
            )?;
            memory.write_int(
                pointer,
                4,
                if a[0] == 0 {
                    0
                } else if name == "_configure_narrow_argv" {
                    startup.argv
                } else {
                    startup.wide_argv
                },
            )?;
            0
        }
        "_set_fmode" => {
            if !matches!(a[0], 0x4000 | 0x8000 | 0x10000) {
                return super::crt::failure(image, memory, 22, 22).map(Some);
            }
            let data = image.crt_data(services, memory, dll)?;
            memory.write_int(data.cell("_fmode").unwrap(), 4, a[0])?;
            0
        }
        "__set_app_type" | "_set_app_type" => {
            if a[0] > 2 {
                return Err(ServiceError::Unavailable("invalid CRT application type"));
            }
            image.crt.app_type.insert(dll.to_owned(), a[0] as u32);
            0
        }
        "_set_new_mode" if a[0] == 0 => 0,
        "_set_new_mode" => {
            return Err(ServiceError::Unavailable(
                "CRT new-handler allocation mode is not implemented",
            ))
        }
        "_set_invalid_parameter_handler"
        | "__setusermatherr"
        | "_register_thread_local_exe_atexit_callback"
            if a[0] == 0 =>
        {
            0
        }
        "_set_invalid_parameter_handler"
        | "__setusermatherr"
        | "_register_thread_local_exe_atexit_callback" => {
            return Err(ServiceError::Unavailable(
                "this CRT callback protocol is not implemented",
            ))
        }
        "_crt_atexit" | "_crt_at_quick_exit" => {
            if a[0] == 0 {
                return super::crt::failure(image, memory, 22, u32::MAX).map(Some);
            }
            let total: usize = image
                .crt
                .normal
                .values()
                .chain(image.crt.quick.values())
                .map(Vec::len)
                .sum();
            if total >= 4096 {
                return super::crt::failure(image, memory, 12, u32::MAX).map(Some);
            }
            let handlers = if name == "_crt_atexit" {
                &mut image.crt.normal
            } else {
                &mut image.crt.quick
            };
            handlers.entry(dll.to_owned()).or_default().push(a[0]);
            0
        }
        "_initialize_onexit_table" => {
            if a[0] == 0 {
                return super::crt::failure(image, memory, 22, u32::MAX).map(Some);
            }
            memory.write(a[0], &[0; 12])?;
            0
        }
        "_register_onexit_function" => {
            return super::crt::failure(image, memory, 38, u32::MAX).map(Some)
        }
        "__acrt_iob_func" => return super::crt::failure(image, memory, 9, 0).map(Some),
        "setvbuf" => return super::crt::failure(image, memory, 9, u32::MAX).map(Some),
        "_crt_debugger_hook" => 0,
        "_lock" | "_unlock" => {
            if a[0] >= 128 {
                return Err(ServiceError::Unavailable("invalid CRT lock index"));
            }
            let key = (dll.to_owned(), a[0] as u32);
            let tid = image.threads.active;
            let &(owner, depth) = image.crt.locks.get(&key).unwrap_or(&(0, 0));
            if name == "_lock" {
                if depth != 0 && owner != tid {
                    return Err(ServiceError::Blocked);
                }
                image.crt.locks.insert(
                    key,
                    (
                        tid,
                        depth
                            .checked_add(1)
                            .ok_or(ServiceError::Unavailable("CRT lock recursion overflow"))?,
                    ),
                );
            } else {
                if owner != tid || depth == 0 {
                    return Err(ServiceError::Unavailable(
                        "CRT lock is not owned by this thread",
                    ));
                }
                if depth == 1 {
                    image.crt.locks.remove(&key);
                    image.threads.changed();
                } else {
                    image.crt.locks.insert(key, (tid, depth - 1));
                }
            }
            0
        }
        _ => return Ok(None),
    }))
}
