"""Original event lifecycle loops with an algebraic return checksum."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 10)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec20 31c9 ba01000000 4531c0 4531c9')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x11a0)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1')
    call(0x1190)
    code += bytes.fromhex('4901c6 4c89e1')
    call(0x1198)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c420 415f 415e 415c c3')
    code.extend(bytes(512 - len(code)))
    for table in [0x180, 0x190]:
        struct.pack_into('<QQ', code, table + 8, 0x11e0, 0x11f0)
    code[0x1e2:0x1ed] = b'ResetEvent\0'
    code[0x1f2:0x1ff] = b'CreateEventW\0'
    return image(code, pe64, 8, b'SetEvent\0'), dict(
        rax=hex(2 * n), retired=8 * n + 18, work=10 * n + 19,
        api_calls=2 * n + 1, startup=True)


def x86(work, pe32):
    n = max(1, work // 8)
    code = bytearray.fromhex('57 53 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 6a00 6a00 6a01 6a00 ff15') + struct.pack('<I', 0x401198)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('53 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c6 53 ff15') + struct.pack('<I', 0x401194)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5b 5f cc')
    code.extend(bytes(512 - len(code)))
    for table in [0x180, 0x190]:
        struct.pack_into('<II', code, table + 4, 0x11e0, 0x11f0)
    code[0x1e2:0x1ed] = b'ResetEvent\0'
    code[0x1f2:0x1ff] = b'CreateEventW\0'
    return image(code, pe32, 4, b'SetEvent\0'), dict(
        eax=(2 * n) & 0xffffffff, retired=8 * n + 15)
