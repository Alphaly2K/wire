"""Original bounded class loops: duplicate registration or real metadata copy-out."""
import struct
from amd64_perf import pe64


def image(code, builder, width, symbols):
    code.extend(bytes(0x400 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cb] = b'user32.dll\0'
    for i, symbol in enumerate(symbols):
        struct.pack_into('<Q' if width == 8 else '<I', code, 0x140 + i * width,
                         0x1200 + i * 0x20)
        code[0x202 + i * 0x20:0x202 + i * 0x20 + len(symbol)] = symbol
    if width == 8:
        for offset, fmt, value in [(0, '<I', 80), (4, '<I', 3),
                                   (8, '<Q', 0x140001000), (24, '<Q', 0x140000000),
                                   (64, '<Q', 0x140001380)]:
            struct.pack_into(fmt, code, 0x300 + offset, value)
    else:
        struct.pack_into('<12I', code, 0x300,
                         48, 3, 0x401000, 0, 0, 0x400000, 0, 0, 0, 0, 0x401380, 0)
    name = 'WirePerfClass\0'.encode('utf-16le')
    code[0x380:0x380 + len(name)] = name
    result = bytearray(builder(code))
    struct.pack_into('<II', result, 0x98 + (112 if width == 8 else 96) + 8, 0x1100, 40)
    return result


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec20')

    def record():
        at = len(code)
        code.extend(bytes.fromhex('488d0d') + struct.pack('<i', 0x1300 - (0x1000 + at + 7)))

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    record()
    call(0x1180)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    record()
    call(0x1180)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1 48ba') + struct.pack('<Q', 0x140000000)
    call(0x1188)
    code += bytes.fromhex('4901c6 4c89f0 4883c420 415f 415e 415c c3')
    return image(code, pe64, 8, [b'RegisterClassExW\0', b'UnregisterClassW\0']), dict(
        rax='0x1', retired=5 * n + 19, work=6 * n + 21, api_calls=n + 2, startup=True)


def x86(work, pe32):
    n = max(1, work // 11)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 6800134000 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c3 83ec28')
    loop = len(code)
    code += bytes.fromhex('c7042444332211 8d0424 50 53 6800004000 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 8b0424 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('83c428 6800004000 53 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 89f0 5b 5e 5f cc')
    return image(code, pe32, 4,
                 [b'RegisterClassExW\0', b'GetClassInfoW\0', b'UnregisterClassW\0']), dict(
        eax=(4 * n + 1) & 0xffffffff, retired=11 * n + 18, api_calls=n + 2)
