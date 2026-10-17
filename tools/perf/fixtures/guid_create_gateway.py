"""Original CoCreateGuid null-output admission loop, without random inputs."""
import struct
from amd64_perf import pe64
from perf.fixtures.com_gateway import image as com_image


def image(code, builder, width):
    result = com_image(code, builder, width)
    symbol = b'CoCreateGuid\0'
    result[0x400:0x430] = bytes(0x30)
    result[0x402:0x402 + len(symbol)] = symbol
    return result


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    code += bytes.fromhex('31c9')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1180 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8), dict(rax=hex(0x80070057 * n),
                                    retired=5 * n + 10, work=6 * n + 10,
                                    api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 5)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('6a00 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    return image(code, pe32, 4), dict(eax=(0x80004003 * n) & 0xffffffff,
                                    retired=5 * n + 7, api_calls=n)
