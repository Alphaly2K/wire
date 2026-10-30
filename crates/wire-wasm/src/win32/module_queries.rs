use super::{
    call::Call, catalog::ModuleQueryApi as Q, context, module_query_values as values, ServiceError,
    Services,
};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
    pe::linker::Phase,
    process::{current, Loaded},
};

fn query(
    id: Q,
    call: &Call,
    memory: &Memory,
    image: &Loaded,
    services: &Services,
) -> Result<values::Output, u32> {
    let a = &call.arguments;
    if memory.mode() == Mode::X64
        && a[1] == 0
        && matches!(id, Q::GetModuleFileNameExA | Q::GetModuleFileNameExW)
    {
        image.process.query(a[0])?;
        if image.process.target(a[0])?.id != image.process.id() {
            return Err(50);
        }
    } else {
        image
            .process
            .check_current(a[0], current::QUERY | current::VM_READ)?;
    }
    let modules: Vec<_> = image
        .modules
        .modules
        .get(&image.info[0])
        .into_iter()
        .chain(
            image
                .modules
                .modules
                .values()
                .filter(|m| m.base != image.info[0]),
        )
        .filter(|m| m.phase != Phase::Failed)
        .map(|m| values::Module {
            base: m.base,
            name: &m.name,
            path: Some(&m.path),
            image: Some((
                m.image.size,
                if m.image.entry_rva == 0 {
                    0
                } else {
                    m.base + u64::from(m.image.entry_rva)
                },
            )),
        })
        .chain(services.providers().map(|(base, name)| values::Module {
            base,
            name,
            path: None,
            image: None,
        }))
        .collect();
    values::prepare(id, a, memory.mode() as usize / 8, image.info[0], &modules)
}

pub(super) fn dispatch(
    id: Q,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
    services: &Services,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "module queries require a loaded image",
    ))?;
    match query(id, call, memory, image, services) {
        Ok(output) => {
            for (at, bytes) in output.writes.iter().rev() {
                memory.check(*at, bytes.len(), WRITE)?;
            }
            for (at, bytes) in output.writes {
                memory.write(at, &bytes)?;
            }
            Ok(output.value)
        }
        Err(code) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(code),
            )?;
            Ok(0)
        }
    }
}
