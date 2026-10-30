//! Fixed-arena shader validation, WGSL generation and typed reflection.
//! DXBC decoding and legalization belong to the pinned upstream C++ compiler.
mod abi;
mod allocator;
mod link;
mod reflection;
pub use link::link_vertex;
use reflection::{io, reflected};
pub use reflection::{InternalBinding, Io, ResourceBinding, Translation};
const INPUT_LIMIT: usize = 16 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 20 * 1024 * 1024;
pub fn translate(source: &[u8], stage: u32) -> Result<Translation, String> {
    finish(&parse_module(source, stage)?, stage)
}
fn parse_module(source: &[u8], stage: u32) -> Result<naga::Module, String> {
    if source.len() < 20 || source.len() > INPUT_LIMIT || !source.len().is_multiple_of(4) {
        return Err("SPIR-V input capacity/alignment".into());
    }
    if u32::from_le_bytes(source[..4].try_into().unwrap()) != 0x07230203
        || u32::from_le_bytes(source[12..16].try_into().unwrap()) > 1_000_000
    {
        return Err("SPIR-V header/id capacity".into());
    }
    let expected = match stage {
        1 => naga::ShaderStage::Vertex,
        2 => naga::ShaderStage::Fragment,
        4 => naga::ShaderStage::Compute,
        _ => return Err("portable compiler stage".into()),
    };
    // Keep D3D's existing coordinate convention; GPU adapter/linker owns the
    // explicit draw contract. Never relax SPIR-V capability validation.
    let options = naga::front::spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: true,
        block_ctx_dump_prefix: None,
    };
    let mut module = naga::front::spv::parse_u8_slice(source, &options)
        .map_err(|e| format!("SPIR-V parse: {e:?}"))?;
    if module.entry_points.len() != 1 || module.entry_points[0].stage != expected {
        return Err("shader entry point/stage mismatch".into());
    }
    // WGSL has no write-only storage-buffer address space. Widen the
    // declaration without introducing any shader reads or changing stores.
    // Storage textures have their own supported access modes and stay intact.
    for (_, variable) in module.global_variables.iter_mut() {
        if let naga::AddressSpace::Storage { access } = &mut variable.space {
            if access.contains(naga::StorageAccess::STORE) {
                *access |= naga::StorageAccess::LOAD;
            }
        }
    }
    Ok(module)
}
fn validate_module(module: &naga::Module) -> Result<naga::valid::ModuleInfo, String> {
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(module)
    .map_err(|e| format!("WebGPU shader validation: {e:?}"))
}
fn finish(module: &naga::Module, stage: u32) -> Result<Translation, String> {
    let info = validate_module(module)?;
    let entry = &module.entry_points[0];
    let (bindings, internal_bindings) = reflected(module, stage)?;
    let mut input_interface = Vec::new();
    let mut output_interface = Vec::new();
    for argument in &entry.function.arguments {
        io(
            module,
            argument.ty,
            argument.binding.as_ref(),
            &mut input_interface,
        )?;
    }
    if let Some(result) = &entry.function.result {
        io(
            module,
            result.ty,
            result.binding.as_ref(),
            &mut output_interface,
        )?;
    }
    input_interface.sort_by_key(|v| v.location);
    output_interface.sort_by_key(|v| v.location);
    let inputs = input_interface.iter().map(|v| v.location).collect();
    let code =
        naga::back::wgsl::write_string(module, &info, naga::back::wgsl::WriterFlags::empty())
            .map_err(|e| format!("WGSL generation: {e:?}"))?;
    if code.len() > INPUT_LIMIT {
        return Err("WGSL output capacity".into());
    }
    Ok(Translation {
        code,
        entry_point: entry.name.clone(),
        stage,
        bindings,
        internal_bindings,
        inputs,
        input_interface,
        output_interface,
        workgroup_size: entry.workgroup_size,
    })
}

#[cfg(test)]
mod tests;
