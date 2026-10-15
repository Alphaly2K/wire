"""Original PE32 acquire/release loop with exact count and return checksums."""
import struct


def x86(work, pe32):
    n = max(1, work // 9)
    code = bytearray.fromhex('57 53 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 6a00 6a00 6a00 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('6a00 53 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 53 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5b 5f cc')
    code.extend(bytes(0x300 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    for i, name in enumerate([b'CreateMutexW\0', b'WaitForSingleObject\0', b'ReleaseMutex\0']):
        symbol = 0x200 + 0x20 * i
        struct.pack_into('<I', code, 0x140 + i * 4, 0x1000 + symbol)
        code[symbol + 2:symbol + 2 + len(name)] = name
    result = pe32(code)
    struct.pack_into('<II', result, 0x98 + 96 + 8, 0x1100, 40)
    return result, dict(eax=n, retired=9 * n + 14, api_calls=2 * n + 1)
