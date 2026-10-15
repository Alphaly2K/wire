"""Original rejected metadata-query loops; validate every return and API count."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 8)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')
    loop = len(code)
    # Invalid information class: neither the null path nor null output is read.
    code += bytes.fromhex('31c9 ba01000000 4531c0')
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, b'GetFileAttributesExA\0'), dict(
        rax='0x0', retired=7 * n + 10, work=8 * n + 10,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db')
    loop = len(code)
    # PE32 keeps its known ABI and unimplemented zero return for this export.
    code += bytes.fromhex('6a00 6a01 6a00 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 5b 5f cc')
    return image(code, pe32, 4, b'GetFileAttributesExA\0'), dict(
        eax=0, retired=7 * n + 7, api_calls=n)
