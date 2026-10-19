"""Original bounded token query loop; one token stays live for the whole run."""
import struct

from .classes_gateway import image
from .pe import pe64


def x64(work):
    n = max(1, work // 13)
    code = bytearray.fromhex('4154 4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec40 48c7c1ffffffff ba08000000 4c8d442428')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    code += bytes.fromhex('4901c6 4c8b642428 488d442438 4889442420')
    loop = len(code)
    code += bytes.fromhex('4c89e1 ba14000000 4c8d442430 41b904000000')
    call(0x1188)
    code += bytes.fromhex('4901c6 8b442430 4901c6 8b442438 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89f0 4883c440 415f 415e 415c c3')
    result = image(code, pe64, 8, [b'OpenProcessToken\0', b'GetTokenInformation\0'])
    result[0x3c0:0x3cd] = b'advapi32.dll\0'
    return result, dict(rax=hex(5 * n + 1), retired=12 * n + 20, work=13 * n + 21,
                        api_calls=n + 1, startup=True)


def x86(work, pe32):
    # ABI-only query consumes all five arguments, returns zero and writes nothing.
    n = max(1, work // 9)
    code = bytearray.fromhex('57 56 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31f6')
    loop = len(code)
    code += bytes.fromhex('6a01 6a04 6a01 6a14 6a01 ff15') + struct.pack('<I', 0x401180)
    code += bytes.fromhex('01c6 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89f0 5e 5f cc')
    result = image(code, pe32, 4, [b'GetTokenInformation\0'])
    result[0x3c0:0x3cd] = b'advapi32.dll\0'
    return result, dict(eax=0, retired=9 * n + 7, api_calls=n)
