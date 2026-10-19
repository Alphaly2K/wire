"""Original SRW loop checks each successful acquisition and the released token."""
import struct
from .pe import pe64


def x64(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec38 31c0 4889442420')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    code += bytes.fromhex('488d4c2420')
    call(0x1180)
    code += bytes.fromhex('4901c6 488d4c2420')
    call(0x1188)
    code += bytes.fromhex('488b442420 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c438 415f 415e c3')
    code.extend(bytes(0x300 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    for i, name in enumerate([b'TryAcquireSRWLockExclusive\0', b'ReleaseSRWLockExclusive\0']):
        symbol = 0x200 + i * 0x30
        struct.pack_into('<Q', code, 0x140 + i * 8, 0x1000 + symbol)
        code[symbol + 2:symbol + 2 + len(name)] = name
    data = bytearray(pe64(code))
    struct.pack_into('<II', data, 0x98 + 112 + 8, 0x1100, 40)
    return data, dict(rax=hex(n), retired=9 * n + 12, work=11 * n + 12,
                      api_calls=2 * n, startup=True)
