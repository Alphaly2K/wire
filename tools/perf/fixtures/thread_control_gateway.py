"""Original APC admission loop: invalid identity precedes callback validation."""
import struct
from amd64_perf import pe64
from perf.fixtures.thread_properties_gateway import image as properties_image


def image(code, builder, width):
    result = properties_image(code, builder, width)
    for i, symbol in enumerate([b'QueueUserAPC\0', b'GetLastError\0']):
        at = 0x200 + 0x200 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result


def x64(work):
    n = max(1, work // 8)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    code += bytes.fromhex('31c9 31d2 49b8') + struct.pack('<Q', 0xabcdef12_34567890)
    call(0x1180)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    call(0x1188)
    code += bytes.fromhex('4c01f0 4883c428 415f 415e c3')
    return image(code, pe64, 8), dict(rax='0x6', retired=7 * n + 11,
                                    work=8 * n + 12, api_calls=n + 1, startup=True)


def x86(work, pe32):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('6890785634 6a00 6a00 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01f0 5e 5f cc')
    return image(code, pe32, 4), dict(eax=120, retired=7 * n + 8, api_calls=n + 1)
