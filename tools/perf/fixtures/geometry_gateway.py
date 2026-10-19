"""Original OffsetRect loop with stack-only output and algebraic coordinates."""
import struct
from .pe import pe64
from .input_profile_gateway import image


def x64(work):
    n = max(1, work // 8)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec38 660fefc0 f30f7f442420')
    loop = len(code)
    code += bytes.fromhex('488d4c2420 ba01000000 41b8ffffffff')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1180 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b442420 4c01f0 4883c438 415f 415e c3')
    return image(code, pe64, 8, b'user32.dll\0', b'OffsetRect\0'), dict(
        rax=hex(2 * n), retired=7 * n + 13, work=8 * n + 13,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 8)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec10 31c0 890424 89442404 89442408 8944240c')
    loop = len(code)
    code += bytes.fromhex('89e0 6aff 6a01 50 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b0424 01f0 83c410 5e 5f cc')
    return image(code, pe32, 4, b'user32.dll\0', b'OffsetRect\0'), dict(
        eax=2 * n & 0xffffffff, retired=8 * n + 15, api_calls=n)
