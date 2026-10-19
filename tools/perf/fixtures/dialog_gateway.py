"""Look up a real child control and consume the returned handle on every call."""
import struct
from .window_queries_gateway import image


def x86(work, pe32):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 56 53 55 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 68') + struct.pack('<I', 0x401300)

    def call(iat):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', iat))

    call(0x401180)
    for arg in reversed([0, 0x401380, 0, 0, 0, 0, 32, 32, 0, 0, 0x400000, 0]):
        code += b'\x68' + struct.pack('<I', arg)
    call(0x401184)
    code += bytes.fromhex('89c3')
    for index, arg in reversed(list(enumerate(
            [0, 0x401380, 0, 0x40000000, 0, 0, 32, 32, 0, 42, 0x400000, 0]))):
        code += b'\x53' if index == 8 else b'\x68' + struct.pack('<I', arg)
    call(0x401184)
    code += bytes.fromhex('89c5')
    loop = len(code)
    code += bytes.fromhex('6a2a 53')
    call(0x401188)
    code += bytes.fromhex('31e8 09c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    for push in ['55', '53']:
        code += bytes.fromhex(push)
        call(0x40118c)
        code += bytes.fromhex('83f001 09c6')
    code += bytes.fromhex('89f0 5d 5b 5e 5f cc')
    assert len(code) <= 0x100, 'entry must end before the import descriptor'
    code.extend(bytes(0x800 - len(code)))
    for offset, value in [(0x300, 48), (0x308, 0x401600),
                           (0x314, 0x400000), (0x328, 0x401380)]:
        struct.pack_into('<I', code, offset, value)
    name = 'WireDialogPerf\0'.encode('utf-16le')
    code[0x380:0x380 + len(name)] = name
    code[0x600:0x608] = bytes.fromhex('b803000000 c21000')
    return image(code, pe32, 4, [b'RegisterClassExW\0', b'CreateWindowExW\0',
                                b'GetDlgItem\0', b'DestroyWindow\0']), dict(
        eax=0, retired=7 * n + 65, api_calls=n + 5)
