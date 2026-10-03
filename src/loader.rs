//! PE image loader: maps the executable into the guest address space,
//! resolves imports into API stubs, and sets up the initial thread context
//! (stack, TEB, PEB, command line).

use crate::api::ApiTable;
use crate::mem::{GuestMem, SPACE_SIZE};
use crate::pe::PeFile;
use std::path::Path;

pub const STACK_BASE: u32 = 0x0018_0000;
pub const STACK_SIZE: u32 = 0x0002_0000; // 128 KiB committed; growth is M2 scope
pub const TEB_ADDR: u32 = 0x7ffd_f000;
pub const PEB_ADDR: u32 = 0x7ffd_e000;
pub const CMDLINE_ADDR: u32 = 0x00fe_0000;

pub struct LoadedImage {
    pub pe: PeFile,
    pub entry: u32,
    pub image_base: u32,
    pub esp: u32,
    pub teb: u32,
    pub peb: u32,
    pub import_count: usize,
    pub cmdline_a: u32,
    pub cmdline_w: u32,
}

pub fn load_image(
    mem: &mut GuestMem,
    api: &mut ApiTable,
    path: &Path,
    display_name: &str,
) -> Result<LoadedImage, String> {
    let buf = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let pe = PeFile::parse(&buf)?;
    let base = pe.image_base;
    if base < 0x1_0000 || base as u64 + pe.size_of_image as u64 > SPACE_SIZE {
        return Err(format!(
            "unusual image layout: base {base:#x}, size {:#x}",
            pe.size_of_image
        ));
    }
    mem.commit(base, pe.size_of_image)
        .map_err(|e| format!("commit image: {e}"))?;
    mem.write_bytes(
        base,
        &buf[..pe.size_of_headers.min(buf.len() as u32) as usize],
    )
    .map_err(|e| format!("map headers: {e}"))?;
    for s in &pe.sections {
        if s.raw_size == 0 || s.raw_ptr == 0 {
            continue;
        }
        let start = s.raw_ptr as usize;
        if start >= buf.len() {
            continue;
        }
        let end = (start + s.raw_size as usize).min(buf.len());
        mem.write_bytes(base.wrapping_add(s.virtual_address), &buf[start..end])
            .map_err(|e| format!("map section {}: {e}", s.name))?;
    }

    // Relocations: we always map at the preferred base (we own the entire
    // guest address space), so no fixups are needed. DLL loading (M2+) will
    // need DIR_BASERELOC applied here.

    let imports = pe.imports(&buf)?;
    let mut import_count = 0usize;
    for d in &imports {
        for f in &d.functions {
            let stub = api.register(&d.dll, f.name.as_deref(), f.ordinal);
            mem.write_u32(base.wrapping_add(f.iat_rva), stub)
                .map_err(|e| format!("patch IAT: {e}"))?;
            import_count += 1;
        }
    }

    // Initial thread stack. Return address points at stub #0 (ExitEmulation).
    mem.commit(STACK_BASE, STACK_SIZE)
        .map_err(|e| format!("commit stack: {e}"))?;
    let esp = (STACK_BASE + STACK_SIZE - 64) & !15;
    let exit_stub = api.lookup("wire", "ExitEmulation").unwrap();
    mem.write_u32(esp, exit_stub).map_err(|e| e.to_string())?;

    // Minimal TEB/PEB. Only fields actually read by typical CRT startup code.
    mem.commit(TEB_ADDR, 0x1000).map_err(|e| e.to_string())?;
    mem.commit(PEB_ADDR, 0x1000).map_err(|e| e.to_string())?;
    mem.write_u32(TEB_ADDR, 0xffff_ffff)
        .map_err(|e| e.to_string())?; // SEH chain: empty
    mem.write_u32(TEB_ADDR + 0x04, STACK_BASE + STACK_SIZE)
        .map_err(|e| e.to_string())?; // StackBase
    mem.write_u32(TEB_ADDR + 0x08, STACK_BASE)
        .map_err(|e| e.to_string())?; // StackLimit
    mem.write_u32(TEB_ADDR + 0x18, TEB_ADDR)
        .map_err(|e| e.to_string())?; // Self
    mem.write_u32(TEB_ADDR + 0x20, 4242)
        .map_err(|e| e.to_string())?; // ProcessId
    mem.write_u32(TEB_ADDR + 0x24, 1)
        .map_err(|e| e.to_string())?; // ThreadId (main = 1)
    mem.write_u32(TEB_ADDR + 0x30, PEB_ADDR)
        .map_err(|e| e.to_string())?; // ProcessEnvironmentBlock
    mem.write_u32(PEB_ADDR + 0x08, base)
        .map_err(|e| e.to_string())?; // ImageBaseAddress

    // Command line: `"<display_name>" ` in narrow and wide form.
    mem.commit(CMDLINE_ADDR, 0x1000)
        .map_err(|e| e.to_string())?;
    let cmdline = format!("\"{display_name}\" ");
    let narrow = crate::codepage::encode(&cmdline, 932)
        .map_err(|e| format!("command-line encoding: {e}"))?
        .0;
    mem.write_bytes(CMDLINE_ADDR, &narrow)
        .map_err(|e| e.to_string())?;
    mem.write_u8(CMDLINE_ADDR + narrow.len() as u32, 0)
        .map_err(|e| e.to_string())?;
    let cmdline_w_addr = CMDLINE_ADDR + 0x800;
    let wide: Vec<u16> = cmdline.encode_utf16().collect();
    for (i, u) in wide.iter().enumerate() {
        mem.write_u16(cmdline_w_addr + 2 * i as u32, *u)
            .map_err(|e| e.to_string())?;
    }
    mem.write_u16(cmdline_w_addr + 2 * wide.len() as u32, 0)
        .map_err(|e| e.to_string())?;

    Ok(LoadedImage {
        entry: base.wrapping_add(pe.entry_rva),
        image_base: base,
        esp,
        teb: TEB_ADDR,
        peb: PEB_ADDR,
        import_count,
        cmdline_a: CMDLINE_ADDR,
        cmdline_w: cmdline_w_addr,
        pe,
    })
}
