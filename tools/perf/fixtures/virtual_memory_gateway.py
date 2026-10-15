"""Original VirtualQuery loops check both returned size and allocated base."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def imports(code, builder, width):
    code.extend(bytes(512 - len(code)))
    for table in [0x180, 0x190]:
        struct.pack_into('<Q' if width == 8 else '<I', code, table + width, 0x11f0)
    code[0x1f2:0x1ff] = b'VirtualAlloc\0'
    return image(code, builder, width, b'VirtualQuery\0')


def x64(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('4155 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec60 31c9 ba00100000 41b800300000 41b904000000')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1198)
    code += bytes.fromhex('4989c5')
    loop = len(code)
    code += bytes.fromhex('4c89e9 488d542420 41b830000000')
    call(0x1190)
    code += bytes.fromhex('4901c6 488b442420 4c29e8 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c460 415f 415e 415d c3')
    return imports(code, pe64, 8), dict(
        rax=hex(48 * n), retired=10 * n + 18, work=11 * n + 19,
        api_calls=n + 1, startup=True)


def x86(work, pe32):
    n = max(1, work // 11)
    code = bytearray.fromhex('57 53 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec20 6a04 6800300000 6800100000 6a00 ff15')
    code += struct.pack('<I', 0x401194) + bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('8d0424 6a1c 50 53 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c6 8b0424 29d8 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c420 5e 5b 5f cc')
    return imports(code, pe32, 4), dict(
        eax=(28 * n) & 0xffffffff, retired=11 * n + 17, api_calls=n + 1)
