"""Original PE64 image builder shared by architecture-specific fixtures."""
import struct


def pe64(code):
    raw = (len(code) + 511) // 512 * 512
    image = bytearray(512 + raw)
    image[:2] = b'MZ'
    def u16(at, value): struct.pack_into('<H', image, at, value)
    def u32(at, value): struct.pack_into('<I', image, at, value)
    def u64(at, value): struct.pack_into('<Q', image, at, value)
    u32(0x3c, 0x80)
    image[0x80:0x84] = b'PE\0\0'
    u16(0x84, 0x8664); u16(0x86, 1); u16(0x94, 0xf0)
    opt = 0x98
    u16(opt, 0x20b); u32(opt + 16, 0x1000); u64(opt + 24, 0x140000000)
    u32(opt + 32, 4096); u32(opt + 36, 512)
    u32(opt + 56, 4096 + (len(code) + 4095) // 4096 * 4096)
    u32(opt + 60, 512); u64(opt + 72, 65536); u64(opt + 80, 4096)
    u32(opt + 108, 16)
    section = opt + 0xf0
    image[section:section + 5] = b'.text'
    u32(section + 8, len(code)); u32(section + 12, 4096)
    u32(section + 16, raw); u32(section + 20, 512)
    u32(section + 36, 0x60000020)
    image[512:512 + len(code)] = code
    return image
