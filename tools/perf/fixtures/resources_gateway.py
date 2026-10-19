"""Original PE32 image exercising all ten resource gateways and both copy-outs."""
import struct


def x86(work, pe32):
    n = max(1, work // 55)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)

    def push(value):
        code.extend(b'\x68' + struct.pack('<I', value))

    def call(iat):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', iat))
        code.extend(bytes.fromhex('01c6'))

    for iat in [0x401300, 0x401304]:
        for value in [6, 1, 0]:
            push(value)
        call(iat)
        if iat == 0x401300:
            code.extend(bytes.fromhex('89c3'))
    for iat, language in [(0x401308, 0x409), (0x40130c, 0x777)]:
        for value in [language, 1, 6, 0]:
            push(value)
        call(iat)
    for iat in [0x401310, 0x401314]:
        code.extend(b'\x53')
        push(0)
        call(iat)
    for iat in [0x401318, 0x40131c]:
        push(0x401800)
        call(iat)
    for iat, limit, load in [(0x401330, 3, '0fb705'), (0x401334, 5, '0fb605')]:
        for value in [limit, 0x402000, 0, 0]:
            push(value)
        call(iat)
        code.extend(bytes.fromhex(load) + struct.pack('<I', 0x402000))
        code.extend(bytes.fromhex('01c6'))
    code.extend(b'\x4f')
    at = len(code)
    code.extend(bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6))
    code.extend(bytes.fromhex('89f0 5b 5e 5f cc'))
    assert len(code) <= 0x200
    code.extend(bytes(0x1200 - len(code)))
    for at, values in [(0x200, (0x1280, 0, 0, 0x1380, 0x1300)),
                       (0x214, (0x12b0, 0, 0, 0x1390, 0x1330))]:
        struct.pack_into('<5I', code, at, *values)
    for at, name in [(0x380, b'kernel32.dll\0'), (0x390, b'user32.dll\0')]:
        code[at:at + len(name)] = name
    symbols = ['FindResourceA', 'FindResourceW', 'FindResourceExA', 'FindResourceExW',
               'LoadResource', 'SizeofResource', 'LockResource', 'FreeResource',
               'LoadStringW', 'LoadStringA']
    for i, name in enumerate(symbols):
        slot = 0x280 + i * 4 if i < 8 else 0x2b0 + (i - 8) * 4
        struct.pack_into('<I', code, slot, 0x1400 + i * 0x20)
        text = name.encode() + b'\0'
        at = 0x402 + i * 0x20
        code[at:at + len(text)] = text
    for at, value in [(14, 1), (38, 1), (62, 2)]:
        struct.pack_into('<H', code, 0x600 + at, value)
    for at, value in [(16, 6), (20, 0x80000018), (40, 1), (44, 0x80000030),
                       (64, 0x411), (68, 80), (72, 0x409), (76, 96),
                       (80, 0x1800), (84, 36), (96, 0x1800), (100, 36)]:
        struct.pack_into('<I', code, 0x600 + at, value)
    struct.pack_into('<3H', code, 0x800, 2, 0x65e5, 0x672c)
    result = pe32(code)
    # Put copy-out data on its own writable page, preserving static code IR.
    struct.pack_into('<H', result, 0x86, 2)
    struct.pack_into('<I', result, 0x98 + 56, 0x3000)
    struct.pack_into('<I', result, 0x180, 0x1000)
    struct.pack_into('<I', result, 0x188, 0x1000)
    result[0x1a0:0x1a8] = b'.data\0\0\0'
    struct.pack_into('<4I', result, 0x1a8, 0x200, 0x2000, 0x200, 0x1200)
    struct.pack_into('<I', result, 0x1c4, 0xc0000040)
    struct.pack_into('<II', result, 0x98 + 104, 0x1200, 60)
    struct.pack_into('<II', result, 0x98 + 112, 0x1600, 112)
    per_loop = 3 * 0x401650 + 0x401660 + 2 * 0x401800 + 36 + 2 + 0x65e5 + 4 + 0x93
    return result, dict(eax=(per_loop * n) & 0xffffffff, retired=55 * n + 9,
                        api_calls=10 * n)
