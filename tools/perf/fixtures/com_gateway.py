"""Original balanced apartment loop; the second initialization returns S_FALSE."""
import struct
from amd64_perf import pe64
from perf.fixtures.thread_properties_gateway import image as properties_image


def image(code, builder, width):
    result = properties_image(code, builder, width)
    result[0x3c0:0x3cd] = b'ole32.dll\0' + bytes(3)
    for i, symbol in enumerate([b'CoInitializeEx\0', b'CoUninitialize\0']):
        at = 0x400 + i * 0x30
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result


def x64(work):
    n = max(1, work // 16)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    for _ in range(2):
        code += bytes.fromhex('31c9 ba02000000')
        call(0x1180)
        code += bytes.fromhex('4901c6')
    call(0x1188)
    call(0x1188)
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8), dict(rax=hex(n), retired=12 * n + 10,
                                    work=16 * n + 10, api_calls=4 * n, startup=True)


def x86(work, pe32):
    n = max(1, work // 12)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    for _ in range(2):
        code += bytes.fromhex('6a02 6a00 ff15') + struct.pack('<I', 0x401180)
        code += bytes.fromhex('01c6')
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    return image(code, pe32, 4), dict(eax=n & 0xffffffff, retired=12 * n + 7, api_calls=4 * n)
