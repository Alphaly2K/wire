#!/usr/bin/env python3
"""Offline PE import/delay-import/CLR PInvoke inventory plus observed API traces.

Reads game files without executing or modifying them. String candidates are
explicitly unconfirmed; imports and ImplMap rows are declarations, not proof
that a particular path ran. The optional coverage helper checks ABI tables,
not behavioral completeness or optional graphics/input backend capabilities.
"""
import argparse
from collections import Counter, defaultdict
import csv
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import time


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


API_PREFIX = re.compile(r'^(?:Get|Set|Create|Delete|Destroy|Open|Close|Read|Write|Find|Load|Free|Is|Has|Co|Ro|Windows|Reg|Rtl|Nt|Virtual|Heap|Tls|Fls|BCrypt|Crypt|Path|HidD_|HidP_|SetupDi|XInput|Imm|WSA|time|joy|waveOut|Direct3D|D3D11|D3D12|SH|Enable|Disable|Register|Unregister|Query|Wait|Wake|Initialize|Interlocked|Exit|Sleep|Peek|Post|Send|Dispatch|Translate|Move|Adjust|Show|Monitor|Enum|MessageBox|MultiByte|WideChar|CompareString|LCMap)')
ASCII_STRING = re.compile(rb'(?<![A-Za-z0-9_])([A-Za-z_][A-Za-z0-9_@?$]{2,127})\x00')


def candidates(pe, known):
    found = {}
    for m in ASCII_STRING.finditer(pe.data):
        name = m[1].decode('ascii')
        if API_PREFIX.match(name) or (name in known and (name.startswith('_') or name in
            {'malloc', 'calloc', 'realloc', 'free', 'memcpy', 'memmove', 'memset', 'memcmp', 'strlen',
             'strcmp', 'strcpy', 'strncpy', 'sprintf', 'printf', 'fopen', 'fclose', 'fread', 'fwrite',
             'socket', 'connect', 'recv', 'send', 'select', 'closesocket'})):
            found.setdefault(name, m.start(1))
    return [{'dll': '?', 'name': name, 'evidence': 'string_candidate', 'file_offset': offset} for name, offset in sorted(found.items())]


CALL32 = re.compile(r'^#\d+\s+([^\s!]+)!([^\s(]+)\(')
CALL64 = re.compile(r'^\s+([^\s!]+)!([^\s]+) args=')
FAULT = re.compile(r'probe64: ([^\s!]+)!([^\s:]+):.*unimplemented')
LOOKUP = re.compile(r'(?:export=Name\("([^"\n]+)"\)|GetProcAddress lookup module=\S+ symbol=([^\s]+))')


def unescape_tsv(value):
    return re.sub(r'\\([\\trn])', lambda m: {'\\': '\\', 't': '\t', 'r': '\r', 'n': '\n'}[m[1]], value)


def trace_records(path):
    observed = defaultdict(lambda: {'count': 0, 'unimplemented': 0, 'failed': 0})
    lookups = Counter()
    with path.open(errors='replace') as stream:
        for line in stream:
            parts = line.rstrip('\n').split('\t')
            if len(parts) == 4 and parts[0] in ('completed','lookup','failed'):
                kind, dll, name, count = parts
                dll, name = unescape_tsv(dll), unescape_tsv(name)
                count = int(count)
                if count < 0: raise ValueError('negative runtime API count')
                if kind == 'lookup':
                    # Keep the provider when the full runtime inventory knows it.
                    observed.setdefault((dll.lower(), name, 'lookup'), {'count':0,'unimplemented':0,'failed':0})['count'] += count
                else:
                    value = observed[(dll.lower(), name)]
                    value['count' if kind == 'completed' else 'failed'] += count
                continue
            match = CALL32.match(line) or CALL64.match(line)
            if match:
                key = (match[1].lower(), match[2])
                observed[key]['count'] += 1
                observed[key]['unimplemented'] += '[UNIMPLEMENTED]' in line
            match = FAULT.search(line)
            if match:
                observed[(match[1].lower(), match[2])]['unimplemented'] += 1
                observed[(match[1].lower(), match[2])]['failed'] += 1
            match = LOOKUP.search(line)
            if match:
                lookups[match[1] or match[2]] += 1
    out = [{'dll': key[0], 'name': key[1], 'evidence': 'runtime_lookup' if len(key) == 3 else 'runtime', **value}
           for key, value in observed.items()]
    out += [{'dll': '?', 'name': name, 'evidence': 'runtime_lookup', 'count': count} for name, count in lookups.items()]
    return out


