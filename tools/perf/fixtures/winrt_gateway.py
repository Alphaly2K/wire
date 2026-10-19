"""Original bounded HSTRING reference/length loop; no per-iteration allocation."""
import struct
from .pe import pe64
from .thread_properties_gateway import image


def x64(work):
    n = max(1, work // 13)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec48')
    loop = len(code)
    at = len(code)
    code += bytes.fromhex('488d0d') + struct.pack('<i', 0x1260 - (0x1000 + at + 7))
    code += bytes.fromhex('ba04000000 4c8d442420 4c8d4c2438')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1180 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 488b4c2438')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1188 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c448 415f 415e c3')
    code.extend(bytes(0x280 - len(code)))
    struct.pack_into('<5H', code, 0x260, 0x41, 0, 0xd800, 0x65e5, 0)
    result = image(code, pe64, 8)
    dll = b'api-ms-win-core-winrt-string-l1-1-0.dll\0'
    result[0x3c0:0x400] = dll + bytes(0x40 - len(dll))
    for i, symbol in enumerate([b'WindowsCreateStringReference\0', b'WindowsGetStringLen\0']):
        at = 0x400 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result, dict(rax=hex(4 * n), retired=11 * n + 10,
                        work=13 * n + 10, api_calls=2 * n, startup=True)
