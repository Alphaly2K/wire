"""Original CancelWaitableTimer loop; no time-dependent expiration checksum."""
import struct
from amd64_perf import pe64
from perf.fixtures.module_gateway import image


def x64(work):
    n = max(1, work // 6)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec20 31c9 ba01000000 4531c0')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1198)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1')
    call(0x1190)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c420 415f 415e 415c c3')
    code.extend(bytes(512 - len(code)))
    for table in [0x180, 0x190]:
        struct.pack_into('<Q', code, table + 8, 0x1140)
    symbol = b'CreateWaitableTimerW\0'
    code[0x142:0x142 + len(symbol)] = symbol
    return image(code, pe64, 8, b'CancelWaitableTimer\0'), dict(
        rax=hex(n), retired=5 * n + 17, work=6 * n + 18,
        api_calls=n + 1, startup=True)
