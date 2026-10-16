"""Original LocalAlloc/GlobalSize loops exercise their shared allocation owner."""
import struct
from amd64_perf import pe64


def image(code, builder, width):
    code.extend(bytes(0x300 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    for i, name in enumerate([b'LocalAlloc\0', b'GlobalSize\0']):
        symbol = 0x200 + 0x20 * i
        struct.pack_into('<Q' if width == 8 else '<I', code, 0x140 + i * width,
                         0x1000 + symbol)
        code[symbol + 2:symbol + 2 + len(name)] = name
    result = bytearray(builder(code))
    struct.pack_into('<II', result, 0x98 + (112 if width == 8 else 96) + 8,
                     0x1100, 40)
    return result


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec20 b940000000 ba17000000')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1')
    call(0x1188)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c420 415f 415e 415c c3')
    return image(code, pe64, 8), dict(
        rax=hex(23 * n), retired=5 * n + 16, work=6 * n + 17,
        api_calls=n + 1, startup=True)


def x86(work, pe32):
    n = max(1, work // 6)
    code = bytearray.fromhex('53 56 57 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 6a17 6a40 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c6')
    loop = len(code)
    code += bytes.fromhex('56 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 5f 5e 5b cc')
    return image(code, pe32, 4), dict(
        eax=(23 * n) & 0xffffffff, retired=5 * n + 13, api_calls=n + 1)
