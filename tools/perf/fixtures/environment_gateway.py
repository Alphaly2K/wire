"""All nine PE32 environment exports, with only two bounded block allocations."""
import struct


def x86(work, pe32):
    n = max(1, work // 28)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')

    def call(index):
        code.extend(bytes.fromhex('ff15') + struct.pack('<I', 0x401180 + 4 * index))

    def push(value):
        code.extend(b'\x68' + struct.pack('<I', value))

    for index in [0, 1]:
        call(index)
        code.extend(bytes.fromhex('8b00 01c6'))
    loop = len(code)
    for index in [2, 3]:
        for value in [10, 0x30000, 0x30000]:
            push(value)
        call(index)
        code.extend(bytes.fromhex('01c6'))
    for index in [4, 5]:
        push(0xdeadbeef)
        call(index)
        code.extend(bytes.fromhex('01c6'))
    for index in [6, 7]:
        push(0x30000)
        push(0x30000)
        call(index)
        code.extend(bytes.fromhex('01c6'))
    call(8)
    code.extend(bytes.fromhex('01c6 4f'))
    at = len(code)
    code.extend(bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6))
    code.extend(bytes.fromhex('89f0 5e 5f cc'))
    assert len(code) < 0x100
    code.extend(bytes(0x400 - len(code)))
    for at, value in [(0x100, 0x1140), (0x10c, 0x11c0), (0x110, 0x1180)]:
        struct.pack_into('<I', code, at, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    names = ['GetEnvironmentStrings', 'GetEnvironmentStringsW',
             'GetEnvironmentVariableA', 'GetEnvironmentVariableW',
             'FreeEnvironmentStringsA', 'FreeEnvironmentStringsW',
             'SetEnvironmentVariableA', 'SetEnvironmentVariableW', 'GetEnvironmentStringsA']
    for i, name in enumerate(names):
        struct.pack_into('<I', code, 0x140 + 4 * i, 0x1200 + 0x20 * i)
        at = 0x202 + 0x20 * i
        text = name.encode() + b'\0'
        code[at:at + len(text)] = text
    result = pe32(code)
    struct.pack_into('<II', result, 0x98 + 104, 0x1100, 40)
    return result, dict(eax=(2 * n) & 0xffffffff, retired=28 * n + 13,
                        api_calls=7 * n + 2)
