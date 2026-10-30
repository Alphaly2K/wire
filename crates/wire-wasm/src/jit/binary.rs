//! Binary encoding for the generated module's small Wasm instruction vocabulary.
#[derive(Default)]
pub(super) struct Code {
    pub bytes: Vec<u8>,
    depth: u32,
}

#[derive(Clone, Copy)]
pub(super) struct Imports {
    pub memory: bool,
    pub masked: bool,
    pub fp_area: bool,
    pub numeric: bool,
    pub vectors: super::vector::Bank,
    pub floating: u64,
    pub extended: u128,
    pub crc: u8,
    pub clock: bool,
}

impl Imports {
    pub fn numeric_base(self) -> u32 {
        if self.memory {
            self.fp_area_base() + if self.fp_area { 2 } else { 0 }
        } else {
            0
        }
    }
    pub fn fp_area_base(self) -> u32 {
        7 + u32::from(self.masked)
    }
    pub fn function_count(self) -> u32 {
        self.flags_base() + 1
    }
    pub fn flags_base(self) -> u32 {
        self.clock_base() + u32::from(self.clock)
    }
    pub fn clock_base(self) -> u32 {
        self.crc_base() + self.crc.count_ones()
    }
    pub fn crc_base(self) -> u32 {
        self.extended_base() + self.extended.count_ones()
    }
    pub fn extended_base(self) -> u32 {
        self.floating_base() + self.floating.count_ones()
    }
    pub fn floating_base(self) -> u32 {
        self.numeric_base() + if self.numeric { 4 } else { 0 }
    }
}

pub(super) fn unsigned(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            break;
        }
    }
}

fn signed(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        let done = (value == 0 && byte & 64 == 0) || (value == -1 && byte & 64 != 0);
        out.push(byte | if done { 0 } else { 128 });
        if done {
            break;
        }
    }
}

