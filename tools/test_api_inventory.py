"""Independent miniature PE/CLR fixtures for inventory evidence and bounds."""
import struct
import tempfile
import unittest
from pathlib import Path
from api_inventory import PE, scan_game, trace_records


def fixture(bits=32, delay_va=False):
    data = bytearray(512 + 4096)
    def word(at, value, width=4): data[at:at+width] = value.to_bytes(width, 'little')
    def rva(at): return 512 + at - 0x1000
    data[:2] = b'MZ'
    word(0x3c, 0x80)
    data[0x80:0x84] = b'PE\0\0'
    word(0x84, 0x14c if bits == 32 else 0x8664, 2)
    word(0x86, 1, 2)
    opt_size = 224 if bits == 32 else 240
    word(0x94, opt_size, 2)
    opt = 0x98
    word(opt, 0x10b if bits == 32 else 0x20b, 2)
    base = 0x400000 if bits == 32 else 0x140000000
    word(opt + (28 if bits == 32 else 24), base, bits // 8)
    word(opt + 60, 512)
    directory = opt + (96 if bits == 32 else 112)
    word(directory - 4, 16)
    word(directory + 8, 0x1000); word(directory + 12, 40)
    word(directory + 13*8, 0x1400); word(directory + 13*8+4, 64)
    sec = opt + opt_size
    data[sec:sec+6] = b'.rdata'
    for at, value in [(8,4096),(12,0x1000),(16,4096),(20,512)]: word(sec+at, value)
    for at, value in [(0,0x1200),(12,0x1100),(16,0x1200)]: word(rva(0x1000)+at,value)
    data[rva(0x1100):rva(0x1100)+13] = b'KERNEL32.dll\0'
    width = bits // 8
    word(rva(0x1200), 0x1300, width)
    word(rva(0x1200)+width, (1 << (bits-1)) | 17, width)
    data[rva(0x1300)+2:rva(0x1300)+15] = b'GetTickCount\0'
    delta = base if delay_va else 0
    for at, value in [(0,0 if delay_va else 1),(4,0x1600+delta),(12,0x1500+delta),(16,0x1500+delta)]: word(rva(0x1400)+at, value)
    word(rva(0x1500), 0x1700, width)
    data[rva(0x1600):rva(0x1600)+11] = b'USER32.dll\0'
    data[rva(0x1700)+2:rva(0x1700)+15] = b'PeekMessageW\0'
    return data, directory, rva


class InventoryTests(unittest.TestCase):
    def test_pe32_pe64_import_names_ordinals_and_delay_descriptors(self):
        for bits in [32, 64]:
            data, _, _ = fixture(bits)
            pe = PE(data)
            self.assertEqual(pe.arch, 'x86' if bits == 32 else 'x64')
            self.assertEqual([(r['dll'],r['name']) for r in pe.imports()],
                             [('kernel32.dll','GetTickCount'),('kernel32.dll','#17')])
            self.assertEqual(pe.imports(True)[0]['name'], 'PeekMessageW')
            self.assertEqual(pe.imports(True)[0]['evidence'], 'delay_import')
            self.assertEqual(pe.imports()[1]['iat_rva'], 0x1200 + bits//8)
        self.assertEqual(PE(fixture(32, True)[0]).imports(True)[0]['name'], 'PeekMessageW')

    def test_truncated_unbacked_and_unterminated_import_tables_fail(self):
        data, _, rva = fixture()
        with self.assertRaises(ValueError): PE(data[:100])
        data[rva(0x1000)+12:rva(0x1000)+16] = (0x3000).to_bytes(4,'little')
        with self.assertRaises(ValueError): PE(data).imports()
        data, directory, rva = fixture()
        data[directory+12:directory+16] = (20).to_bytes(4,'little')
        self.assertEqual(len(PE(data).imports()),2) # terminator beyond advertised size
        data[rva(0x1000)+20+12:rva(0x1000)+20+16] = (0x3000).to_bytes(4,'little')
        with self.assertRaises(ValueError): PE(data).imports()

    def test_clr_implmap_pinvoke_is_read_from_metadata_not_guessed_strings(self):
        data, directory, rva = fixture()
        strings = b'\0kernel32.dll\0GetTickCount\0'
        table = bytearray(24)
        table[4:8] = bytes([2,0,0,1])
        struct.pack_into('<Q', table, 8, (1<<6)|(1<<26)|(1<<28))
        table += struct.pack('<III',1,1,1)
        table += bytes(14)  # MethodDef: 8+string+blob+Param
        table += struct.pack('<H',1)  # ModuleRef
        table += struct.pack('<HHHH',0x101,3,14,1)  # ImplMap
        metadata = bytearray(b'BSJB' + struct.pack('<HHII',1,1,0,4) + b'v4\0\0' + struct.pack('<HH',0,2))
        offset = len(metadata) + 12 + 20
        metadata += struct.pack('<II',offset,len(table)) + b'#~\0\0'
        metadata += struct.pack('<II',offset+len(table),len(strings)) + b'#Strings\0\0\0\0'
        metadata += table + strings
        struct.pack_into('<II',data,directory+14*8,0x1800,72)
        struct.pack_into('<III',data,rva(0x1800),72,0,0x1900)
        struct.pack_into('<I',data,rva(0x1800)+12,len(metadata))
        data[rva(0x1900):rva(0x1900)+len(metadata)] = metadata
        records = PE(data).pinvokes()
        self.assertEqual(len(records),1)
        self.assertEqual((records[0]['dll'],records[0]['name'],records[0]['member_token']),
                         ('kernel32.dll','GetTickCount',0x06000001))
        self.assertEqual(records[0]['evidence'],'pinvoke')

    def test_runtime_calls_queries_and_failures_keep_distinct_evidence(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root)/'trace.txt'
            path.write_text('#1 kernel32.dll!GetTickCount() = 0x1\n'
                '#2 user32.dll!Missing() = 0 [UNIMPLEMENTED]\n'
                '#3 kernel32.dll!GetProcAddress() = 1 ; export=Name("Sleep")\n'
                '  kernel32.dll!Sleep args=[1] -> 0x0\n'
                'probe64: hid.dll!HidD_GetHidGuid: unimplemented Win64 import\n'
                'completed\tkernel32.dll\tSleep\t8\n'
                'lookup\tkernel32.dll\tFlsAlloc\t3\n'
                'lookup\tguest.dll\tName\\tWith\\\\Escape\t1\n'
                'failed\tkernel32.dll\tSleep\t1\n')
            rows = trace_records(path)
            indexed = {(r['evidence'],r['dll'],r['name']):r for r in rows}
            self.assertEqual(indexed['runtime','user32.dll','Missing']['unimplemented'],1)
            self.assertEqual(indexed['runtime_lookup','?','Sleep']['count'],1)
            self.assertEqual(indexed['runtime','hid.dll','HidD_GetHidGuid']['unimplemented'],1)
            self.assertEqual(indexed['runtime','kernel32.dll','Sleep']['count'],9)
            self.assertEqual(indexed['runtime','kernel32.dll','Sleep']['unimplemented'],0)
            self.assertEqual(indexed['runtime','kernel32.dll','Sleep']['failed'],1)
            self.assertEqual(indexed['runtime_lookup','kernel32.dll','FlsAlloc']['count'],3)
            self.assertEqual(indexed['runtime_lookup','guest.dll','Name\tWith\\Escape']['count'],1)

    def test_renamed_pe_and_guest_runtime_queries_keep_provider_candidates(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            exe = root/'game.exe'
            exe.write_bytes(fixture()[0])
            (root/'renamed.plugin').write_bytes(fixture(64)[0])
            (root/'USER32.dll').write_bytes(fixture()[0])
            trace = root/'api.tsv'
            trace.write_text('lookup\tuser32.dll\tGuestExport\t1\n')
            game = scan_game('test', exe, [trace], set())
            self.assertEqual(len(game['binaries']), 3)
            lookup = next(r for r in game['records'] if r['evidence'] == 'runtime_lookup')
            self.assertEqual(lookup['provider'], 'bundled')
            self.assertEqual(lookup['local_provider_candidates'], ['USER32.dll'])


if __name__ == '__main__': unittest.main()
