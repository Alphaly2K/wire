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


from .pe import PE
from .traces import CALL32, CALL64, FAULT, LOOKUP, trace_records, unescape_tsv


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
    for path in (Path(__file__).resolve().parents[2]/'src').rglob('*.rs'):
        known.update(re.findall(r'"([A-Za-z_][A-Za-z0-9_@?$]{2,127})"', path.read_text()))
    games = [scan_game(name, Path(path).resolve(), traces[name], known)
             for name, path in (value.split('=', 1) for value in args.game)]
    apply_coverage(games, args.coverage_helper)
    seconds = time.monotonic()-start
    write_reports(games, args.output, seconds)
    print(f'{len(games)} games, {sum(len(g["binaries"]) for g in games)} PEs, {seconds:.2f}s: {args.output/"summary.md"}')


if __name__ == '__main__':
    main()