pub(super) fn name(out: &mut Vec<u8>, value: &str) {
    unsigned(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

pub(super) fn section(out: &mut Vec<u8>, id: u8, bytes: &[u8]) {
    out.push(id);
    unsigned(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

pub(super) fn signature(out: &mut Vec<u8>, params: &[u8], results: &[u8]) {
    out.push(0x60);
    unsigned(out, params.len() as u32);
    out.extend_from_slice(params);
    unsigned(out, results.len() as u32);
    out.extend_from_slice(results);
}

impl Code {
    pub fn op(&mut self, opcode: u8) {
        self.bytes.push(opcode);
    }
    pub fn uint(&mut self, value: u32) {
        unsigned(&mut self.bytes, value);
    }
    pub fn i32(&mut self, value: u32) {
        self.op(0x41);
        signed(&mut self.bytes, value as i32 as i64);
    }
    pub fn i64(&mut self, value: u64) {
        self.op(0x42);
        signed(&mut self.bytes, value as i64);
    }
    pub fn get(&mut self, local: u32) {
        self.op(0x20);
        self.uint(local);
    }
    pub fn set(&mut self, local: u32) {
        self.op(0x21);
        self.uint(local);
    }
    pub fn global_get(&mut self, index: u32) {
        self.op(0x23);
        self.uint(index);
    }
    pub fn global_set(&mut self, index: u32) {
        self.op(0x24);
        self.uint(index);
    }
    pub fn simd(&mut self, opcode: u32) {
        self.op(0xfd);
        self.uint(opcode);
    }
    pub fn vector_const(&mut self, bytes: [u8; 16]) {
        self.simd(12);
        self.bytes.extend(bytes);
    }
    pub fn vector_memory(&mut self, opcode: u32, offset: usize) {
        self.simd(opcode);
        self.uint(0);
        self.uint(offset.try_into().expect("vector memory offset"));
    }
    pub fn load64(&mut self, offset: usize) {
        self.memory(0x29, offset);
    }
    pub fn store64(&mut self, offset: usize) {
        self.memory(0x37, offset);
    }
    pub fn memory(&mut self, op: u8, offset: usize) {
        self.op(op);
        self.uint(0);
        self.uint(offset.try_into().expect("state field offset"));
    }
    pub fn open(&mut self, kind: u8, result: u8) -> u32 {
        self.op(kind);
        self.op(result);
        self.depth += 1;
        self.depth
    }
    pub fn block(&mut self) -> u32 {
        self.open(0x02, 0x40)
    }
    pub fn if_void(&mut self) {
        self.open(0x04, 0x40);
    }
    pub fn if_i32(&mut self) {
        self.open(0x04, 0x7f);
    }
    pub fn otherwise(&mut self) {
        self.op(0x05);
    }
    pub fn end(&mut self) {
        self.op(0x0b);
        self.depth -= 1;
    }
    pub fn branch(&mut self, label: u32) {
        self.op(0x0c);
        self.uint(self.depth.checked_sub(label).expect("live branch label"));
    }
    pub fn table(&mut self, labels: &[u32]) {
        self.table_default(labels, *labels.last().expect("nonempty branch table"));
    }
    pub fn table_default(&mut self, labels: &[u32], default: u32) {
        self.op(0x0e);
        self.uint(labels.len() as u32);
        for label in labels.iter().copied().chain(Some(default)) {
            self.uint(self.depth.checked_sub(label).expect("live table label"));
        }
    }

    pub fn call(&mut self, function: u32) {
        self.op(0x10);
        self.uint(function);
    }

    pub fn indirect(&mut self, kind: u32) {
        self.op(0x11);
        self.uint(kind);
        self.uint(0);
    }

    pub fn function_body(mut self, locals: &[(u32, u8)]) -> Vec<u8> {
        assert_eq!(self.depth, 0);
        self.op(0x0b);
        let mut body = Vec::new();
        unsigned(&mut body, locals.len() as u32);
        for &(count, kind) in locals {
            unsigned(&mut body, count);
            body.push(kind);
        }
        body.extend(self.bytes);
        let mut encoded = Vec::new();
        unsigned(&mut encoded, body.len() as u32);
        encoded.extend(body);
        encoded
    }

    pub fn module(self, required: Imports, resolver: Self) -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        // Imported helpers operate on concrete memory accesses; none decode
        // guest instructions. State boundaries belong to the resident driver.
        let mut types = vec![16, 0x60, 4, 0x7f, 0x7e, 0x7f, 0x7f, 1, 0x7f];
        for result in [0x7f, 0x7e] {
            types.extend_from_slice(&[0x60, 4, 0x7f, 0x7e, 0x7f, 0x7f, 1, result]);
        }
        types.extend_from_slice(&[0x60, 4, 0x7f, 0x7e, 0x7f, 0x7e, 1, 0x7f]);
        types.extend_from_slice(&[0x60, 2, 0x7f, 0x7e, 0]);
        types.extend_from_slice(&[0x60, 1, 0x7e, 1, 0x7f]);
        super::state::signature(&mut types);
        signature(&mut types, &[0x7e, 0x7e], &[0x7e]);
        signature(&mut types, &[0x7f, 0x7e, 0x7e, 0x7e], &[0x7f]);
        signature(
            &mut types,
            &[0x7f, 0x7f, 0x7e, 0x7e, 0x7e, 0x7e, 0x7f],
            &[0x7f],
        );
        signature(
            &mut types,
            &[0x7f, 0x7f, 0x7e, 0x7e, 0x7e, 0x7e, 0x7f, 0x7f],
            &[0x7f],
        );
        signature(&mut types, &[0x7f, 0x7e, 0x7e, 0x7e, 0x7f], &[0x7f]);
        signature(&mut types, &[0x7f, 0x7e, 0x7f], &[0x7f]);
        signature(&mut types, &[], &[0x7e, 0x7f]);
        signature(&mut types, &[0x7f, 0x7e], &[0x7f]);
        super::flags::signature(&mut types);
        section(&mut module, 1, &types);
        let mut imports = Vec::new();
        unsigned(
            &mut imports,
            1 + required.function_count()
                + if required.vectors.active() {
                    u32::from(super::vector::bank::GLOBALS)
                } else {
                    0
                },
        );
        name(&mut imports, "core");
        name(&mut imports, "memory");
        imports.extend_from_slice(&[2, 0, 1]);
        if required.memory {
            for (index, function) in [
                "wire_resolve",
                "wire_load",
                "wire_store",
                "wire_address_fault",
            ]
            .into_iter()
            .enumerate()
            {
                name(&mut imports, "core");
                name(&mut imports, function);
                imports.extend_from_slice(&[0, index as u8 + 1]);
            }
            name(&mut imports, "core");
            name(&mut imports, "wire_store128");
            imports.extend_from_slice(&[0, 8]);
            name(&mut imports, "core");
            name(&mut imports, "wire_probe");
            imports.extend_from_slice(&[0, 1]);
            name(&mut imports, "jit");
            name(&mut imports, "lookup");
            imports.extend_from_slice(&[0, 1]);
            if required.masked {
                name(&mut imports, "core");
                name(&mut imports, "wire_store_masked");
                imports.extend_from_slice(&[0, 11]);
            }
            if required.fp_area {
                for (function, kind) in [("wire_fp_area", 1), ("wire_fp_commit", 12)] {
                    name(&mut imports, "core");
                    name(&mut imports, function);
                    imports.extend_from_slice(&[0, kind]);
                }
            }
        }
        if required.numeric {
            for (function, kind) in [
                ("wire_mul_high", 7),
                ("wire_imul_high", 7),
                ("wire_div64", 8),
                ("wire_idiv64", 8),
            ] {
                name(&mut imports, "core");
                name(&mut imports, function);
                imports.extend_from_slice(&[0, kind]);
            }
        }
        super::vector::floating::imports(&mut imports, required.floating);
        super::vector::x87::imports(&mut imports, required.extended);
        for (index, function) in ["wire_crc8", "wire_crc16", "wire_crc32", "wire_crc64"]
            .into_iter()
            .enumerate()
        {
            if required.crc & (1 << index) != 0 {
                name(&mut imports, "core");
                name(&mut imports, function);
                imports.extend_from_slice(&[0, 14]);
            }
        }
        if required.clock {
            name(&mut imports, "platform");
            name(&mut imports, "clock");
            imports.extend_from_slice(&[0, 13]);
        }
        if required.vectors.active() {
            super::vector::bank::imports(&mut imports);
        }
        name(&mut imports, "jit");
        name(&mut imports, "materialize");
        imports.extend_from_slice(&[0, 15]);
        section(&mut module, 2, &imports);
        section(&mut module, 3, &[2, 6, 5]);
        let mut exports = vec![1];
        name(&mut exports, "chunk");
        exports.push(0);
        unsigned(&mut exports, required.function_count());
        section(&mut module, 7, &exports);
        let mut functions = vec![2];
        let mut locals = super::state::CHUNK_LOCALS.to_vec();
        if required.vectors.vector_locals() != 0 {
            locals.push((required.vectors.vector_locals(), 0x7b));
        }
        if required.vectors.wide_locals() != 0 {
            locals.push((required.vectors.wide_locals(), 0x7e));
        }
        if required.vectors.control_locals() != 0 {
            locals.push((required.vectors.control_locals(), 0x7f));
        }
        functions.extend(self.function_body(&locals));
        functions.extend(resolver.function_body(&[]));
        section(&mut module, 10, &functions);
        module
    }
}
