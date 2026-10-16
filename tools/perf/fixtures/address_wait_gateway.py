"""Original WaitOnAddress loop compares unequal, explicit eight-byte values."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 9)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec48 48c744242001000000 48c744242800000000')
    loop = len(code)
    code += bytes.fromhex('488d4c2420 488d542428 41b808000000 4531c9')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c448 415f 415e c3')
    return image(code, pe64, 8, b'WaitOnAddress\0'), dict(
        rax=hex(n), retired=8 * n + 12, work=9 * n + 12, api_calls=n, startup=True)
