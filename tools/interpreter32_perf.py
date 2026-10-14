#!/usr/bin/env python3
"""Frozen PE32 raw/static-IR comparisons with algebraic state and budget checks."""
import argparse,json,os,re,statistics,struct,subprocess,sys
from pathlib import Path
sys.dont_write_bytecode=True
from perf.measurement import digest, paired_order, run_command

def pe32(code):
    raw=(len(code)+511)//512*512;b=bytearray(512+raw)
    def u16(at,v):struct.pack_into('<H',b,at,v)
    def u32(at,v):struct.pack_into('<I',b,at,v)
    b[:2]=b'MZ';u32(0x3c,0x80);b[0x80:0x84]=b'PE\0\0';u16(0x84,0x14c);u16(0x86,1);u16(0x94,0xe0);u16(0x96,0x102)
    opt=0x98;u16(opt,0x10b);u32(opt+16,0x1000);u32(opt+28,0x400000);u32(opt+32,4096);u32(opt+36,512);u32(opt+56,8192);u32(opt+60,512);u32(opt+72,65536);u32(opt+76,4096);u32(opt+92,16)
    s=opt+0xe0;b[s:s+5]=b'.text';u32(s+8,len(code));u32(s+12,4096);u32(s+16,raw);u32(s+20,512);u32(s+36,0x60000020);b[512:512+len(code)]=code;return b

