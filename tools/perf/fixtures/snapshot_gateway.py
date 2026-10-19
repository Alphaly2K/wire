"""Original bounded Toolhelp copyout loop; one snapshot is opened and closed."""
import struct

from .classes_gateway import image
from .pe import pe64


def x64(work):
    n = max(1, work // 12)
    code = bytearray.fromhex('4154 4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4881ec70020000 b902000000 31d2')

    def call(iat):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', iat - (0x1000 + at + 6)))

    call(0x1180)
    code += bytes.fromhex('4989c4')
    loop = len(code)
    code += bytes.fromhex('c744243038020000 4c89e1 488d542430')
    call(0x1188)
    code += bytes.fromhex('4901c6 8b442438 4901c6 8b442430 4901c6 41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1')
    call(0x1190)
    code += bytes.fromhex('4901c6 4c89f0 4881c470020000 415f 415e 415c c3')
    result = image(code, pe64, 8, [b'CreateToolhelp32Snapshot\0', b'Process32FirstW\0', b'CloseHandle\0'])
    result[0x3c0:0x3cd] = b'kernel32.dll\0'
    return result, dict(rax=hex(570 * n + 1), retired=11 * n + 19, work=12 * n + 21,
                        api_calls=n + 2, startup=True)
