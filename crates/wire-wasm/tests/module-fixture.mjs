import {fixture} from './pe-fixture.mjs';

export function moduleFixture(mode,{dll=false,dependency=null,seed=7,delta=0,tls=false,entry=true,attach=true,forward=null,ordinal=false,data=false}={}){
 const f=fixture(mode,{tls}),{bytes,ptr,preferred,put,rput,rtext,offset,absolute,relocations}=f;
 const kept=relocations.filter(rva=>rva>=0x2200);relocations.splice(0,relocations.length,...kept);bytes.fill(0x90,0x400,0x800);put(0x96,dll?0x2102:0x102,2);if(!entry)put(f.opt+16,0);
 if(!dependency){put(f.dirs+8,0);put(f.dirs+12,0);}else{
  bytes.fill(0,offset(0x20c0),offset(0x2200));rtext(0x20c0,dependency+'\0');rtext(0x20e2,(data?'Data':'GetValue')+'\0');rput(0x2080,ordinal?(1n<<BigInt(mode-1))|17n:0x20e0,ptr);
 }
 const emitter=start=>{
  let pc=start;const code=(...values)=>{bytes.set(values,offset(pc));pc+=values.length;};
  const abs=value=>{absolute(pc,value,4);pc+=4;};
  const rel=target=>{rput(pc,BigInt.asUintN(32,BigInt(target-pc-4)));pc+=4;};
  return {code,abs,rel,get pc(){return pc;}};
 };
 const store=(e,rva)=>{if(mode===32){e.code(0xa3);e.abs(preferred+BigInt(rva));}else{e.code(0x89,0x05);e.rel(rva);}};
 const call=e=>{e.code(0xff,0x15);if(mode===32)e.abs(preferred+0x20a0n);else e.rel(0x20a0);};
 const capture=(e,rva)=>{
  if(mode===32){for(let n=0;n<3;n++){e.code(0x8b,0x44,0x24,4*(n+1));store(e,rva+n*8);}}
  else{for(const [n,code] of [[0,[0x48,0x89,0x0d]],[1,[0x89,0x15]],[2,[0x4c,0x89,0x05]]]){e.code(...code);e.rel(rva+n*8);}}
 };
 const e=emitter(0x1000);
 if(dll){
  capture(e,0x3040);
  if(dependency){if(mode===64)e.code(0x48,0x83,0xec,0x28);call(e);if(mode===64)e.code(0x48,0x83,0xc4,0x28);if(delta)e.code(0x83,0xc0,delta);}else e.code(0xb8,seed,0,0,0);
  store(e,0x3000);e.code(0xff,0x05);if(mode===32)e.abs(preferred+0x3030n);else e.rel(0x3030);e.code(0xb8,attach?1:0,0,0,0);if(mode===32)e.code(0xc2,12,0);else e.code(0xc3);
  const value=emitter(0x1080);if(mode===32){value.code(0xa1);value.abs(preferred+0x3000n);}else{value.code(0x8b,0x05);value.rel(0x3000);}value.code(0xc3);
  put(f.dirs,0x2500);put(f.dirs+4,0x100);rput(0x2510,17);rput(0x2514,2);rput(0x2518,2);rput(0x251c,0x2540);rput(0x2520,0x2550);rput(0x2524,0x2560);rput(0x2540,forward?0x25b0:0x1080);rput(0x2544,0x3000);rput(0x2550,0x2570);rput(0x2554,0x2580);rput(0x2560,0,2);rput(0x2562,1,2);rtext(0x2570,'GetValue\0');rtext(0x2580,'Data\0');if(forward)rtext(0x25b0,forward+'\0');
 }else{
  if(data){if(mode===32){e.code(0xa1);e.abs(preferred+0x20a0n);}else{e.code(0x48,0x8b,0x05);e.rel(0x20a0);}e.code(0x8b,0);}
  else{if(mode===64)e.code(0x48,0x83,0xec,0x28);call(e);if(mode===64)e.code(0x48,0x83,0xc4,0x28);}e.code(0xc3);
 }
 if(tls){
  const t=emitter(0x1100);
  capture(t,0x3060);
  if(mode===32){t.code(0xa1);t.abs(preferred+0x300cn);t.code(0x64,0x8b,0x15,0x2c,0,0,0,0x8b,0x04,0x82,0x8b,0);}
  else{t.code(0x8b,0x0d);t.rel(0x300c);t.code(0x65,0x48,0x8b,0x04,0x25,0x58,0,0,0,0x48,0x8b,0x04,0xc8,0x8b,0);}
  store(t,0x3010);t.code(0xff,0x05);if(mode===32)t.abs(preferred+0x3020n);else t.rel(0x3020);
  if(mode===32)t.code(0xc2,12,0);else t.code(0xc3);rput(0x2400,0x11110000+seed);
 }
 f.rebuildRelocations();return f;
}
