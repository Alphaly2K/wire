"""Original SYSTEMTIME-to-FILETIME loop with a fixed 1970-01-01 input."""
import struct
from .pe import pe64
from .module_gateway import image


def x64(work):
    n = max(1, work // 9)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    # Shadow space, 16-byte SYSTEMTIME and an 8-byte FILETIME output.
    code += bytes.fromhex('4883ec48 48b8') + struct.pack('<4H', 1970, 1, 0, 1)
    code += bytes.fromhex('4889442420 48c744242800000000')
    loop = len(code)
    code += bytes.fromhex('488d4c2420 488d542430')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 488b442430 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c448 415f 415e c3')
    return image(code, pe64, 8, b'SystemTimeToFileTime\0'), dict(
        rax=hex((116444736000000001 * n) & ((1 << 64) - 1)),
        retired=8 * n + 13, work=9 * n + 13, api_calls=n, startup=True)
