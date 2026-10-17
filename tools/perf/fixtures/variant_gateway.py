"""Original scalar VARIANT copy/clear loop; checksum checks tag and payload."""
import struct
from perf.fixtures.bstr_gateway import image as bstr_image


def x86(work, pe32):
    n = max(1, work // 15)
    code = bytearray.fromhex('57 56 55 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec10 89e5 66c745000000')
    loop = len(code)
    code += bytes.fromhex('68') + struct.pack('<I', 0x401260)
    code += bytes.fromhex('55 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 0fb74500 01c6 8b4508 01c6')
    code += bytes.fromhex('55 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 0fb74500 01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c410 5d 5e 5f cc')
    result = bstr_image(code, pe32, 4)
    result[0x460:0x470] = struct.pack('<4HI4x', 3, 0, 0, 0, 42)
    for i, symbol in enumerate([b'VariantCopy\0', b'VariantClear\0']):
        at = 0x400 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result, dict(eax=45 * n & 0xffffffff, retired=15 * n + 13,
                        api_calls=2 * n)
