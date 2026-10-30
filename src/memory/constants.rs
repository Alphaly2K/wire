// Shared Windows virtual memory constants, independent of host backing.
pub const GUEST_PAGE: u32 = 4096;
pub const ALLOCATION_GRANULARITY: u32 = 65536;
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_MAPPED: u32 = 0x40000;
pub const MEM_IMAGE: u32 = 0x1000000;
pub const PAGE_NOACCESS: u32 = 1;
pub const PAGE_READONLY: u32 = 2;
pub const PAGE_READWRITE: u32 = 4;
pub const PAGE_EXECUTE: u32 = 16;
pub const PAGE_EXECUTE_READ: u32 = 32;
pub const PAGE_EXECUTE_READWRITE: u32 = 64;
pub const PAGE_GUARD: u32 = 256;
