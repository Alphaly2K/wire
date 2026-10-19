"""Original shared stock-arrow lookup loop; resource count stays constant."""
import struct
from .pe import pe64
from .input_profile_gateway import image


def x64(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    code += bytes.fromhex('31c9 ba007f0000')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1180 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, b'user32.dll\0', b'LoadCursorA\0'), dict(
        rax=hex((n * 0x7ffd00610000) & 0xffffffffffffffff), retired=6*n+10,
        work=7*n+10, api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 6)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('68007f0000 6a00 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    return image(code, pe32, 4, b'user32.dll\0', b'LoadCursorA\0'), dict(
        eax=(n * 0x42007f00) & 0xffffffff, retired=6*n+7, api_calls=n)
