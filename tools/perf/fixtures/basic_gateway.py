"""Original fixed-input processor-count and PE32 string-query loops."""
import struct
from .pe import pe64
from .thread_properties_gateway import image as gateway_image


def image(code, builder, width, symbols):
    result = gateway_image(code, builder, width)
    for i, symbol in enumerate(symbols):
        at = 0x400 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result


COUNTS = [b'GetActiveProcessorCount\0', b'GetMaximumProcessorCount\0']


def x64(work):
    n = max(1, work // 10)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    for iat in [0x1180, 0x1188]:
        code += bytes.fromhex('b9ffff0000')
        at = len(code)
        code += bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6))
        code += bytes.fromhex('4901c6')
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, COUNTS), dict(rax=hex(2 * n), retired=8 * n + 10,
                                            work=10 * n + 10, api_calls=2 * n, startup=True)


def x86(work, pe32):
    n = max(1, work // 8)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    for iat in [0x401180, 0x401184]:
        code += bytes.fromhex('68ffff0000 ff15') + struct.pack('<I', iat)
        code += bytes.fromhex('01c6')
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    return image(code, pe32, 4, COUNTS), dict(eax=2 * n & 0xffffffff,
                                            retired=8 * n + 7, api_calls=2 * n)


def x86_strings(work, pe32):
    n = max(1, work // 9)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('68') + struct.pack('<I', 0x401260)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 68') + struct.pack('<I', 0x401268)
    code += bytes.fromhex('68') + struct.pack('<I', 0x401260)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    result = image(code, pe32, 4, [b'lstrlenW\0', b'lstrcmpiW\0'])
    result[0x460:0x470] = 'AbC\0aBc\0'.encode('utf-16le')
    return result, dict(eax=3 * n & 0xffffffff, retired=9 * n + 7, api_calls=2 * n)
