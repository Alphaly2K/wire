//! Deterministic Wasm execution fixtures for the explicit Node validation tool.
use std::{
    mem::{offset_of, size_of},
    path::Path,
};
use wire_wasm::{
    jit,
    machine::{Mode, State},
    memory::{Memory, EXECUTE, PAGE, PAGE_EXECUTE_READ, PAGE_GUARD, READ, WRITE},
};

fn fixture(directory: &Path, name: &str, mode: Mode, entry: u64, bytes: &[u8]) -> String {
    let mut memory = Memory::new(mode);
    let base = entry & !(PAGE - 1);
    memory.map(base, PAGE * 2, READ | WRITE | EXECUTE).unwrap();
    memory.write(base, &[0xcc; PAGE as usize * 2]).unwrap();
    memory.write(entry, bytes).unwrap();
    memory.protect(base, PAGE * 2, EXECUTE).unwrap();
    let compiled = jit::compile(&mut memory, entry).unwrap();
    assert!(compiled.valid(&memory));
    std::fs::write(directory.join(name), &compiled.bytes).unwrap();
    memory.protect(base, PAGE, READ | WRITE | EXECUTE).unwrap();
    assert!(!compiled.valid(&memory));
    format!(
        "{{\"name\":\"{name}\",\"mode\":{},\"entry\":\"{entry}\"}}",
        mode as u32
    )
}

