"""Original module query loops: checksum includes every returned image handle."""
import struct
from amd64_perf import pe64


def image(code, builder, width, symbol=b'GetModuleHandleExW\0'):
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<Q' if width == 8 else '<I', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    result = bytearray(builder(code))
    struct.pack_into('<II', result, 0x98 + (112 if width == 8 else 96) + 8, 0x1100, 40)
    return result


def x64(work):
    n = max(1, work // 10)
    # Keep checksum registers across Win64 calls; the output is a local stack slot.
    code = bytearray.fromhex('4155 4156 4157 53 4531ed 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    # FROM_ADDRESS | UNCHANGED_REFCOUNT, code address in the main PE.
    code += bytes.fromhex('b906000000 48ba') + struct.pack('<Q', 0x140001000)
    code += bytes.fromhex('4c8d442420')
    call = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1190 - (0x1000 + call + 6))
    code += bytes.fromhex('4901c6 488b5c2420 4901dd 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e8 4c01f0 4883c428 5b 415f 415e 415d c3')
    return image(code, pe64, 8), dict(rax=hex(n * (0x140000000 + 1) & ((1 << 64) - 1)),
                                    retired=9 * n + 16, work=10 * n + 16,
                                    api_calls=n, startup=True)


def x86(work, pe32):
    n = max(1, work // 9)
    code = bytearray.fromhex('57 53 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 31f6 83ec04')
    loop = len(code)
    code += bytes.fromhex('8d0424 50 68') + struct.pack('<I', 0x401000)
    code += bytes.fromhex('6a06 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 033424 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 01f0 83c404 5e 5b 5f cc')
    return image(code, pe32, 4), dict(eax=(n * (0x400000 + 1)) & 0xffffffff,
                                    retired=9 * n + 13)
