#!/usr/bin/env python3
"""Frozen PE32 raw/static-IR comparisons with algebraic state and budget checks."""
import argparse,json,os,re,statistics,struct,subprocess,time,sys
from pathlib import Path
sys.dont_write_bytecode=True
from interpreter_perf import digest

def pe32(code):
    raw=(len(code)+511)//512*512;b=bytearray(512+raw)
    def u16(at,v):struct.pack_into('<H',b,at,v)
    def u32(at,v):struct.pack_into('<I',b,at,v)
    b[:2]=b'MZ';u32(0x3c,0x80);b[0x80:0x84]=b'PE\0\0';u16(0x84,0x14c);u16(0x86,1);u16(0x94,0xe0);u16(0x96,0x102)
    opt=0x98;u16(opt,0x10b);u32(opt+16,0x1000);u32(opt+28,0x400000);u32(opt+32,4096);u32(opt+36,512);u32(opt+56,8192);u32(opt+60,512);u32(opt+72,65536);u32(opt+76,4096);u32(opt+92,16)
    s=opt+0xe0;b[s:s+5]=b'.text';u32(s+8,len(code));u32(s+12,4096);u32(s+16,raw);u32(s+20,512);u32(s+36,0x60000020);b[512:512+len(code)]=code;return b

def fixture(name,work):
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
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',action='append',required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--pairs',type=int,default=3);p.add_argument('--work',type=int,default=30000000);p.add_argument('--cases',nargs='+',default=['sse-register','fp-scalar','bit-test','integer']);p.add_argument('--paths',nargs='+',default=['ir','raw']);a=p.parse_args()
    bins={n:Path(v).resolve(strict=True) for n,v in (s.split('=',1) for s in a.binary)};a.out.mkdir(parents=True,exist_ok=False)
    manifest=dict(binaries={n:dict(path=str(v),sha256=digest(v)) for n,v in bins.items()},script_sha256=digest(Path(__file__)),pairs=a.pairs,inputs={});results={}
    for case in a.cases:
        data,expected=fixture(case,a.work);exe=a.out/f'{case}.exe';exe.write_bytes(data);ir=a.out/f'{case}.wir'
        subprocess.run([str(next(iter(bins.values()))),'prepare',str(exe),'--out',str(ir)],check=True,stdout=subprocess.DEVNULL)
        manifest['inputs'][case]=dict(exe_sha256=digest(exe),ir_sha256=digest(ir),**expected);(a.out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
        for path in a.paths:
            runs=[]
            for pair in range(a.pairs):
                for name in list(bins) if pair%2==0 else list(reversed(bins)):
                    out=a.out/case/path/f'pair-{pair}-{name}';out.mkdir(parents=True)
                    cmd=[str(bins[name]),'run',str(exe),'--headless','--max-instr',str(expected['retired']+10),'--log-dir',str(out/'logs')]
                    if path=='ir':cmd+=['--ir',str(ir)]
                    with (out/'report.txt').open('w') as report:
                        t=time.perf_counter();proc=subprocess.Popen(cmd,stdout=report,stderr=subprocess.STDOUT);_,status,usage=os.wait4(proc.pid,0);proc.returncode=os.waitstatus_to_exitcode(status)
                    text=(out/'report.txt').read_text();regs=re.search(r'final regs: eax=(0x[0-9a-f]+)',text);count=re.search(r'instructions: (\d+)',text)
                    result=dict(pair=pair,binary=name,path=path,wall_s=time.perf_counter()-t,cpu_s=usage.ru_utime+usage.ru_stime,peak_rss_bytes=usage.ru_maxrss,exit_code=proc.returncode,command=cmd,eax=None if regs is None else int(regs[1],16),retired=None if count is None else int(count[1]))
                    result['validated']=bool('breakpoint' in text and result['eax']==expected['eax'] and result['retired']==expected['retired'])
                    (out/'run.json').write_text(json.dumps(result,indent=2)+'\n');runs.append(result);print(f'{case}/{path} pair={pair} {name} wall={result["wall_s"]:.4f} cpu={result["cpu_s"]:.4f} validated={result["validated"]}',flush=True)
                    if not result['validated']:raise RuntimeError(str(out)+'\n'+text)
            results[f'{case}/{path}']=dict(runs=runs,medians={n:{k:statistics.median(r[k] for r in runs if r['binary']==n) for k in ['wall_s','cpu_s','peak_rss_bytes']} for n in bins});(a.out/'compare.json').write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()
