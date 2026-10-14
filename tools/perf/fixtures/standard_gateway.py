"""Original file/endpoint query loops with algebraic return and API-count goldens."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')
    loop = len(code)
    code += bytes.fromhex('b901010000')  # Original stdout endpoint, not redirect slot.
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, b'GetFileType\0'), dict(
        rax=hex(3 * n), retired=5 * n + 10, work=6 * n + 10,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 5)
    code = bytearray.fromhex('57 53 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 6a00 6880000000 6a03 6a00 6a00 6800000080 68')
    # Open this fixture's own PE read-only; no asset or host write is required.
    code += struct.pack('<I', 0x401140)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401194)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('53 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5b 5f cc')
    code.extend(bytes(512 - len(code)))
    for offset in [0x184, 0x194]:
        struct.pack_into('<I', code, offset, 0x11e0)
    code[0x1e2:0x1ee] = b'CreateFileA\0'
    path = b'gateway-file-type.exe\0'
    code[0x140:0x140 + len(path)] = path
    return image(code, pe32, 4, b'GetFileType\0'), dict(
        eax=n & 0xffffffff, retired=5 * n + 18, api_calls=n + 1)
