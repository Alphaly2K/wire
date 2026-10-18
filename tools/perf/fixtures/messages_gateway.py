"""Post and consume real thread messages, accumulating the delivered WPARAM."""
import struct
from amd64_perf import pe64
from perf.fixtures.window_queries_gateway import image


def x64(work):
    n = max(1, work // 15)
    code = bytearray.fromhex('4156 4157 4883ec58 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    code += bytes.fromhex('31c9 ba01800000 41b807000000 4531c9')
    call(0x1180)
    code += bytes.fromhex('488d4c2420')
    code += bytes.fromhex('31d2 4531c0 4531c9')
    call(0x1188)
    code += bytes.fromhex('4c03742430')
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c458 415f 415e c3')
    assert len(code) <= 0x100, 'entry must end before the import descriptor'
    return image(code, pe64, 8, [b'PostMessageW\0', b'GetMessageW\0']), dict(
        rax=hex(7 * n), retired=13 * n + 10, work=15 * n + 10,
        api_calls=2 * n, startup=True)


def x86(work, pe32):
    n = max(1, work // 14)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 83ec20')

    def call(iat):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', iat))

    loop = len(code)
    code += bytes.fromhex('6a00 6a07 6801800000 6a00')
    call(0x401180)
    code += bytes.fromhex('89e0 6a00 6a00 6a00 50')
    call(0x401184)
    code += bytes.fromhex('03742408')
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 83c420 5e 5f cc')
    assert len(code) <= 0x100, 'entry must end before the import descriptor'
    return image(code, pe32, 4, [b'PostMessageW\0', b'GetMessageW\0']), dict(
        eax=(7 * n) & 0xffffffff, retired=14 * n + 9, api_calls=2 * n)
