//! Owned input/result buffers; reset drops them back to the arena allocator.
use crate::{link_vertex, translate, Translation, INPUT_LIMIT, OUTPUT_LIMIT};
use std::cell::RefCell;
#[derive(Default)]
struct Session {
    input: Vec<u8>,
    output: Vec<u8>,
    error: Vec<u8>,
}
impl Session {
    fn reset(&mut self) {
        *self = Self::default();
    }
    fn fail(&mut self, message: String) {
        self.reset();
        self.error = message.into_bytes();
        self.error.truncate(8192);
    }
    fn publish(&mut self, result: Result<Translation, String>) -> u32 {
        match result.and_then(|v| serde_json::to_vec(&v).map_err(|e| e.to_string())) {
            Ok(output) if output.len() <= OUTPUT_LIMIT => {
                self.output = output;
                0
            }
            Ok(_) => {
                self.fail("shader package output capacity".into());
                1
            }
            Err(error) => {
                self.fail(error);
                1
            }
        }
    }
    fn input(&mut self, length: usize) -> *mut u8 {
        if self.input.try_reserve_exact(length).is_err() {
            self.fail("SPIR-V input allocation failed".into());
            return std::ptr::null_mut();
        }
        self.input.resize(length, 0);
        self.input.as_mut_ptr()
    }
}
thread_local! { static SESSION: RefCell<Session> = RefCell::default(); }
#[no_mangle]
pub extern "C" fn wire_shader_abi() -> u32 {
    2
}
#[no_mangle]
pub extern "C" fn wire_shader_input(length: u32) -> *mut u8 {
    SESSION.with(|cell| {
        let mut s = cell.borrow_mut();
        s.reset();
        if length < 20 || length as usize > INPUT_LIMIT {
            s.fail("SPIR-V input capacity".into());
            return std::ptr::null_mut();
        }
        s.input(length as usize)
    })
}
#[no_mangle]
pub extern "C" fn wire_shader_compile(stage: u32) -> u32 {
    SESSION.with(|cell| {
        let mut s = cell.borrow_mut();
        let source = std::mem::take(&mut s.input);
        let result = translate(&source, stage);
        drop(source);
        s.publish(result)
    })
}
#[no_mangle]
pub extern "C" fn wire_shader_link_input(vertex: u32, pixel: u32) -> *mut u8 {
    SESSION.with(|cell| {
        let mut s = cell.borrow_mut();
        s.reset();
        if !(20..=INPUT_LIMIT).contains(&(vertex as usize))
            || !(20..=INPUT_LIMIT).contains(&(pixel as usize))
        {
            s.fail("shader link input capacity".into());
            return std::ptr::null_mut();
        }
        s.input(vertex as usize + pixel as usize)
    })
}
#[no_mangle]
pub extern "C" fn wire_shader_link(vertex: u32) -> u32 {
    SESSION.with(|cell| {
        let mut s = cell.borrow_mut();
        let source = std::mem::take(&mut s.input);
        let split = vertex as usize;
        let result = if split <= source.len() {
            link_vertex(&source[..split], &source[split..])
        } else {
            Err("shader link input split".into())
        };
        drop(source);
        s.publish(result)
    })
}
#[no_mangle]
pub extern "C" fn wire_shader_output() -> *const u8 {
    SESSION.with(|s| s.borrow().output.as_ptr())
}
#[no_mangle]
pub extern "C" fn wire_shader_output_size() -> u32 {
    SESSION.with(|s| s.borrow().output.len() as u32)
}
#[no_mangle]
pub extern "C" fn wire_shader_error() -> *const u8 {
    SESSION.with(|s| s.borrow().error.as_ptr())
}
#[no_mangle]
pub extern "C" fn wire_shader_error_size() -> u32 {
    SESSION.with(|s| s.borrow().error.len() as u32)
}
#[no_mangle]
pub extern "C" fn wire_shader_reset() {
    SESSION.with(|s| s.borrow_mut().reset());
}
#[no_mangle]
pub extern "C" fn wire_shader_stat(which: u32) -> u32 {
    crate::allocator::stat(which)
}
