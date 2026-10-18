"""Original valid-DC query loop; one acquisition and one release per process."""
import struct
from amd64_perf import pe64
from perf.fixtures.local_gateway import image as local_image


def image(code, builder, width):
    result = local_image(code, builder, width)
    result[0x3c0:0x3cd] = b'gdi32.dll\0'.ljust(13, b'\0')
    result[0x400:0x460] = bytes(0x60)
    for i, name in enumerate([b'CreateCompatibleDC\0', b'GetDeviceCaps\0', b'DeleteDC\0']):
        offset = 0x402 + 0x20 * i
        result[offset:offset + len(name)] = name
    return result


def x64(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec20 31c9')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1 ba58000000')
    call(0x1188)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1')
    call(0x1190)
    code += bytes.fromhex('4901c6 4c89f0 4883c420 415f 415e 415c c3')
    return image(code, pe64, 8), dict(
        rax=hex(96 * n + 1), retired=6 * n + 18, work=7 * n + 20,
        api_calls=n + 2, startup=True)


def x86(work, pe32):
    n = max(1, work // 6)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 6a00 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('6a58 53 ff15') + struct.pack('<I', 0x401184)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('53 ff15') + struct.pack('<I', 0x401188)
    code += bytes.fromhex('01c6 89f0 5b 5e 5f cc')
    return image(code, pe32, 4), dict(
        eax=(96 * n + 1) & 0xffffffff, retired=6 * n + 15, api_calls=n + 2)
