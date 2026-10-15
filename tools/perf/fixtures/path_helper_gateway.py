"""Original NULL path-removal loop; no host filesystem or clock reads."""
import struct
from perf.fixtures.module_gateway import image


def x86(work, pe32):
    n = max(1, work // 5)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db')
    loop = len(code)
    code += bytes.fromhex('6a00 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 5b 5f cc')
    result = image(code, pe32, 4, b'PathRemoveFileSpecA\0')
    result[0x3c0:0x3cd] = b'shlwapi.dll\0\0'
    return result, dict(eax=0, retired=5 * n + 7, api_calls=n)