def scan_game(name, exe, traces, known):
    root = exe.parent
    files, warnings, records = [], [], []
    paths = []
    for path in sorted(root.rglob('*')):
        if not path.is_file(): continue
        try:
            with path.open('rb') as stream: mz = stream.read(2) == b'MZ'
        except OSError as error:
            warnings.append({'binary': str(path), 'phase': 'read_header', 'error': str(error)})
            continue
        if mz or path == exe: paths.append(path)
    for path in paths:
        try:
            data = path.read_bytes()
            pe = PE(data)
        except (OSError, ValueError, UnicodeError) as error:
            warnings.append({'binary': str(path), 'error': str(error)})
            continue
        def phase(label, action, default):
            try: return action()
            except (ValueError, UnicodeError) as error:
                warnings.append({'binary': str(path), 'phase': label, 'error': str(error)})
                return default
        imports = phase('imports', pe.imports, []) + phase('delay_imports', lambda: pe.imports(True), [])
        pinvokes = phase('pinvokes', pe.pinvokes, [])
        exports = phase('exports', pe.exports, {})
        relative = str(path.relative_to(root))
        row = {'path': relative, 'arch': pe.arch, 'sha256': hashlib.sha256(data).hexdigest(),
               'managed': bool(pe.directory(14)[0]), 'import_count': len(imports),
               'delay_count': sum(r['evidence'] == 'delay_import' for r in imports),
               'pinvoke_count': len(pinvokes), 'exports': exports}
        files.append(row)
        for r in imports + pinvokes + candidates(pe, known):
            records.append({**r, 'binary': relative, 'binary_arch': pe.arch})
    by_name = defaultdict(list)
    for row in files: by_name[Path(row['path']).name.lower()].append(row)
    graph = defaultdict(set)
    for r in records:
        if r['evidence'] not in ('import', 'delay_import', 'pinvoke'): continue
        local = by_name.get(r['dll'], [])
        if local:
            r['local_provider_candidates'] = [row['path'] for row in local]
            graph[r['binary']].update(r['local_provider_candidates'])
            r['provider'] = 'bundled'
        else: r['provider'] = 'external'
    reached, pending = set(), [exe.name]
    while pending:
        binary = pending.pop()
        if binary in reached: continue
        reached.add(binary)
        pending.extend(graph[binary] - reached)
    arch = next((f['arch'] for f in files if f['path'] == exe.name), 'unknown')
    for r in records:
        r['entry_import_reachable'] = r['binary'] in reached
        r['arch'] = arch if r['evidence'] == 'pinvoke' else r['binary_arch']
    for path in traces:
        for r in trace_records(path):
            local = by_name.get(r['dll'], [])
            records.append({**r, 'binary': f'trace:{path}', 'arch': arch,
                            'provider': 'bundled' if local else 'external',
                            **({'local_provider_candidates': [row['path'] for row in local]} if local else {})})
    # Export-forwarder dependencies are separate declarations. A local wrapper
    # can still need an external Windows implementation.
    for row in files:
        seen = set()
        for target in row['exports'].values():
            if not target or target in seen: continue
            seen.add(target)
            dll, sep, function = target.rpartition('.')
            if not sep: continue
            if not dll.lower().endswith('.dll'): dll += '.dll'
            records.append({'dll': dll.lower(), 'name': function, 'evidence': 'export_forwarder',
                            'binary': row['path'], 'arch': row['arch'],
                            'entry_import_reachable': row['path'] in reached,
                            'provider': 'bundled' if dll.lower() in by_name else 'external'})
    return {'game': name, 'exe': str(exe), 'arch': arch, 'binaries': files, 'warnings': warnings,
            'traces': [str(p) for p in traces], 'records': records}


def apply_coverage(games, helper):
    keys = sorted({(r['arch'], r['dll'], name) for g in games for r in g['records']
                   for name in r.get('binding_candidates', [r['name']])
                   if r['dll'] != '?' and r.get('provider') != 'bundled' and r['dll'].endswith('.dll')})
    coverage = {}
    if helper:
        result = subprocess.run([str(helper)], input=''.join('\t'.join(k)+'\n' for k in keys), text=True,
                                capture_output=True, check=True)
        for line in result.stdout.splitlines():
            arch, dll, name, status = line.split('\t')
            coverage[(arch, dll, name)] = status
    for game in games:
        for r in game['records']:
            status = coverage.get((r['arch'], r['dll'], r['name']), 'not_checked')
            if status == 'abi_missing' and any(coverage.get((r['arch'], r['dll'], name)) == 'abi_known'
                for name in r.get('binding_candidates', [])):
                status = 'abi_known_with_suffix'
            r['coverage'] = ('provider_unresolved' if r['dll'] == '?' else 'bundled_provider'
                if r.get('provider') == 'bundled' else status)


