"""Original critical-section loop consumes TryEnter and the released recursion slot."""
import struct
from .pe import pe64


def image(code, builder, width):
    code.extend(bytes(0x300 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    for i, name in enumerate([b'InitializeCriticalSection\0', b'TryEnterCriticalSection\0',
                              b'LeaveCriticalSection\0']):
        symbol = 0x200 + i * 0x30
        struct.pack_into('<Q' if width == 8 else '<I', code, 0x140 + i * width,
                         0x1000 + symbol)
        code[symbol + 2:symbol + 2 + len(name)] = name
    data = bytearray(builder(code))
    struct.pack_into('<II', data, 0x98 + (112 if width == 8 else 96) + 8, 0x1100, 40)
    return data


def x64(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec58 488d4c2420')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    loop = len(code)
    code += bytes.fromhex('488d4c2420')
    call(0x1188)
    code += bytes.fromhex('4901c6 488d4c2420')
    call(0x1190)
    code += bytes.fromhex('8b44242c 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c458 415f 415e c3')
    return image(code, pe64, 8), dict(rax=hex(n), retired=9 * n + 12,
                                    work=11 * n + 13, api_calls=2 * n + 1, startup=True)


def x86(work, pe32):
    n = max(1, work // 11)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec18 8d0424 50 ff15') + struct.pack('<I', 0x401180)
    loop = len(code)
    code += bytes.fromhex('8d0424 50 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 8d0424 50 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('8b442408 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c418 5e 5f cc')
    return image(code, pe32, 4), dict(eax=n & 0xffffffff, retired=11 * n + 12,
                                    api_calls=2 * n + 1)
