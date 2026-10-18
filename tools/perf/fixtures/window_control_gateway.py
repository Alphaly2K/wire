"""Consume SendMessage results from an application's guest WNDPROC on both ABIs."""
import struct
from amd64_perf import pe64
from perf.fixtures.window_queries_gateway import image


def x64(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('4154 4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec60')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    at = len(code)
    code += bytes.fromhex('488d0d') + struct.pack('<i', 0x1300 - (0x1000 + at + 7))
    call(0x1180)
    for i, arg in enumerate([0, 0x140001380, 0, 0x10000000,
                             10, 20, 640, 480, 0, 0, 0x140000000, 0]):
        if i < 4:
            code += bytes.fromhex(['48b9', '48ba', '49b8', '49b9'][i])
            code += struct.pack('<Q', arg)
        else:
            code += bytes.fromhex('31c0') if arg == 0 else bytes.fromhex('48b8') + struct.pack('<Q', arg)
            code += bytes.fromhex('48894424') + bytes([32 + (i - 4) * 8])
    call(0x1188)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('4c89e1 ba01800000 4531c0 4531c9')
    call(0x1190)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1')
    call(0x1198)
    code += bytes.fromhex('4901c6 4c89f0 4883c460 415f 415e 415c c3')
    assert len(code) <= 0x100, 'entry must end before the import descriptor'
    data = image(code, pe64, 8, [b'RegisterClassExW\0', b'CreateWindowExW\0',
                                b'SendMessageW\0', b'DestroyWindow\0'])
    # image installs the default class procedure; replace it with the same
    # application-defined two-instruction callback used by the PE32 image.
    data[0x200 + 0x600:0x200 + 0x60d] = bytes.fromhex('b803000000 c3') + bytes(7)
    return data, dict(rax=hex(3 * n + 1), retired=10 * n + 51,
                      work=11 * n + 54, api_calls=n + 3, startup=True)


def x86(work, pe32):
    n = max(1, work // 10)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 68') + struct.pack('<I', 0x401300)

    def call(iat):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', iat))

    call(0x401180)
    for arg in reversed([0, 0x401380, 0, 0x10000000,
                         10, 20, 640, 480, 0, 0, 0x400000, 0]):
        code += b'\x68' + struct.pack('<I', arg)
    call(0x401184)
    code += bytes.fromhex('89c3')
    loop = len(code)
    code += bytes.fromhex('6a00 6a00 6801800000 53')
    call(0x401188)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('53')
    call(0x40118c)
    code += bytes.fromhex('01c6 89f0 5b 5e 5f cc')
    code.extend(bytes(0x800 - len(code)))
    for offset, value in [(0x300, 48), (0x308, 0x401600),
                           (0x314, 0x400000), (0x328, 0x401380)]:
        struct.pack_into('<I', code, offset, value)
    name = 'WireControlPerf\0'.encode('utf-16le')
    code[0x380:0x380 + len(name)] = name
    code[0x600:0x608] = bytes.fromhex('b803000000 c21000')
    return image(code, pe32, 4, [b'RegisterClassExW\0', b'CreateWindowExW\0',
                                b'SendMessageW\0', b'DestroyWindow\0']), dict(
        eax=(3 * n + 1) & 0xffffffff, retired=10 * n + 36, api_calls=n + 3)
