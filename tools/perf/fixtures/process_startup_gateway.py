"""Consume both command pointers, both STARTUPINFO records and feature replies."""
import struct

from .classes_gateway import image


def x86(work, pe32):
    n = max(1, work // 31)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec44')
    loop = len(code)
    for iat in [0x401180, 0x401184]:
        code += bytes.fromhex('ff15') + struct.pack('<I', iat)
        code += bytes.fromhex('85c0 0f95c0 0fb6c0 01c6')
    for iat in [0x401188, 0x40118c]:
        code += bytes.fromhex('8d0424 50 ff15') + struct.pack('<I', iat)
        code += bytes.fromhex('8b0424 01c6')
    for feature in [6, 10, 12]:
        code += b'\x68' + struct.pack('<I', feature)
        code += bytes.fromhex('ff15') + struct.pack('<I', 0x401190)
        code += bytes.fromhex('01c6')
    code += b'\x4f'
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('83c444 89f0 5b 5e 5f cc')
    result = image(code, pe32, 4, [b'GetCommandLineA\0', b'GetCommandLineW\0',
                   b'GetStartupInfoA\0', b'GetStartupInfoW\0', b'IsProcessorFeaturePresent\0'])
    provider = b'kernel32.dll\0'
    result[0x3c0:0x3c0 + len(provider)] = provider
    return result, dict(eax=(140 * n) & 0xffffffff, retired=31 * n + 11,
                        api_calls=7 * n)
