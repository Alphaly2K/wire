import assert from 'node:assert/strict';
import {fixture} from './pe-fixture.mjs';
const encode=(value,length)=>Uint8Array.from({length},(_,n)=>Number((value>>BigInt(n*8))&255n));

// A complete PE fixture creates/paints its own window, echoes stdin and waits for targeted input.
export function program(mode, message=0x8001) {
 const f=fixture(mode,{tls:false}),ptr=mode/8;f.relocations.splice(0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1400));f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));
 const imports=new Map();let nameAt=0x2300;
 for(const[group,[dll,names]]of[['user32.dll',['RegisterClassExW','CreateWindowExW','DefWindowProcW','UpdateWindow','GetMessageW']],['kernel32.dll',['GetStdHandle','ReadFile','WriteFile']]].entries()){
  const lookup=0x2080+group*0x80,iat=0x2180+group*0x80,desc=0x2000+group*20,dllAt=0x2500+group*32;
  f.rput(desc,lookup);f.rput(desc+12,dllAt);f.rput(desc+16,iat);f.rtext(dllAt,dll+'\0');
  for(const[n,name]of names.entries()){f.rtext(nameAt+2,name+'\0');f.rput(lookup+n*ptr,nameAt,ptr);f.rput(iat+n*ptr,nameAt,ptr);imports.set(name,iat+n*ptr);nameAt+=2+name.length+1;nameAt+=(nameAt&1);}
 }
 assert.ok(nameAt<0x2500);f.put(f.dirs+12,60);
 const text=0x3120;f.bytes.set(Uint8Array.from([...'Child\0'].flatMap(c=>[c.charCodeAt(0),0])),f.offset(text));
 f.rput(0x3000,mode===32?48:80);f.absolute(0x3000+(mode===32?20:24),f.preferred);f.absolute(0x3000+(mode===32?40:64),f.preferred+BigInt(text));f.rput(0x3000+(mode===32?32:48),6,ptr);
 let pc=0x1000;const emit=(...bytes)=>{f.bytes.set(bytes,f.offset(pc));pc+=bytes.length;};
 const absolute=(opcode,rva)=>{emit(...opcode);f.absolute(pc,f.preferred+BigInt(rva));pc+=ptr;};
 const rip=(opcode,rva)=>{emit(...opcode,...encode(BigInt.asUintN(32,BigInt(rva-pc-opcode.length-4)),4));};
 const load=(rva)=>mode===32?absolute([0xa1],rva):rip([0x48,0x8b,0x05],rva);
 const save=(rva)=>mode===32?absolute([0xa3],rva):rip([0x48,0x89,0x05],rva);
 const address=rva=>({address:rva}),memory=rva=>({memory:rva});
 const argument=(arg,register)=>{
  if(mode===32){if(arg.memory!==undefined)absolute([0xff,0x35],arg.memory);else if(arg.address!==undefined)absolute([0x68],arg.address);else emit(0x68,...encode(BigInt.asUintN(32,BigInt(arg)),4));return;}
  const op=register<4?[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][register]:[0x48,0xb8];
  if(arg.memory!==undefined){if(register<4)rip([[0x48,0x8b,0x0d],[0x48,0x8b,0x15],[0x4c,0x8b,0x05],[0x4c,0x8b,0x0d]][register],arg.memory);else load(arg.memory);}
  else if(arg.address!==undefined)absolute(op,arg.address);else emit(...op,...encode(BigInt.asUintN(64,BigInt(arg)),8));
  if(register>=4)emit(0x48,0x89,0x44,0x24,32+(register-4)*8);
 };
 const api=(name,args=[])=>{if(mode===32)for(const arg of args.toReversed())argument(arg);else args.forEach(argument);if(mode===32)absolute([0xff,0x15],imports.get(name));else rip([0xff,0x15],imports.get(name));};
 if(mode===64)emit(0x48,0x83,0xec,104);
 load(imports.get('DefWindowProcW'));save(0x3008);api('RegisterClassExW',[address(0x3000)]);
 api('CreateWindowExW',[0,address(text),address(text),0x10000000,10,20,16,12,0,0,f.preferred,0]);save(0x3080);
 api('UpdateWindow',[memory(0x3080)]);
 api('GetStdHandle',[0xfffffff6]);save(0x3090);api('ReadFile',[memory(0x3090),address(0x30c0),1,address(0x30c4),0]);
 api('GetStdHandle',[0xfffffff5]);save(0x3090);api('WriteFile',[memory(0x3090),address(0x30c0),1,address(0x30c4),0]);
 api('GetMessageW',[address(0x30d0),memory(0x3080),message,message]);
 if(mode===32)absolute([0xa1],0x30d8);else rip([0x8b,0x05],0x30e0);
 if(mode===64)emit(0x48,0x83,0xc4,104);emit(0xc3);assert.ok(pc<=0x1400);f.rebuildRelocations();return f.bytes;
}

