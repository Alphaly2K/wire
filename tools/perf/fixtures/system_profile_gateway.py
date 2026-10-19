"""Original GetSystemInfo loops consume both architecture and page-size output."""
import struct
from .pe import pe64
from .module_gateway import image


def x64(work):
    n = max(1, work // 9)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    # Shadow space followed by the 48-byte SYSTEM_INFO output.
    code += bytes.fromhex('4883ec58')
    loop = len(code)
    code += bytes.fromhex('488d4c2420')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('0fb7442420 4901c6 8b442424 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c458 415f 415e c3')
    return image(code, pe64, 8, b'GetSystemInfo\0'), dict(
        rax=hex(4105 * n), retired=8 * n + 10, work=9 * n + 10,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 9)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec24')
    loop = len(code)
    code += bytes.fromhex('8d0424 50 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('0fb70424 01c6 8b442404 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c424 5e 5f cc')
    return image(code, pe32, 4, b'GetSystemInfo\0'), dict(
        eax=(4096 * n) & 0xffffffff, retired=9 * n + 9, api_calls=n)
