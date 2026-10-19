"""Original AMD64 register, REP and basic API gateway expectations."""
import struct
from .pe import pe64

MASK = (1 << 64) - 1
CONST = 0x1122334455667788


def additional(name, work):
    code = bytearray()

    def emit(value):
        code.extend(bytes.fromhex(value))

    def imm(prefix, value, size=8):
        emit(prefix + int(value).to_bytes(size, 'little').hex())

    if name == 'sse-register':
        n = max(1, work // 6)
        imm('48b8', 1); emit('66480f6ec0')
        imm('48bb', CONST); emit('66480f6ecb')
        imm('41bf', n, 4); loop = len(code)
        emit('660fd4c1 660fd4c1 660ffbc1 660f6fd0')
        expected = (1 + n * CONST) & MASK; retired = total = 6 * n + 7
    elif name == 'sse-gpr-transfer':
        n = max(1, work // 6)
        imm('48b8', 1); imm('41bf', n, 4); loop = len(code)
        emit('66480f6ec0 66480f7ec3 4883c001 4831c3')
        expected = n + 1; retired = total = 6 * n + 3
    elif name == 'shift-register':
        n = max(1, work // 7)
        imm('48b8', 1); imm('48bb', CONST)
        imm('41bf', n, 4); loop = len(code)
        emit('48c1c00d 48c1c80d 48d1e3 48d1eb 4801d8')
        expected = (1 + n * CONST) & MASK; retired = total = 7 * n + 4
    else:
        assert name == 'rep-memory'
        n = max(1, work // 2055)
        imm('48b8', CONST); imm('41bf', n, 4); loop = len(code)
        emit('488dbc2400d0ffff'); imm('b9', 1024, 4); emit('f348ab')
        emit('488db42400d0ffff 488dbc2400a0ffff')
        imm('b9', 1024, 4); emit('f348a5')
        expected = 4 * CONST & MASK; retired = 9 * n + 7; total = 2055 * n + 7
    emit('49ffcf')
    at = len(code); emit('0f85' + struct.pack('<i', loop - (at + 6)).hex())
    if name == 'sse-register':
        emit('66480f7ed0')
    if name == 'rep-memory':
        emit('488b842400a0ffff 48038424f8bfffff 4803842400d0ffff 48038424f8efffff')
    emit('c3')
    return pe64(code), dict(rax=hex(expected), retired=retired, work=total,
                           bytes_written=16384 * n if name == 'rep-memory' else 0)


def gateway_fixture(work, version=False, nls=False):
    n = max(1, work // 5)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec28')
    loop = len(code)
    call = len(code); code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code); code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<Q', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'GetACP\0' if nls else b'GetVersion\0' if version else b'GetCurrentThreadId\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    image = bytearray(pe64(code))
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1100, 40)
    value = 932 if nls else 0x4a65000a if version else 1
    return image, dict(rax=hex(n * value & MASK), retired=4 * n + 10, work=5 * n + 10, api_calls=n, startup=True)


def nls_conversion_fixture(work):
    n = max(1, work // 12)
    code = bytearray.fromhex('4156 4157 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4531f6 4883ec38')
    loop = len(code)
    code += bytes.fromhex('488d442430 4889442420 c744242802000000 b9a4030000 31d2')
    at = len(code)
    code += bytes.fromhex('4c8d05') + struct.pack('<i', 0x11f0 - (0x1000 + at + 7))
    code += bytes.fromhex('41b9ffffffff')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b442430 4c01f0 4883c438 415f 415e c3')
    code.extend(bytes(512 - len(code)))
    for offset, value in [(0x100, 0x1180), (0x10c, 0x11c0), (0x110, 0x1190)]:
        struct.pack_into('<I', code, offset, value)
    struct.pack_into('<Q', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'MultiByteToWideChar\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    code[0x1f0:0x1f2] = b'A\0'
    image = bytearray(pe64(code))
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1100, 40)
    return image, dict(rax=hex(65 + 2 * n), retired=11 * n + 11, work=12 * n + 11, api_calls=n, startup=True)