def guid_gateway_fixture(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 83ec10')
    loop = len(code)
    # CLSIDFromString(NULL, &stack_guid); stdcall consumes both arguments.
    code += bytes.fromhex('8d0424 50 6a00 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b0424 01d8 83c410 5b 5f cc')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<I', code, 0x180, 0x11d0)
    code[0x1c0:0x1ca] = b'ole32.dll\0'
    symbol = b'CLSIDFromString\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    image = pe32(code)
    struct.pack_into('<II', image, 0x98 + 96 + 8, 0x1100, 40)
    return image, dict(eax=0, retired=7 * n + 10)

def gdi_gateway_fixture(work):
    n = max(1, work // 7)
    code = bytearray.fromhex('57 56 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 6a00 ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('89c6')
    loop = len(code)
    # Stock 1x1 bitmap: GetPixel(memory_dc, 0, 0) returns black COLORREF.
    code += bytes.fromhex('6a00 6a00 56 ff15') + struct.pack('<I', 0x401194)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('56 ff15') + struct.pack('<I', 0x401198)
    code += bytes.fromhex('89d8 5b 5e 5f cc')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    for offset, target in [(0x180, 0x11d0), (0x184, 0x11f0), (0x188, 0x11b0)]:
        struct.pack_into('<I', code, offset, target)
    code[0x1c0:0x1ca] = b'gdi32.dll\0'
    for offset, symbol in [(0x1d2, b'CreateCompatibleDC\0'), (0x1f2, b'GetPixel\0'), (0x1b2, b'DeleteDC\0')]:
        code[offset:offset + len(symbol)] = symbol
    image = pe32(code)
    struct.pack_into('<II', image, 0x98 + 96 + 8, 0x1100, 40)
    return image, dict(eax=0, retired=7 * n + 14)

def version_gateway_fixture(work, nls=False):
    n = max(1, work // 4)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db')
    loop = len(code)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('89d8 5b 5f cc')
    code.extend(bytes(512 - len(code)))
    struct.pack_into('<I', code, 0x100, 0x1180)
    struct.pack_into('<I', code, 0x10c, 0x11c0)
    struct.pack_into('<I', code, 0x110, 0x1190)
    struct.pack_into('<I', code, 0x180, 0x11d0)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'GetACP\0' if nls else b'GetVersion\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    image = pe32(code)
    struct.pack_into('<II', image, 0x98 + 96 + 8, 0x1100, 40)
    value = 932 if nls else 0x1db10106
    return image, dict(eax=(n * value) & 0xffffffff, retired=4 * n + 7)

def nls_conversion_fixture(work):
    n = max(1, work // 11)
    code = bytearray.fromhex('57 53 bf') + struct.pack('<I', n)
    code += bytes.fromhex('31db 83ec04')
    loop = len(code)
    # Convert a terminated ASCII string including NUL into two UTF-16 units.
    code += bytes.fromhex('8d0424 6a02 50 6aff 68') + struct.pack('<I', 0x4011f0)
    code += bytes.fromhex('6a00 68') + struct.pack('<I', 932)
    code += bytes.fromhex('ff15') + struct.pack('<I', 0x401190)
    code += bytes.fromhex('01c3 4f')
    at = len(code)
    code += bytes.fromhex('0f85') + struct.pack('<i', loop - at - 6)
    code += bytes.fromhex('8b0424 01d8 83c404 5b 5f cc')
    code.extend(bytes(512 - len(code)))
    for offset, value in [(0x100, 0x1180), (0x10c, 0x11c0), (0x110, 0x1190), (0x180, 0x11d0)]:
        struct.pack_into('<I', code, offset, value)
    code[0x1c0:0x1cd] = b'kernel32.dll\0'
    symbol = b'MultiByteToWideChar\0'
    code[0x1d2:0x1d2 + len(symbol)] = symbol
    code[0x1f0:0x1f2] = b'A\0'
    image = pe32(code)
    struct.pack_into('<II', image, 0x98 + 96 + 8, 0x1100, 40)
    return image, dict(eax=(65 + 2 * n) & 0xffffffff, retired=11 * n + 10)

def fixture(name,work):
    if name=='gateway-module':
        from perf.fixtures.module_gateway import x86
        return x86(work,pe32)
    if name == 'gateway-nls':
        return version_gateway_fixture(work, nls=True)
    if name == 'nls-conversion':
        return nls_conversion_fixture(work)
    if name == 'gateway-version':
        return version_gateway_fixture(work)
    if name == 'gateway-guid':
        return guid_gateway_fixture(work)
    if name == 'gateway-gdi':
        return gdi_gateway_fixture(work)
    n=max(1,work//6);c=bytearray()
    if name=='sse-register':
        c+=bytes.fromhex('b801000000 660f6ec0 b803000000 660f6ec8 bf')+struct.pack('<I',n)
        loop=len(c);c+=bytes.fromhex('660ffec1 660ffec1 660ffac1 660f6fd0');epilogue=bytes.fromhex('660f7ed0');expected=(1+3*n)&0xffffffff;count=6*n+6
    elif name=='bit-test':
        c+=bytes.fromhex('b801000000 b903000000 bf')+struct.pack('<I',n);loop=len(c);c+=bytes.fromhex('0fbbc8 0fb3c8 0fabc8 0fa3c8');epilogue=b'';expected=9;count=6*n+3
    elif name=='fp-scalar':
        c+=bytes.fromhex('b80000c03f 660f6ec0 b80000803f 660f6ec8 bf')+struct.pack('<I',n);loop=len(c);c+=bytes.fromhex('f30f59c1 f30f5ad0 f20f5ada f30f2cc3');epilogue=b'';expected=1;count=6*n+5
    else:
        c+=bytes.fromhex('b801000000 b903000000 bf')+struct.pack('<I',n);loop=len(c);c+=bytes.fromhex('01c8 31cb 89c2 83c001');epilogue=b'';expected=1+4*n;count=6*n+3
    c+=bytes.fromhex('4f 0f85');c+=struct.pack('<i',loop-len(c)-4);c+=epilogue+bytes.fromhex('cc')
    return pe32(c),dict(eax=expected&0xffffffff,retired=count)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',action='append',required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--pairs',type=int,default=3);p.add_argument('--work',type=int,default=30000000);p.add_argument('--cases',nargs='+',default=['sse-register','fp-scalar','bit-test','integer']);p.add_argument('--paths',nargs='+',default=['ir','raw']);p.add_argument('--discard-api-log',action='store_true',help='send API trace writes to the null device, retaining formatting and counters');a=p.parse_args()
    bins={n:Path(v).resolve(strict=True) for n,v in (s.split('=',1) for s in a.binary)};a.out.mkdir(parents=True,exist_ok=False)
    manifest=dict(binaries={n:dict(path=str(v),sha256=digest(v)) for n,v in bins.items()},script_sha256=digest(Path(__file__)),measurement_sha256=digest(Path(__file__).parent/'perf'/'measurement.py'),pairs=a.pairs,api_log='discard' if a.discard_api_log else 'file',inputs={});results={}
    for case in a.cases:
        data,expected=fixture(case,a.work);exe=a.out/f'{case}.exe';exe.write_bytes(data);ir=a.out/f'{case}.wir'
        subprocess.run([str(next(iter(bins.values()))),'prepare',str(exe),'--out',str(ir)],check=True,stdout=subprocess.DEVNULL)
        manifest['inputs'][case]=dict(exe_sha256=digest(exe),ir_sha256=digest(ir),**expected);(a.out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
        for path in a.paths:
            runs=[]
            for pair in range(a.pairs):
                for name in paired_order(bins, pair):
                    out=a.out/case/path/f'pair-{pair}-{name}';out.mkdir(parents=True)
                    if a.discard_api_log:
                        (out/'logs').mkdir()
                        (out/'logs/api.log').symlink_to(os.devnull)
                    cmd=[str(bins[name]),'run',str(exe),'--headless','--max-instr',str(expected['retired']+10),'--log-dir',str(out/'logs')]
                    if path=='ir':cmd+=['--ir',str(ir)]
                    result = run_command(cmd, out/'report.txt')
                    text=(out/'report.txt').read_text();regs=re.search(r'final regs: eax=(0x[0-9a-f]+)',text);count=re.search(r'instructions: (\d+)',text)
                    result.update(pair=pair,binary=name,path=path,eax=None if regs is None else int(regs[1],16),retired=None if count is None else int(count[1]))
                    result['validated']=bool('breakpoint' in text and result['eax']==expected['eax'] and result['retired']==expected['retired'])
                    (out/'run.json').write_text(json.dumps(result,indent=2)+'\n');runs.append(result);print(f'{case}/{path} pair={pair} {name} wall={result["wall_s"]:.4f} cpu={result["cpu_s"]:.4f} validated={result["validated"]}',flush=True)
                    if not result['validated']:raise RuntimeError(str(out)+'\n'+text)
            results[f'{case}/{path}']=dict(runs=runs,medians={n:{k:statistics.median(r[k] for r in runs if r['binary']==n) for k in ['wall_s','cpu_s','peak_rss_bytes']} for n in bins});(a.out/'compare.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()
