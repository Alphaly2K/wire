"""Original BSTR prefix queries; both architectures accept caller-owned storage."""
import struct
from amd64_perf import pe64
from perf.fixtures.thread_properties_gateway import image as gateway_image


def image(code, builder, width):
    code.extend(bytes(0x280 - len(code)))
    struct.pack_into('<I3H', code, 0x260, 6, 0x41, 0, 0xd800)
    result = gateway_image(code, builder, width)
    result[0x3c0:0x3cd] = b'oleaut32.dll\0'
    for i, symbol in enumerate([b'SysStringLen\0', b'SysStringByteLen\0']):
        at = 0x400 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result


def x64(work):
    n = max(1, work // 10)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    for iat in [0x1180, 0x1188]:
        at = len(code)
        code += bytes.fromhex('488d0d') + struct.pack('<i', 0x1264 - (0x1000 + at + 7))
        at = len(code)
        code += bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6))
        code += bytes.fromhex('4901c6')
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8), dict(rax=hex(9 * n), retired=8 * n + 10,
                                    work=10 * n + 10, api_calls=2 * n, startup=True)


def x86(work, pe32):
    n = max(1, work // 8)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    for iat in [0x401180, 0x401184]:
        code += bytes.fromhex('68') + struct.pack('<I', 0x401264)
        code += bytes.fromhex('ff15') + struct.pack('<I', iat)
        code += bytes.fromhex('01c6')
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    return image(code, pe32, 4), dict(eax=(9 * n) & 0xffffffff,
                                    retired=8 * n + 7, api_calls=2 * n)
