import {moduleFixture} from './module-fixture.mjs';

const encode=(value,width)=>Array.from({length:width},(_,n)=>Number((BigInt(value)>>BigInt(n*8))&255n));
export const trace=0x21000n;

export function dynamicFixture(mode,{id=1,...options}={}){
 const f=moduleFixture(mode,{dll:true,...options});
 const wrap=(at,original,offset,record)=>{
  const counters=trace+BigInt(id*16+offset),inc=address=>mode===32?[0xff,0x05,...encode(address,4)]:[0x48,0xb8,...encode(address,8),0xff,0x00];
  const prefix=mode===32?[0x8b,0x44,0x24,8,0x83,0xf8,1,0x75,0]:[0x83,0xfa,1,0x75,0],code=[...prefix,...inc(counters)];
  const jump=()=>code.push(0xe9,...encode(BigInt(original-at-code.length-5),4));jump();code[prefix.length-1]=code.length-prefix.length;
  code.push(...inc(counters+4n));
  if(record){const address=trace+0x400n;code.push(...(mode===32?[0xa1,...encode(address,4),0xc1,0xe0,4,0x83,0xc8,id,0xa3,...encode(address,4)]:[0x49,0xbb,...encode(address,8),0x41,0x8b,0x03,0xc1,0xe0,4,0x83,0xc8,id,0x41,0x89,0x03]));}
  jump();f.bytes.set(code,f.offset(at));
 };
 if(options.entry!==false){wrap(0x1180,0x1000,0,true);f.put(f.opt+16,0x1180);}
 if(options.tls){wrap(0x1200,0x1100,8,false);f.rput(0x2300,f.preferred+0x1200n,f.ptr);f.rput(0x2300+f.ptr,f.preferred+0x1200n,f.ptr);}
 return f;
}

export function nestedFixture(mode,{id=2,child='leaf.dll',attach=true}={}){
 const f=dynamicFixture(mode,{id,dependency:'kernel32.dll',tls:true});f.rtext(0x20e2,'LoadLibraryA\0');f.rtext(0x25d0,child+'\0');
 const kept=f.relocations.filter(rva=>rva<0x1000||rva>=0x1100);f.relocations.splice(0,f.relocations.length,...kept);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1080));
 const code=mode===32?[0x83,0x7c,0x24,8,1,0x75,0]:[0x83,0xfa,1,0x75,0];const jump=code.length-1;
 const relative=target=>encode(BigInt(target-0x1000-code.length-4),4);
 const absolute=value=>{const at=0x1000+code.length;code.push(...encode(value,4));f.relocations.push(at);};
 if(mode===32){code.push(0x68);absolute(f.preferred+0x25d0n);code.push(0xff,0x15);absolute(f.preferred+0x20a0n);code.push(0xa3);absolute(f.preferred+0x3000n);}
 else{code.push(0x48,0x83,0xec,40,0x48,0x8d,0x0d);code.push(...relative(0x25d0));code.push(0xff,0x15);code.push(...relative(0x20a0));code.push(0x48,0x83,0xc4,40,0x48,0x89,0x05);code.push(...relative(0x3000));}
 code.push(0xb8,attach?1:0,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3]));code[jump]=code.length-jump-1;code.push(0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3]));f.bytes.set(code,f.offset(0x1000));f.rebuildRelocations();return f;
}
