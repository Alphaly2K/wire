"""Original HeapSize loops check a real small allocation and exact API counts."""
import struct
from amd64_perf import pe64


def image(code, builder, width):
    code.extend(bytes(0x300 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    for i, name in enumerate([b'GetProcessHeap\0', b'HeapAlloc\0', b'HeapSize\0']):
        symbol = 0x200 + 0x20 * i
        struct.pack_into('<Q' if width == 8 else '<I', code, 0x140 + i * width,
                         0x1000 + symbol)
        code[symbol + 2:symbol + 2 + len(name)] = name
    result = bytearray(builder(code))
    struct.pack_into('<II', result, 0x98 + (112 if width == 8 else 96) + 8,
                     0x1100, 40)
    return result


def x64(work):
    n = max(1, work // 8)
    code = bytearray.fromhex('4154 4155 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    code += bytes.fromhex('4989c5 4c89e9 ba08000000 41b817000000')
    call(0x1188)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e9 31d2 4d89e0')
    call(0x1190)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e 415d 415c c3')
    return image(code, pe64, 8), dict(
        rax=hex(23 * n), retired=7 * n + 21, work=8 * n + 23,
        api_calls=n + 2, startup=True)


def x86(work, pe32):
    n = max(1, work // 8)
    code = bytearray.fromhex('55 53 56 57 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c5 6a17 6a08 55 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('53 6a00 55 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5f 5e 5b 5d cc')
    return image(code, pe32, 4), dict(
        eax=(23 * n) & 0xffffffff, retired=7 * n + 18, api_calls=n + 2)
