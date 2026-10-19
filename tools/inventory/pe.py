"""Checked PE32/PE32+ imports, exports, delay imports and CLR declarations."""
import struct


class PE:
    def __init__(self, data):
        self.data = data
        if data[:2] != b'MZ':
            raise ValueError('missing MZ')
        nt = self.number(0x3c, 4)
        if self.slice(nt, 4) != b'PE\0\0':
            raise ValueError('missing PE')
        self.machine = self.number(nt + 4, 2)
        self.arch = {0x14c: 'x86', 0x8664: 'x64'}.get(self.machine, f'0x{self.machine:x}')
        count, opt_size = self.number(nt + 6, 2), self.number(nt + 20, 2)
        opt = nt + 24
        self.slice(opt, opt_size)
        magic = self.number(opt, 2)
        if magic not in (0x10b, 0x20b) or count > 96:
            raise ValueError('unsupported PE layout')
        self.width = 8 if magic == 0x20b else 4
        self.base = self.number(opt + (24 if self.width == 8 else 28), self.width)
        self.headers = self.number(opt + 60, 4)
        d = 112 if self.width == 8 else 96
        nd = min(self.number(opt + d - 4, 4), 16)
        if d + nd * 8 > opt_size:
            raise ValueError('directories outside optional header')
        self.dirs = [struct.unpack('<II', self.slice(opt + d + n * 8, 8)) for n in range(nd)]
        self.sections = []
        for n in range(count):
            s = opt + opt_size + n * 40
            self.slice(s, 40)
            virtual, rva, size, offset = struct.unpack('<IIII', self.slice(s + 8, 16))
            self.slice(offset, size)
            self.sections.append((rva, max(virtual, size), offset, size))

    def slice(self, offset, size):
        if offset < 0 or size < 0 or offset + size > len(self.data):
            raise ValueError(f'truncated PE at file offset {offset:#x}, size {size}')
        return self.data[offset:offset + size]

    def number(self, offset, width):
        return int.from_bytes(self.slice(offset, width), 'little')

    def offset(self, rva, size=1):
        if 0 <= rva and rva + size <= self.headers:
            self.slice(rva, size)
            return rva
        for start, _, raw, length in self.sections:
            if start <= rva and rva + size <= start + length:
                self.slice(raw + rva - start, size)
                return raw + rva - start
        raise ValueError(f'RVA {rva:#x} not backed by file data')

    def at(self, rva, width):
        return self.number(self.offset(rva, width), width)

    def string(self, rva):
        out = bytearray()
        for n in range(4096):
            b = self.at(rva + n, 1)
            if not b:
                return out.decode('ascii')
            out.append(b)
        raise ValueError('unterminated PE string')

    def directory(self, index):
        return self.dirs[index] if index < len(self.dirs) else (0, 0)

    def thunks(self, dll, table, iat, evidence):
        for n in range(65536):
            value = self.at(table + n * self.width, self.width)
            if not value:
                return
            name = f'#{value & 65535}' if value & (1 << (self.width * 8 - 1)) else self.string(value + 2)
            yield {'dll': dll.lower(), 'name': name, 'evidence': evidence, 'iat_rva': iat + n * self.width}
        raise ValueError('unterminated import thunk table')

    def imports(self, delay=False):
        rva, size = self.directory(13 if delay else 1)
        if not rva:
            return []
        stride = 32 if delay else 20
        out = []
        # Windows loaders use the DLL-name terminator. Some valid linkers leave
        # that terminator outside the advertised directory size.
        for n in range(4096):
            row = [self.at(rva + n * stride + i * 4, 4) for i in range(stride // 4)]
            if row[1 if delay else 3] == 0:
                return out
            if delay:
                if row[0] & ~1:
                    raise ValueError('unknown delay import attributes')
                def addr(value):
                    if value == 0 or row[0] & 1:
                        return value
                    if value < self.base:
                        raise ValueError('invalid VA delay import descriptor')
                    return value - self.base
                name, iat, table = addr(row[1]), addr(row[3]), addr(row[4])
                evidence = 'delay_import'
            else:
                table, _, _, name, iat = row
                evidence = 'import'
            out.extend(self.thunks(self.string(name), table or iat, iat, evidence))
        raise ValueError('unterminated import descriptor directory')

    def exports(self):
        rva, size = self.directory(0)
        if not rva:
            return {}
        first, count, names, functions, strings, ordinals = [self.at(rva + n, 4) for n in (16, 20, 24, 28, 32, 36)]
        if count > 65536 or names > 65536:
            raise ValueError('export table too large')
        out = {}
        for n in range(count):
            value = self.at(functions + n * 4, 4)
            if value:
                out[f'#{first + n}'] = self.string(value) if rva <= value < rva + size else None
        for n in range(names):
            ordinal = self.at(ordinals + n * 2, 2)
            if ordinal >= count:
                raise ValueError('export name ordinal outside function table')
            key = f'#{first + ordinal}'
            if key in out:
                out[self.string(self.at(strings + n * 4, 4))] = out[key]
        return out

    def pinvokes(self):
        cli, _ = self.directory(14)
        if not cli:
            return []
        metadata_size = self.at(cli + 12, 4)
        metadata = self.offset(self.at(cli + 8, 4), metadata_size)
        if self.slice(metadata, 4) != b'BSJB':
            raise ValueError('missing CLR metadata signature')
        version = self.number(metadata + 12, 4)
        pos = metadata + 16 + (version + 3) // 4 * 4
        streams = self.number(pos + 2, 2)
        pos += 4
        heaps = {}
        for _ in range(streams):
            offset, size = struct.unpack('<II', self.slice(pos, 8))
            if offset + size > metadata_size:
                raise ValueError('CLR stream outside metadata directory')
            end = self.data.find(b'\0', pos + 8, pos + 40)
            if end < 0:
                raise ValueError('unterminated CLR stream name')
            name = self.slice(pos + 8, end - pos - 8).decode('ascii')
            heaps[name] = self.slice(metadata + offset, size)
            pos = (end + 4) // 4 * 4
        table = heaps.get('#~', heaps.get('#-'))
        strings = heaps.get('#Strings')
        if table is None or strings is None or len(table) < 24:
            raise ValueError('missing CLR tables or string heap')
        def u(at, width):
            if at + width > len(table):
                raise ValueError('truncated CLR tables')
            return int.from_bytes(table[at:at + width], 'little')
        valid = u(8, 8)
        rows, pos = {}, 24
        for n in range(64):
            if valid & (1 << n):
                rows[n] = u(pos, 4)
                pos += 4
        if not rows.get(28):
            return []
        def index(t): return 4 if rows.get(t, 0) >= 65536 else 2
        def coded(bits, targets): return 4 if max((rows.get(t, 0) for t in targets), default=0) >= 1 << (16 - bits) else 2
        s, g, b = [4 if table[6] & mask else 2 for mask in (1, 2, 4)]
        td = coded(2, [2, 1, 27])
        md = coded(1, [6, 10])
        # ECMA-335 II.22 table row widths through ImplMap (0x1c).
        widths = [
            2+s+3*g, coded(2, [0, 26, 35, 1])+2*s, 4+2*s+td+index(4)+index(6), index(4),
            2+s+b, index(6), 8+s+b+index(8), index(8), 4+s, index(2)+td,
            coded(3, [2, 1, 26, 6, 27])+s+b, 2+coded(2, [4, 8, 23])+b,
            coded(5, [6,4,1,2,8,9,10,0,14,23,20,17,26,27,32,35,38,39,40,42,44,43])+coded(3, [6,10])+b,
            coded(1, [4,8])+b, 2+coded(2, [2,6,32])+b, 6+index(2), 4+index(4), b,
            index(2)+index(20), index(20), 2+s+td, index(2)+index(23), index(23), 2+s+b,
            2+index(6)+coded(1, [20,23]), index(2)+2*md, s, b, 2+coded(1, [4,6])+s+index(26),
        ]
        offsets = {}
        for n in range(29):
            offsets[n] = pos
            pos += rows.get(n, 0) * widths[n]
        if pos > len(table):
            raise ValueError('CLR rows exceed table stream')
        def string(i):
            if i >= len(strings): raise ValueError('CLR string heap index outside stream')
            end = strings.find(b'\0', i)
            if end < 0: raise ValueError('unterminated CLR heap string')
            return strings[i:end].decode('utf-8')
        modules = [string(u(offsets[26] + n*s, s)) for n in range(rows.get(26, 0))]
        out = []
        for n in range(rows[28]):
            at = offsets[28] + n * widths[28]
            flags = u(at, 2)
            member_width = coded(1, [4,6])
            member = u(at + 2, member_width)
            name = string(u(at + 2 + member_width, s))
            scope = u(at + 2 + member_width + s, index(26))
            if not 1 <= scope <= len(modules): raise ValueError('invalid CLR ModuleRef row')
            dll = modules[scope - 1].lower()
            if '.' not in dll and dll != '__internal': dll += '.dll'
            bindings = [name]
            if not flags & 1 and flags & 6 and not name.endswith(('A', 'W')):
                bindings.append(name + ('W' if flags & 6 in (4, 6) else 'A'))
            out.append({'dll': dll, 'name': name, 'evidence': 'pinvoke', 'mapping_flags': flags,
                        'binding_candidates': bindings,
                        'member_token': (0x06000000 if member & 1 else 0x04000000) | (member >> 1)})
        return out
