"""Bounded window field loops with consumed values and real logical windows."""
import struct
from amd64_perf import pe64


def image(code, builder, width, symbols):
    code.extend(bytes(0x800 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cb] = b'user32.dll\0'
    for i, symbol in enumerate(symbols):
        struct.pack_into('<Q' if width == 8 else '<I', code, 0x140 + i * width,
                         0x1200 + i * 0x20)
        code[0x202 + i * 0x20:0x202 + i * 0x20 + len(symbol)] = symbol
    if width == 8:
        for offset, fmt, value in [(0, '<I', 80), (8, '<Q', 0x140001600),
                                   (24, '<Q', 0x140000000),
                                   (64, '<Q', 0x140001380)]:
            struct.pack_into(fmt, code, 0x300 + offset, value)
        name = 'WireQueryPerf\0'.encode('utf-16le')
        code[0x380:0x380 + len(name)] = name
        # WM_NCCREATE returns 1; all other messages return 0. Four creation
        # and two destruction callbacks each execute these four instructions.
        callback = bytes.fromhex('31c0 81fa81000000 0f94c0 c3')
        code[0x600:0x600 + len(callback)] = callback
    result = bytearray(builder(code))
    struct.pack_into('<II', result, 0x98 + (112 if width == 8 else 96) + 8, 0x1100, 40)
    return result


def x64(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec60')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    at = len(code)
    code += bytes.fromhex('488d0d') + struct.pack('<i', 0x1300 - (0x1000 + at + 7))
    call(0x1180)
    for i, arg in enumerate([0, 0x140001380, 0, 0x10000000,
                             10, 20, 640, 480, 0, 0, 0x140000000, 0]):
        if i < 4:
            code += [bytes.fromhex(prefix) for prefix in ['48b9', '48ba', '49b8', '49b9']][i]
            code += struct.pack('<Q', arg)
        else:
            code += bytes.fromhex('48b8') + struct.pack('<Q', arg)
            code += bytes.fromhex('48894424') + bytes([32 + (i - 4) * 8])
    call(0x1188)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1 baf0ffffff')
    call(0x1190)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1')
    call(0x1198)
    code += bytes.fromhex('4901c6 4c89f0 4883c460 415f 415e 415c c3')
    return image(code, pe64, 8, [b'RegisterClassExW\0', b'CreateWindowExW\0',
                                b'GetWindowLongPtrW\0', b'DestroyWindow\0']), dict(
        rax=hex((n * 0x10000000 + 1) & ((1 << 64) - 1)),
        retired=6 * n + 63, work=7 * n + 66, api_calls=n + 3, startup=True)


def x86(work, pe32):
    n = max(1, work // 6)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c3 6a2a 6aeb 53 ff15') + struct.pack('<I', 0x401184)
    loop = len(code)
    code += bytes.fromhex('6aeb 53 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5b 5e 5f cc')
    return image(code, pe32, 4, [b'GetDesktopWindow\0', b'SetWindowLongA\0',
                                b'GetWindowLongW\0']), dict(
        eax=(42 * n) & 0xffffffff, retired=6 * n + 15, api_calls=n + 2)
