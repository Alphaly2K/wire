"""Original absent-device count and IME-query loops, with no host input."""
import struct
from amd64_perf import pe64
from perf.fixtures.thread_properties_gateway import image as gateway_image


def image(code, builder, width, dll, symbol):
    result = gateway_image(code, builder, width)
    result[0x3c0:0x3cd] = dll.ljust(13, b'\0')
    fmt = '<Q' if width == 8 else '<I'
    struct.pack_into(fmt, result, 0x340, 0x1200)
    # The common container starts with two imports; this fixture uses only one.
    struct.pack_into(fmt, result, 0x340 + width, 0)
    result[0x400:0x430] = bytes(0x30)
    result[0x402:0x402 + len(symbol)] = symbol
    return result


def x64(work, ime=False):
    n = max(1, work // (6 if ime else 11))
    code = bytearray.fromhex('4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec28')
    loop = len(code)
    if ime:
        code += bytes.fromhex('b909040904')
    else:
        code += bytes.fromhex('c744242044332211 31c9 488d542420 41b810000000')
    at = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<i', 0x1180 - (0x1000 + at + 6))
    code += bytes.fromhex('4901c6')
    if not ime:
        code += bytes.fromhex('8b442420 4901c6')
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c428 415f 415e c3')
    dll, symbol = ((b'imm32.dll\0', b'ImmIsIME\0') if ime else
                   (b'user32.dll\0', b'GetRawInputDeviceList\0'))
    return image(code, pe64, 8, dll, symbol), dict(
        rax='0x0', retired=(5 if ime else 10) * n + 10,
        work=(6 if ime else 11) * n + 10, api_calls=n, startup=True)


def x86(work, pe32, ime=False):
    n = max(1, work // (5 if ime else 11))
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    if not ime:
        code += bytes.fromhex('83ec04')
    loop = len(code)
    if ime:
        code += bytes.fromhex('6809040904')
    else:
        code += bytes.fromhex('c7042444332211 8d0424 6a10 50 6a00')
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6')
    if not ime:
        code += bytes.fromhex('8b0424 01c6')
    code += bytes.fromhex('4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0')
    if not ime:
        code += bytes.fromhex('83c404')
    code += bytes.fromhex('5e 5f cc')
    dll, symbol = ((b'imm32.dll\0', b'ImmIsIME\0') if ime else
                   (b'user32.dll\0', b'GetRawInputDeviceList\0'))
    return image(code, pe32, 4, dll, symbol), dict(
        eax=0 if ime else (n * 0x11223344) & 0xffffffff,
        retired=(5 * n + 7) if ime else (11 * n + 9), api_calls=n)
