"""Original SLIST initialization loops over guest stack headers."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec38')
    loop = len(code)
    code += bytes.fromhex('488d4c2420')
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b442420 4c01f0 4883c438 415f 415e c3')
    return image(code, pe64, 8, b'InitializeSListHead\0'), dict(
        rax='0x0', retired=5 * n + 11, work=6 * n + 11,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 6)
    # This fixture stops at INT3, so aligning ESP needs no caller restoration.
    code = bytearray.fromhex('83e4f8 57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 83ec08')
    loop = len(code)
    code += bytes.fromhex('8d0424 50 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b0424 01d8 83c408 5b 5f cc')
    return image(code, pe32, 4, b'InitializeSListHead\0'), dict(
        eax=0, retired=6 * n + 11, api_calls=n)
