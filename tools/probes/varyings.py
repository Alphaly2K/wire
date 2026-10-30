"""Original synthetic DXBC for stage interface and draw controls."""
import struct


def _pack(*words):
    return struct.pack('<' + 'I' * len(words), *words)


def _signature(entries):
    data = bytearray(_pack(len(entries), 8) + bytes(24 * len(entries)))
    for i, (name, index, system, kind, register, mask, used) in enumerate(entries):
        offset = len(data)
        data += name.encode() + b'\0'
        struct.pack_into('<5I4B', data, 8 + 24 * i,
                         offset, index, system, kind, register, mask, used, 0, 0)
    return bytes(data)


def _container(inputs, outputs, words, digest):
    words = [words[0], len(words), *words[2:]]
    chunks = [(b'ISGN', _signature(inputs)), (b'OSGN', _signature(outputs)),
              (b'SHDR', _pack(*words))]
    data = bytearray(b'DXBC' + bytes.fromhex(digest) + _pack(1, 0, 3) + bytes(12))
    for i, (tag, chunk) in enumerate(chunks):
        data += bytes((-len(data)) % 4)
        struct.pack_into('<I', data, 32 + 4 * i, len(data))
        data += tag + _pack(len(chunk)) + chunk
    struct.pack_into('<I', data, 24, len(data))
    return bytes(data)


# Fullscreen triangle from SV_VertexID; TEXCOORD0 is (.25, .5, .75, 1).
VERTEX_WORDS = [
    0x00010040, 0x00000000,
    0x04000060, 0x00101012, 0x00000000, 0x00000006,
    0x04000067, 0x001020f2, 0x00000000, 0x00000001,
    0x03000065, 0x001020f2, 0x00000001,
    0x02000068, 0x00000001,
    0x08000036, 0x001020f2, 0x00000000, 0x00004e46, 0xbf800000, 0x3f800000, 0x3f000000, 0x3f800000,
    0x07000020, 0x00100012, 0x00000000, 0x00101e46, 0x00000000, 0x00004001, 0x00000001,
    0x0c000037, 0x001020f2, 0x00000000, 0x00100006, 0x00000000, 0x00004e46, 0x40400000, 0x3f800000, 0x3f000000, 0x3f800000, 0x00102e46, 0x00000000,
    0x07000020, 0x00100012, 0x00000000, 0x00101e46, 0x00000000, 0x00004001, 0x00000002,
    0x0c000037, 0x001020f2, 0x00000000, 0x00100006, 0x00000000, 0x00004e46, 0xbf800000, 0xc0400000, 0x3f000000, 0x3f800000, 0x00102e46, 0x00000000,
    0x08000036, 0x001020f2, 0x00000001, 0x00004e46, 0x3e800000, 0x3f000000, 0x3f400000, 0x3f800000,
    0x0100003e,
]


def fixtures():
    result = {}
    for name, mask, digest in [
        ('varying.vs', 15, 'bf188c6b30b8b439f60df2c1ac457e4e'),
        ('short.vs', 3, '2f0be829582a86cae760a8cd831ad115'),
    ]:
        words = VERTEX_WORDS.copy()
        for i in range(len(words) - 1):
            if words[i] == 0x1020f2 and words[i + 1] == 1:
                words[i] = 0x102002 | (mask << 4)
        result[name] = (1, _container(
            [('SV_VERTEXID', 0, 6, 1, 0, 1, 1)],
            [('SV_POSITION', 0, 1, 3, 0, 15, 0), ('TEXCOORD', 0, 0, 3, 1, mask, 0)],
            words, digest))
    for name, register, kind, interpolation, digest in [
        ('varying.ps', 1, 3, 2, '890e9eb9af24897c5a53cb666241d1e0'),
        ('missing.ps', 2, 3, 2, '3dc878d52be19783985377fdc6faa436'),
        ('integer.ps', 1, 1, 1, '14caa37e1fb9360e2df7cdef115a6848'),
    ]:
        words = [0x40, 0, 0x3000062 | (interpolation << 11), 0x101072, register,
                 0x3000065, 0x1020f2, 0, 0x5000036, 0x102072, 0, 0x101e46, register,
                 0x5000036, 0x102082, 0, 0x4001, 0x3f800000, 0x100003e]
        result[name] = (2, _container(
            [('TEXCOORD', 0 if register == 1 else 1, 0, kind, register, 7, 7)],
            [('SV_TARGET', 0, 0, 3, 0, 15, 0)], words, digest))
    return result