def write_reports(games, output, seconds):
    output.mkdir(parents=True, exist_ok=True)
    (output/'inventory.json').write_text(json.dumps({'elapsed_seconds': seconds, 'games': games}, ensure_ascii=False, indent=2)+'\n')
    columns = ['game','arch','binary','binary_arch','dll','name','evidence','provider','coverage',
               'entry_import_reachable','local_provider_candidates','iat_rva','file_offset',
               'member_token','mapping_flags','binding_candidates','count','unimplemented','failed']
    with (output/'inventory.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, columns, extrasaction='ignore')
        writer.writeheader()
        for game in games:
            for r in sorted(game['records'], key=lambda x: (x['dll'], x['name'], x['binary'], x['evidence'])):
                row = {'game': game['game'], **r}
                for column in ('local_provider_candidates', 'binding_candidates'):
                    if column in row: row[column] = json.dumps(row[column], ensure_ascii=False)
                writer.writerow(row)
    lines = ['# 游戏 API 批量清单', '', f'扫描耗时 {seconds:.2f} 秒。仅读取原始文件，未执行游戏。', '',
             '导入、延迟导入和 CLR ImplMap 是声明；字符串仅作候选。运行日志证明已尝试或完成的调用，',
             '计数仅覆盖所选日志，不是游戏总调用次数。`abi_known` 表示 ABI/分派登记，不保证全部参数或后端能力。', '',
             '| 游戏 | 架构 | PE 文件 | 普通导入 | 延迟导入 | P/Invoke | 字符串候选 | native 导入 ABI 缺失 |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    def unique(records, evidence): return {(r['dll'], r['name']) for r in records if r['evidence'] == evidence}
    for game in games:
        records = game['records']
        missing = {(r['dll'], r['name']) for r in records if r['coverage'] == 'abi_missing' and
                   r['evidence'] in ('import', 'delay_import') and
                   not next((b['managed'] for b in game['binaries'] if b['path'] == r['binary']), False)}
        lines.append(f"| {game['game']} | {game['arch']} | {len(game['binaries'])} | {len(unique(records,'import'))} | {len(unique(records,'delay_import'))} | {len(unique(records,'pinvoke'))} | {len(unique(records,'string_candidate'))} | {len(missing)} |")
    lines += ['', '## 已观测的调用与查询', '',
              '按 DLL+符号去重；含所选历史轨迹，不表示所有游戏分支。查询符号不等于调用该符号。',
              'guest DLL 查询和内部 COM gateway 保留在完整清单，下面仅计 external DLL 条目。', '',
              '| 游戏 | 已观测调用/尝试的符号 | 动态查询符号 | 调用/尝试中的 ABI 缺失 |',
              '| --- | ---: | ---: | ---: |']
    for game in games:
        external = [r for r in game['records'] if r.get('provider') == 'external' and r['dll'].endswith('.dll')]
        missing = {(r['dll'], r['name']) for r in external if r['evidence'] == 'runtime' and r['coverage'] == 'abi_missing'}
        lines.append(f"| {game['game']} | {len(unique(external,'runtime'))} | {len(unique(external,'runtime_lookup'))} | {len(missing)} |")
    lines += ['', '## 按库补齐的候选', '', '包含目录中全部 PE 的声明，可能包含未加载插件及可选回退。完整条目、来源、IAT RVA、',
              '文件偏移和入口导入可达标记见 CSV/JSON。游戏内的 guest DLL 已单列，未当成宿主 Win32 缺口。', '']
    priority = defaultdict(lambda: defaultdict(set))
    for game in games:
        native = {b['path'] for b in game['binaries'] if not b['managed']}
        for r in game['records']:
            if r['coverage'] == 'abi_missing' and ((r['evidence'] in ('import','delay_import','export_forwarder')
                and r['binary'] in native) or r['evidence'] == 'runtime'):
                priority[r['dll']][r['name']].add(game['game'])
    for dll, names in sorted(priority.items(), key=lambda p: (-len(p[1]), p[0])):
        lines += [f'### {dll}（{len(names)}）', '']
        lines += [f"- `{name}` — {', '.join(sorted(which))}" for name, which in sorted(names.items())]
        lines.append('')
    lines += ['托管框架的 P/Invoke 缺口保留在 CSV/JSON，避免把完整 BCL 的可选互操作当作启动前置条件。', '',
              '## 覆盖边界', '', '扫描目录内具有 MZ/PE 头的文件（包括改名插件），以及 CLR P/Invoke 声明。加密/拼接的动态函数名、',
              '间接加载的新 DLL、COM vtable 方法及尚未走到的分支需要补充运行轨迹或进一步反汇编。',
              '字符串候选不升级为已调用 API。ABI 缺失也不直接等同于启动必须成功的 API；可选设备须保留真实失败。', '']
    for game in games:
        if game['warnings']:
            lines += [f"解析警告：{game['game']}", '']
            lines += [f"- `{w['binary']}`：{w['error']}" for w in game['warnings']]
            lines.append('')
    (output/'summary.md').write_text('\n'.join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', action='append', required=True, metavar='NAME=EXE')
    parser.add_argument('--trace', action='append', default=[], metavar='NAME=PATH')
    parser.add_argument('--coverage-helper', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    start = time.monotonic()
    traces = defaultdict(list)
    for value in args.trace:
        name, path = value.split('=', 1)
        traces[name].append(Path(path).resolve())
    known = set()
    for path in (Path(__file__).resolve().parent.parent/'src').rglob('*.rs'):
        known.update(re.findall(r'"([A-Za-z_][A-Za-z0-9_@?$]{2,127})"', path.read_text()))
    games = [scan_game(name, Path(path).resolve(), traces[name], known)
             for name, path in (value.split('=', 1) for value in args.game)]
    apply_coverage(games, args.coverage_helper)
    seconds = time.monotonic()-start
    write_reports(games, args.output, seconds)
    print(f'{len(games)} games, {sum(len(g["binaries"]) for g in games)} PEs, {seconds:.2f}s: {args.output/"summary.md"}')


if __name__ == '__main__':
    main()