fn main() {
    let directory = std::env::args_os().nth(1).expect("output directory");
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(directory.join("driver.wasm"), jit::resident_driver()).unwrap();
    let mut fixtures = Vec::new();
    for mode in [Mode::X86, Mode::X64] {
        for address_override in [false, true] {
            let bits = if address_override {
                if mode == Mode::X86 {
                    16
                } else {
                    32
                }
            } else {
                mode as u32
            };
            for (name, opcode) in [
                ("loop", 0xe2),
                ("loope", 0xe1),
                ("loopne", 0xe0),
                ("jcxz", 0xe3),
            ] {
                let mut bytes = Vec::new();
                if address_override {
                    bytes.push(0x67);
                }
                let length = bytes.len() + 2;
                bytes.extend_from_slice(&[
                    opcode,
                    if name == "jcxz" {
                        2
                    } else {
                        (256 - length) as u8
                    },
                ]);
                if name == "jcxz" {
                    bytes.extend_from_slice(&[0xb0, 1]);
                }
                bytes.push(0xcc);
                fixtures.push(fixture(
                    directory,
                    &format!("{name}-{bits}-{}", mode as u32),
                    mode,
                    0x10000,
                    &bytes,
                ));
            }
        }
        fixtures.push(fixture(
            directory,
            &format!("loopflags-{}", mode as u32),
            mode,
            0x10000,
            &[0x31, 0xc0, 0xe1, 0xfe, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("rotalias-{}", mode as u32),
            mode,
            0x10000,
            &[0xd3, 0xd1, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("doublealias-{}", mode as u32),
            mode,
            0x10000,
            &[0x0f, 0xad, 0xd1, 0xcc],
        ));
        for bits in [8, 16, 32, 64] {
            if mode == Mode::X86 && bits == 64 {
                continue;
            }
            for (name, opcode, operands) in [
                ("xadd", 0xc0, 0xd8),
                ("xaddalias", 0xc0, 0xc0),
                ("cmpxchg", 0xb0, 0xcb),
                ("cmpxchgalias", 0xb0, 0xd8),
            ] {
                let mut bytes = Vec::new();
                if bits == 16 {
                    bytes.push(0x66);
                }
                if bits == 64 {
                    bytes.push(0x48);
                }
                bytes.extend_from_slice(&[0x0f, opcode + u8::from(bits != 8), operands, 0xcc]);
                fixtures.push(fixture(
                    directory,
                    &format!("{name}-{bits}-{}", mode as u32),
                    mode,
                    0x10000,
                    &bytes,
                ));
            }
            for (name, selector) in [("mul", 4), ("imul", 5), ("div", 6), ("idiv", 7)] {
                let mut bytes = Vec::new();
                if bits == 16 {
                    bytes.push(0x66);
                }
                if bits == 64 {
                    bytes.push(0x48);
                }
                bytes.extend_from_slice(&[
                    if bits == 8 { 0xf6 } else { 0xf7 },
                    0xc3 | (selector << 3),
                    0xcc,
                ]);
                fixtures.push(fixture(
                    directory,
                    &format!("{name}-{bits}-{}", mode as u32),
                    mode,
                    0x10000,
                    &bytes,
                ));
            }
            if bits != 8 {
                for (name, opcode, immediate) in [
                    ("shld", 0xa5, false),
                    ("shrd", 0xad, false),
                    ("shldi", 0xa4, true),
                    ("shrdi", 0xac, true),
                ] {
                    let mut bytes = Vec::new();
                    if bits == 16 {
                        bytes.push(0x66);
                    }
                    if bits == 64 {
                        bytes.push(0x48);
                    }
                    bytes.extend_from_slice(&[0x0f, opcode, 0xd8]);
                    if immediate {
                        bytes.push(1);
                    }
                    bytes.push(0xcc);
                    fixtures.push(fixture(
                        directory,
                        &format!("{name}-{bits}-{}", mode as u32),
                        mode,
                        0x10000,
                        &bytes,
                    ));
                }
                for (name, opcode) in [
                    ("bsf", 0xbc),
                    ("bsr", 0xbd),
                    ("bt", 0xa3),
                    ("bts", 0xab),
                    ("btr", 0xb3),
                    ("btc", 0xbb),
                ] {
                    let mut bytes = Vec::new();
                    if bits == 16 {
                        bytes.push(0x66);
                    }
                    if bits == 64 {
                        bytes.push(0x48);
                    }
                    bytes.extend_from_slice(&[
                        0x0f,
                        opcode,
                        if name == "bsf" || name == "bsr" {
                            0xc3
                        } else {
                            0xd8
                        },
                        0xcc,
                    ]);
                    fixtures.push(fixture(
                        directory,
                        &format!("{name}-{bits}-{}", mode as u32),
                        mode,
                        0x10000,
                        &bytes,
                    ));
                }
                for (name, selector) in [("bti", 4), ("btsi", 5), ("btri", 6), ("btci", 7)] {
                    let mut bytes = Vec::new();
                    if bits == 16 {
                        bytes.push(0x66);
                    }
                    if bits == 64 {
                        bytes.push(0x48);
                    }
                    bytes.extend_from_slice(&[0x0f, 0xba, 0xc0 | (selector << 3), 255, 0xcc]);
                    fixtures.push(fixture(
                        directory,
                        &format!("{name}-{bits}-{}", mode as u32),
                        mode,
                        0x10000,
                        &bytes,
                    ));
                }
                if bits >= 32 {
                    let mut bytes = Vec::new();
                    if bits == 64 {
                        bytes.push(0x48);
                    }
                    bytes.extend_from_slice(&[0x0f, 0xc8, 0xcc]);
                    fixtures.push(fixture(
                        directory,
                        &format!("bswap-{bits}-{}", mode as u32),
                        mode,
                        0x10000,
                        &bytes,
                    ));
                }
                for name in ["imul2", "imul3", "imulimm"] {
                    let mut bytes = Vec::new();
                    if bits == 16 {
                        bytes.push(0x66);
                    }
                    if bits == 64 {
                        bytes.push(0x48);
                    }
                    match name {
                        "imul2" => bytes.extend_from_slice(&[0x0f, 0xaf, 0xc3]),
                        "imul3" => bytes.extend_from_slice(&[0x6b, 0xc3, 0xfe]),
                        _ => {
                            bytes.extend_from_slice(&[0x69, 0xc3, 0x80, 0xff]);
                            if bits != 16 {
                                bytes.extend_from_slice(&[0xff, 0xff]);
                            }
                        }
                    }
                    bytes.push(0xcc);
                    fixtures.push(fixture(
                        directory,
                        &format!("{name}-{bits}-{}", mode as u32),
                        mode,
                        0x10000,
                        &bytes,
                    ));
                }
            }
            for (name, selector) in [
                ("shl", 4),
                ("shr", 5),
                ("sar", 7),
                ("rol", 0),
                ("ror", 1),
                ("rcl", 2),
                ("rcr", 3),
            ] {
                let mut bytes = Vec::new();
                if bits == 16 {
                    bytes.push(0x66);
                }
                if bits == 64 {
                    bytes.push(0x48);
                }
                bytes.extend_from_slice(&[
                    if bits == 8 { 0xd2 } else { 0xd3 },
                    0xc0 | (selector << 3),
                    0xcc,
                ]);
                fixtures.push(fixture(
                    directory,
                    &format!("{name}-{bits}-{}", mode as u32),
                    mode,
                    0x10000,
                    &bytes,
                ));
            }
            for (name, byte, unary) in [
                ("add", 0x00, false),
                ("adc", 0x10, false),
                ("sub", 0x28, false),
                ("sbb", 0x18, false),
                ("cmp", 0x38, false),
                ("and", 0x20, false),
                ("or", 0x08, false),
                ("xor", 0x30, false),
                ("test", 0x84, false),
                ("neg", 0x18, true),
                ("not", 0x10, true),
                ("inc", 0x00, true),
                ("dec", 0x08, true),
            ] {
                let mut bytes = Vec::new();
                if bits == 16 {
                    bytes.push(0x66);
                }
                if bits == 64 {
                    bytes.push(0x48);
                }
                if unary {
                    bytes.push(if name == "inc" || name == "dec" {
                        if bits == 8 {
                            0xfe
                        } else {
                            0xff
                        }
                    } else if bits == 8 {
                        0xf6
                    } else {
                        0xf7
                    });
                    bytes.push(0xc0 | byte);
                } else {
                    bytes.push(byte + u8::from(bits != 8));
                    bytes.push(0xd8);
                }
                bytes.push(0xcc);
                let id = format!("{name}-{bits}-{}", mode as u32);
                fixtures.push(fixture(directory, &id, mode, 0x10000, &bytes));
            }
        }
        for (name, operand) in [("xaddlh", 0xe0), ("xaddhl", 0xc4), ("cmpxchgah", 0xdc)] {
            let opcode = if name == "cmpxchgah" { 0xb0 } else { 0xc0 };
            fixtures.push(fixture(
                directory,
                &format!("{name}-8-{}", mode as u32),
                mode,
                0x10000,
                &[0x0f, opcode, operand, 0xcc],
            ));
        }
        for (name, opcode) in [
            ("clc", 0xf8),
            ("stc", 0xf9),
            ("cmc", 0xf5),
            ("cld", 0xfc),
            ("std", 0xfd),
            ("lahf", 0x9f),
            ("sahf", 0x9e),
        ] {
            fixtures.push(fixture(
                directory,
                &format!("{name}-{}", mode as u32),
                mode,
                0x10000,
                &[opcode, 0xcc],
            ));
        }
        fixtures.push(fixture(
            directory,
            &format!("flagchain-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x00, 0xd8, 0xf8, 0xfe, 0xc0, 0xfd, 0x9f, 0xf5, 0x9e, 0xfc, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("scanflags-{}", mode as u32),
            mode,
            0x10000,
            &[0x00, 0xd8, 0x0f, 0xbc, 0xd1, 0x0f, 0x94, 0xc3, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("mulflags-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x00, 0xd8, 0x0f, 0xaf, 0xc1, 0x0f, 0x90, 0xc2, 0x0f, 0x94, 0xc1, 0xcc,
            ],
        ));
        for cc in 0..16 {
            // A control edge separates the producer and consumer, forcing the
            // recipe to survive a CFG join without architectural state stores.
            let id = format!("condition-{cc}-{}", mode as u32);
            fixtures.push(fixture(
                directory,
                &id,
                mode,
                0x10000,
                &[0x00, 0xd8, 0xeb, 0, 0x0f, 0x90 + cc, 0xc1, 0xcc],
            ));
            fixtures.push(fixture(
                directory,
                &format!("staticcc-{cc}-{}", mode as u32),
                mode,
                0x10000,
                &[0x00, 0xd8, 0x0f, 0x90 + cc, 0xc1, 0xcc],
            ));
            fixtures.push(fixture(
                directory,
                &format!("statecc-{cc}-{}", mode as u32),
                mode,
                0x10000,
                &[0x0f, 0x90 + cc, 0xc1, 0xcc],
            ));
        }
        fixtures.push(fixture(
            directory,
            &format!("sum-{}", mode as u32),
            mode,
            0x10000,
            &[
                0xb9, 100, 0, 0, 0, 0x31, 0xc0, 0x01, 0xc8, 0x83, 0xe9, 1, 0x75, 0xf9, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("nested-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x31, 0xc0, 0xb9, 3, 0, 0, 0, 0xba, 4, 0, 0, 0, 0x83, 0xc0, 1, 0x83, 0xea, 1, 0x75,
                0xf8, 0x83, 0xe9, 1, 0x75, 0xee, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("diamond-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x39, 0xd8, 0x7c, 7, 0xb9, 10, 0, 0, 0, 0xeb, 5, 0xb9, 20, 0, 0, 0, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("aliases-{}", mode as u32),
            mode,
            0x10000,
            &[0x88, 0xdc, 0x66, 0x89, 0xc1, 0x89, 0xca, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("cmov-{}", mode as u32),
            mode,
            0x10000,
            &[0x39, 0xd8, 0x0f, 0x4c, 0xcb, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("carry-{}", mode as u32),
            mode,
            0x10000,
            &[0x00, 0xd8, 0xfe, 0xc0, 0x0f, 0x92, 0xc1, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("shiftchain-{}", mode as u32),
            mode,
            0x10000,
            &[0x00, 0xd8, 0xeb, 0, 0xd2, 0xe0, 0x10, 0xd8, 0xcc],
        ));
        fixtures.push(fixture(
            directory,
            &format!("rotatechain-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x00, 0xd8, 0xd2, 0xd0, 0x0f, 0x90, 0xc3, 0x80, 0xd2, 0, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("doublechain-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x00, 0xd8, 0x0f, 0xad, 0xd8, 0x0f, 0x90, 0xc2, 0x0f, 0x92, 0xc3, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("carrychain-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x00, 0xd8, 0xeb, 0, 0x10, 0xd8, 0x18, 0xd8, 0x0f, 0x92, 0xc1, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("crossed-{}", mode as u32),
            mode,
            0x10000,
            &[
                0x31, 0xc0, 0xeb, 4, 0xff, 0xc0, 0xeb, 4, 0xff, 0xc3, 0x75, 0xf8, 0xff, 0xc9, 0x75,
                0xf8, 0xcc,
            ],
        ));
        fixtures.push(fixture(
            directory,
            &format!("inside-{}", mode as u32),
            mode,
            0x10002,
            &[0xeb, 0xfc],
        ));
    }
    fixtures.push(fixture(
        directory,
        "split",
        Mode::X64,
        0x10fff,
        &[0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8, 0xcc],
    ));
    fixtures.push(fixture(
        directory,
        "high",
        Mode::X64,
        0x100010000,
        &[0xb8, 7, 0, 0, 0, 0xeb, 0xf9],
    ));

    // Compilation must neither consume an execute guard nor publish a proof
    // for an instruction whose continuation page cannot be fetched.
    let mut memory = Memory::default();
    memory
        .map(0x10000, PAGE * 2, READ | WRITE | EXECUTE)
        .unwrap();
    memory
        .write(0x10fff, &[0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    memory
        .virtual_protect(0x11000, PAGE, PAGE_EXECUTE_READ | PAGE_GUARD)
        .unwrap();
    assert!(matches!(
        jit::compile(&mut memory, 0x10fff),
        Err(jit::CompileError::Fetch(_))
    ));
    assert_ne!(
        memory.virtual_query(0x11000).unwrap().protect & PAGE_GUARD,
        0
    );

    println!("{{\"size\":{},\"regs\":{},\"flags\":{},\"ip\":{},\"work\":{},\"retired\":{},\"exception\":{},\"fixtures\":[{}]}}",
        size_of::<State>(), offset_of!(State, regs), offset_of!(State, flags), offset_of!(State, ip), offset_of!(State, work), offset_of!(State, retired), offset_of!(State, exception), fixtures.join(","));
}
