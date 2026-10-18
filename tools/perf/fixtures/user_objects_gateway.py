"""Original PE32 property loop on one logical desktop window, no host callbacks."""
import struct


def x86(work, pe32):
    n = max(1, work // 6)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c3 6a2a 6a7b 53 ff15') + struct.pack('<I', 0x401184)
    loop = len(code)
    code += bytes.fromhex('6a7b 53 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('6a7b 53 ff15') + struct.pack('<I', 0x40118c)
    code += bytes.fromhex('01c6 89f0 5b 5e 5f cc')
    code.extend(bytes(0x300 - len(code)))
    for offset, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, offset, value)
    code[0x1c0:0x1cb] = b'user32.dll\0'
    for i, name in enumerate([b'GetDesktopWindow\0', b'SetPropA\0', b'GetPropW\0', b'RemovePropW\0']):
        struct.pack_into('<I', code, 0x140 + 4 * i, 0x1200 + 0x20 * i)
        code[0x202 + 0x20 * i:0x202 + 0x20 * i + len(name)] = name
    result = bytearray(pe32(code))
    struct.pack_into('<II', result, 0x98 + 96 + 8, 0x1100, 40)
    return result, dict(eax=(42 * (n + 1)) & 0xffffffff,
                        retired=6 * n + 19, api_calls=n + 3)
