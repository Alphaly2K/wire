"""Original standard-handle loops with independent return/API-count goldens."""
import struct
from .pe import pe64
from .module_gateway import image


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')
    loop = len(code)
    code += bytes.fromhex('b9f5ffffff')  # STD_OUTPUT_HANDLE is a DWORD selector.
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, b'GetStdHandle\0'), dict(
        rax=hex(0x101 * n), retired=5 * n + 10, work=6 * n + 10,
        api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 5)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db')
    loop = len(code)
    code += bytes.fromhex('68f5ffffff ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 5b 5f cc')
    return image(code, pe32, 4, b'GetStdHandle\0'), dict(
        eax=0, retired=5 * n + 7, api_calls=n)
