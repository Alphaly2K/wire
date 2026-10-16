"""Original multimedia capability loops consume both timer-resolution limits."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def mm_image(code, builder, width):
    data = image(code, builder, width, b'timeGetDevCaps\0')
    data[0x3c0:0x3ca] = b'winmm.dll\0'
    return data


def x64(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec38')
    loop = len(code)
    code += bytes.fromhex('488d4c2420 ba08000000')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('8b442424 c1e810 4901c6 8b442420 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c438 415f 415e c3')
    return mm_image(code, pe64, 8), dict(
        rax=hex(65536 * n), retired=10 * n + 10, work=11 * n + 10,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 11)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec08')
    loop = len(code)
    code += bytes.fromhex('8d0424 6a08 50 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('8b442404 c1e810 01c6 8b0424 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c408 5e 5f cc')
    return mm_image(code, pe32, 4), dict(
        eax=(65536 * n) & 0xffffffff, retired=11 * n + 9, api_calls=n)
