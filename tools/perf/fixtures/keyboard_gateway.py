"""Original key-state snapshots and explicit US-layout query loops."""
import struct
from amd64_perf import pe64
from perf.fixtures.thread_properties_gateway import image as gateway_image


def image(code, builder, width, symbols):
    result = gateway_image(code, builder, width)
    result[0x3c0:0x3cd] = b'user32.dll\0'.ljust(13, b'\0')
    for i, symbol in enumerate(symbols):
        at = 0x400 + i * 0x30
        struct.pack_into('<Q' if width == 8 else '<I', result,
                         0x340 + i * width, 0x1200 + i * 0x30)
        result[at:at + 0x30] = bytes(0x30)
        result[at + 2:at + 2 + len(symbol)] = symbol
    return result


STATE_SYMBOLS = [b'GetKeyboardState\0', b'GetKeyState\0', b'GetAsyncKeyState\0']


def x64(work):
    n = max(1, work // 14)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    # Shadow space followed by a 256-byte stack buffer, below saved registers.
    code += bytes.fromhex('4881ec28010000')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    for i, argument in enumerate(['488d4c2428', 'b941000000', 'b941000000']):
        code += bytes.fromhex(argument)
        call(0x1180 + 8 * i)
        code += bytes.fromhex('4901c6')
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4881c428010000 415f 415e c3')
    return image(code, pe64, 8, STATE_SYMBOLS), dict(
        rax=hex(n), retired=11 * n + 10, work=14 * n + 10,
        api_calls=3 * n, startup=True)


def x86(work, pe32):
    n = max(1, work // 12)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6 81ec00010000')
    loop = len(code)
    for i, argument in enumerate(['8d0424 50', '6a41', '6a41']):
        code += bytes.fromhex(argument + ' ff15') + struct.pack('<I', 0x401180 + 4 * i)
        code += bytes.fromhex('01c6')
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 81c400010000 5e 5f cc')
    return image(code, pe32, 4, STATE_SYMBOLS), dict(
        eax=n & 0xffffffff, retired=12 * n + 9, api_calls=3 * n)


def x64_layout(work):
    n = max(1, work // 12)
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    loop = len(code)
    code += bytes.fromhex('b941000000 31d2 41b809040904')
    call(0x1180)
    code += bytes.fromhex('4901c6 31c9')
    call(0x1188)
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    return image(code, pe64, 8, [b'MapVirtualKeyExW\0', b'GetKeyboardType\0']), dict(
        rax=hex(34 * n), retired=10 * n + 10, work=12 * n + 10,
        api_calls=2 * n, startup=True)
