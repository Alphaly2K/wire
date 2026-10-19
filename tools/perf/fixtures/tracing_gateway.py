"""Original disabled-ETW loop with one bounded provider and checked checksum."""
import struct

from .pe import pe64


def x64(work):
    n = max(1, work // 43)
    # Four saved registers and 0x78 local bytes retain Win64 call alignment.
    # Shadow/stack arguments occupy 0x00..0x2f, GUID 0x30..0x3f, handle 0x40.
    code = bytearray.fromhex('4154 4155 4156 4157 4531f6 41bf') + struct.pack('<I', n)
    code += bytes.fromhex('4883ec78 31c0 4889442430 4889442438')

    def call(index):
        at = len(code)
        code.extend(bytes.fromhex('ff15') + struct.pack('<i', 0x1290 + 8 * index - (0x1000 + at + 6)))
        code.extend(bytes.fromhex('4901c6'))

    code += bytes.fromhex('488d4c2430 31d2 4531c0 4c8d4c2440')
    call(0)
    code += bytes.fromhex('4c8b642440')
    loop = len(code)
    code += bytes.fromhex('4c89e1 488d542430')
    call(2)  # EventEnabled
    code += bytes.fromhex('4c89e1 ba05000000 49c7c0ffffffff')
    call(3)  # EventProviderEnabled
    code += bytes.fromhex('4c89e1 488d542430 4531c0 4531c9')
    call(4)  # EventWrite, zero descriptors
    code += bytes.fromhex('4c89e1 ba05000000 4531c0 41b901000000')
    call(5)  # EventWriteString, disabled: invalid text is not consumed
    code += bytes.fromhex('4c89e1 488d542430 41b801000000 41b901000000')
    code += bytes.fromhex('48c744242000000000 48c744242801000000')
    call(6)  # EventWriteTransfer, zero descriptors and invalid activity pointers
    code += bytes.fromhex('4c89e1 31d2 41b801000000 41b9ffffff7f')
    call(7)  # EventSetInformation, unsupported: status 50
    code += bytes.fromhex('41ffcf')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('4c89e1')
    call(1)  # Retire the only provider.
    code += bytes.fromhex('4c89e1 ba01000000 4531c0 4531c9')
    call(4)  # Retired handle: status 6, before checking the invalid descriptor.
    code += bytes.fromhex('4c89f0 4883c478 415f 415e 415d 415c c3')
    assert len(code) <= 0x200
    code.extend(bytes(0x500 - len(code)))
    for offset, value in [(0x200, 0x1240), (0x20c, 0x12e0), (0x210, 0x1290)]:
        struct.pack_into('<I', code, offset, value)
    code[0x2e0:0x2ed] = b'advapi32.dll\0'
    symbols = ['EventRegister', 'EventUnregister', 'EventEnabled', 'EventProviderEnabled',
               'EventWrite', 'EventWriteString', 'EventWriteTransfer', 'EventSetInformation']
    for index, name in enumerate(symbols):
        struct.pack_into('<Q', code, 0x240 + index * 8, 0x1300 + index * 0x28)
        symbol = name.encode('ascii') + b'\0'
        offset = 0x302 + index * 0x28
        code[offset:offset + len(symbol)] = symbol
    image = pe64(code)
    struct.pack_into('<II', image, 0x98 + 112 + 8, 0x1200, 40)
    # Each imported call adds one synthetic work unit beyond retired x86 ops.
    return image, dict(rax=hex(50 * n + 6), retired=37 * n + 33, work=43 * n + 36,
                       api_calls=6 * n + 3, startup=True)
