"""Original Win64 GUID gateway loop, independent of runtime catalog metadata."""
import struct
from amd64_perf import pe64


def fixture(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec38')
    loop = len(code)
    # CLSIDFromString(NULL, &stack_guid): S_OK and a zero GUID each time.
    code += bytes.fromhex('31c9 488d542420')
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('488b442420 4c01f0 4883c438 415f 415e c3')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<Q', code, 0x180, 0x11d0)
    code[0x1c0:0x1ca] = b'ole32.dll\0'
    symbol = b'CLSIDFromString\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    image = bytearray(pe64(code))
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1100, 40)
    return image, dict(rax='0x0', retired=6 * n + 11, work=7 * n + 11,
                      api_calls=n, startup=True)
