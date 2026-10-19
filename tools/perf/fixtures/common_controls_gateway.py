"""Bounded PE32 control initialization; retain both legacy scalar returns."""
import struct

from .classes_gateway import image


def x86(work, pe32):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 68') + struct.pack('<I', 0x401300)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    result = image(code, pe32, 4,
                   [b'InitCommonControls\0', b'InitCommonControlsEx\0'])
    # Same one-section PE layout, using the actual common-control provider.
    provider = b'comctl32.dll\0'
    result[0x3c0:0x3c0 + len(provider)] = provider
    struct.pack_into('<II', result, 0x500, 8, 0xffffffff)
    return result, dict(eax=(2 * n) & 0xffffffff, retired=7 * n + 7,
                        api_calls=2 * n)
