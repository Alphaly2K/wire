#!/usr/bin/env python3
"""Simulate bounded L0+victim stores over ordered diagnostic windows.

Windows start cold; report warm-half results as well as all requests. Eviction
records are evidence, not additional guest requests. Page generations validate
entries; execution permission/guards remain outside the replacement model.
"""
import argparse,json,re
from collections import OrderedDict
from pathlib import Path
MASK=(1<<64)-1

def l0_hash(pc):return (pc^(pc>>16)^(pc>>32))&131071

def l2_hash(pc,size):
    # Mix aligned and unaligned entrance addresses independently of the L0 xor.
    pc=((pc^(pc>>30))*0xbf58476d1ce4e5b9)&MASK
    pc=((pc^(pc>>27))*0x94d049bb133111eb)&MASK
    return (pc^(pc>>31))&(size-1)

def simulate(events,size,kind):
    l0={};l2={};lru=OrderedDict();stats=[0,0,0];warm=[0,0,0];evictions=0
    for n,(seq,source,pc,epoch,g,g2) in enumerate(events):
        key=(pc,g,g2);slot=l0_hash(pc)
        old=l0.get(slot)
        if old==key:level=0
        else:
            if size:
                loc=l2_hash(pc,size) if kind=='independent' else pc
                found=(l2.get(loc)==key) if kind=='independent' else lru.get(pc)==key
            else:found=False
            level=1 if found else 2
            if found:
                if kind=='independent':del l2[loc]
                else:del lru[pc]
            if old and size:
                if kind=='independent':l2[l2_hash(old[0],size)]=old
                else:
                    lru[old[0]]=old;lru.move_to_end(old[0])
                    if len(lru)>size:lru.popitem(last=False)
            l0[slot]=key;evictions+=old is not None
        stats[level]+=1
        if n>=len(events)//2:warm[level]+=1
    return dict(requests=len(events),L0=stats[0],reuse=stats[1],decode=stats[2],warm_half=dict(L0=warm[0],reuse=warm[1],decode=warm[2]),L0_evictions=evictions)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('report',type=Path);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
    windows={}
    for line in a.report.read_text().splitlines():
        if not line.startswith('AMD64 V2 request: '):continue
        seq,k,pc,epoch,g,g2=line.split(': ',1)[1].split(',');event=(int(seq),int(k)&127,int(pc,16),int(epoch),int(g),int(g2))
        if event[1] in (4,6):continue
        start=max(s for s in [0,10_000_000,100_000_000,1_000_000_000] if event[0]>=s)
        windows.setdefault(start,[]).append(event)
    results={str(s):{f'{kind}-{size}':simulate(e,size,kind) for kind,size in [('none',0),('LRU',256),('LRU',1024),('LRU',4096),('independent',4096),('independent',16384),('independent',32768)]} for s,e in windows.items()}
    a.out.write_text(json.dumps(dict(report=str(a.report),windows=results,limitations='Cold window starts; no host-cost/RSS model, no prediction of elapsed gain.'),indent=2)+'\n')
    print(json.dumps(results,indent=2))
if __name__=='__main__':main()
