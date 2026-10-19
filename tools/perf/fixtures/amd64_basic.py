"""Original AMD64 integer, memory and cache-pressure fixture bytes."""
import struct
from .pe import pe64


def fixtures(work):
    # r15 counts iterations, scratch memory is below the untouched return slot.
    init = bytes.fromhex('48 b8 01 00 00 00 00 00 00 00 '
                         '48 bb 21 43 65 87 a9 cb ed 0f '
                         '48 8d bc 24 00 ff ff ff 48 89 07')
    bodies = {
        'integer': (bytes.fromhex('48 01 d8 48 11 d9 49 89 c2 4c 31 d1 '
                                 '48 8d 49 03 48 39 d8 48 0f 42 d0'), 7, b''),
        'memory': (bytes.fromhex('48 01 07 48 8b 17 48 31 da 48 89 57 08 '
                                '48 03 47 08 48 8b 0f'), 6, b''),
        'simd-mixed': (bytes.fromhex('f3 0f 6f 07 66 0f fe c0 f3 0f 7f 07 '
                                    '66 48 0f 7e c0 48 01 d8 48 31 d9'), 6, b''),
        # 16,384 instructions / 53 KiB of code exercise bounded-cache conflicts.
        'large-code': (bytes.fromhex('48 01 d8 48 31 d9 4d 89 d3 48 8d 49 03') * 4096,
                       4 * 4096, b''),
        'indirect-call': (bytes.fromhex('ff d6 48 01 d8 48 31 d9 48 89 07'),
                          7, bytes.fromhex('48 8d 40 03 49 89 c2 c3')),
    }
    for name, (body, instructions, helper) in bodies.items():
        count = max(1, work // (instructions + 2))
        code = bytearray(init)
        if helper:
            lea_at = len(code)
            code += bytes.fromhex('48 8d 35 00 00 00 00')
        code += bytes.fromhex('41 bf') + struct.pack('<I', count)
        loop = len(code)
        code += body + bytes.fromhex('49 ff cf 0f 85')
        code += struct.pack('<i', loop - (len(code) + 4))
        # Include checked memory and another register in the checksum.
        code += bytes.fromhex('48 33 c1 48 33 07 c3')
        if helper:
            struct.pack_into('<i', code, lea_at + 3, len(code) - (lea_at + 7))
            code += helper
        yield name, pe64(code)
