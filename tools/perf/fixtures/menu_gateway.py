"""Toggle one real menu item's checked bit and consume both prior states."""
import struct
from .window_queries_gateway import image


def x86(work, pe32):
    n = max(1, work // 13)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')

    def call(iat):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', iat))

    call(0x401180)
    code += bytes.fromhex('89c3 68') + struct.pack('<I', 0x401300)
    code += bytes.fromhex('6a01 6a00 53')
    call(0x401184)
    code += bytes.fromhex('83f001 09c6')
    loop = len(code)
    code += bytes.fromhex('6a08 6a4d 53')
    call(0x401188)
    code += bytes.fromhex('09c6 6a00 6a4d 53')
    call(0x401188)
    code += bytes.fromhex('83f008 09c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('53')
    call(0x40118c)
    code += bytes.fromhex('83f001 09c6 89f0 5b 5e 5f cc')
    assert len(code) <= 0x100, 'entry must end before the import descriptor'
    code.extend(bytes(0x800 - len(code)))
    for index, field in enumerate([48, 2, 0, 0, 77, 0, 0, 0, 0, 0, 0, 0]):
        struct.pack_into('<I', code, 0x300 + index * 4, field)
    return image(code, pe32, 4, [b'CreateMenu\0', b'InsertMenuItemW\0',
                                b'CheckMenuItem\0', b'DestroyMenu\0']), dict(
        eax=0, retired=13 * n + 22, api_calls=2 * n + 3)
