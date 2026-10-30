// Generated MASKMOV inputs check byte selection, implicit addresses, precise
// faults and source authority on both cached and concrete memory paths.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine,Exit } from '../web/runtime.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const build=spawnSync('cargo',['build','--quiet','--locked','--offline','--lib','--manifest-path','crates/wire-wasm/Cargo.toml','--target','wasm32-unknown-unknown','--target-dir','target/wasm-rewrite'],{cwd:root,encoding:'utf8'});assert.equal(build.status,0,build.stderr);
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm'))),core=instance.exports;
const fpuLayout=Array.from(new Uint32Array(core.memory.buffer,core.wire_fpu_layout(),7)),stateLayout=Array.from(new Uint32Array(core.memory.buffer,core.wire_state_layout(),11)),base=0x10000n,data=0x20000n;
let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const bytes=(v,size=16)=>Uint8Array.from({length:size},(_,n)=>Number((v>>BigInt(n*8))&255n));
function program(code,mode=64){
 let handle;const calls={resolve:0,load:0,store:0,store_masked:0,begin:0,link:0},watch={onMemory:null,onLink:null};
 const imports={...core,wire_create(...args){handle=core.wire_create(...args);return handle;}};
 for(const name of Object.keys(calls))imports[`wire_${name}`]=(...args)=>{calls[name]++;if(name==='link')watch.onLink?.();if(['resolve','load','store','store_masked'].includes(name))watch.onMemory?.();return core[`wire_${name}`](...args);};
 const vm=new WasmMachine(imports,{mode,ip:base});vm.map(base,4096n,7);vm.write(base,[...code,0xcc]);return {vm,calls,watch,get handle(){return handle;}};
}
function fp(test,value){
 const pointer=core.wire_state(test.handle),view=new DataView(core.memory.buffer);
 if(value){core.wire_cancel_repeat(test.handle);new Uint8Array(core.memory.buffer,pointer+fpuLayout[3],128).set(value.regs);view.setUint16(pointer+fpuLayout[0],value.cw,true);view.setUint16(pointer+fpuLayout[1],value.sw,true);view.setUint16(pointer+fpuLayout[2],value.tags,true);}
 return {cw:view.getUint16(pointer+fpuLayout[0],true),sw:view.getUint16(pointer+fpuLayout[1],true),tags:view.getUint16(pointer+fpuLayout[2],true),regs:new Uint8Array(core.memory.buffer,pointer+fpuLayout[3],128).slice(),ip:view.getBigUint64(pointer+fpuLayout[4],true),dp:view.getBigUint64(pointer+fpuLayout[5],true),opcode:view.getUint16(pointer+fpuLayout[6],true)};
}
const seed=()=>({cw:0x37f,sw:0x6f35,tags:0x93a5,regs:Uint8Array.from({length:128},(_,n)=>(n*37+51)&255)});
const code=(mmx,mode,addressBits=mode,segment=[])=>[...segment,...(addressBits!==mode?[0x67]:[]),...(mmx?[]:[0x66]),0x0f,0xf7,0xc1];
function operands(test,mmx,source,mask){
 if(mmx){const before=seed();before.regs.set(source.slice(0,8),0);before.regs.set(mask.slice(0,8),16);return fp(test,before);}
 test.vm.xmm(0,source);test.vm.xmm(1,mask);return fp(test,seed());
}
const transitioned=before=>({...before,sw:before.sw&~0x3800,tags:0});
function read(test,address,length){const out=Uint8Array.from({length},(_,n)=>Number(core.wire_load(test.handle,address+BigInt(n),1,1)));assert.equal(core.wire_fault_kind(test.handle),0);return out;}
const source=Uint8Array.from({length:16},(_,n)=>(n*19+3)&255),old=Uint8Array.from({length:16},(_,n)=>(n*31+171)&255);
const maskBytes=(mask,size)=>Uint8Array.from({length:16},(_,n)=>(mask&(1<<n)?0x80:0)|(n<size?(n*13+7)&0x7f:0x7f));

for(const mmx of [true,false])for(const mode of [32,64])for(const addressBits of [mode,mode===64?32:16])for(const segment of [[],[0x64],[0x65]]){
 const test=program([0x04,1,...code(mmx,mode,addressBits,segment)],mode),{vm}=test,size=mmx?8:16,all=(1<<size)-1,masks=[0,all,0x55&all,0xaa&all,...Array.from({length:size},(_,n)=>1<<n),...Array.from({length:size},(_,n)=>all^(1<<n))];
 vm.map(data,8192n);vm.mxcsr=0x1f01;const low16=addressBits===16&&!segment.length;if(low16)vm.map(0n,8192n);
 for(const offset of [0,1,4096-size,4092,4095])for(const mask of masks){
  const at=(low16?0n:data)+BigInt(offset),index=segment.length?0n:at;
  if(segment.length){const field=segment[0]===0x64?6:7;new DataView(core.memory.buffer).setBigUint64(core.wire_state(test.handle)+stateLayout[field],at,true);}
  vm.write(at,old);const before=operands(test,mmx,source,maskBytes(mask,size));vm.ip=base;vm.flags=0x602n;vm.register(0,127n);vm.register(7,index|(addressBits===16?0x12340000n:addressBits===32&&mode===64?0x1234567800000000n:0n));
  const gpr=vm.register(7),work=vm.work,stores=test.calls.store_masked;
  test.watch.onMemory=()=>equal(fp(test),before,'memory calls leave resident CPU backing at entry');
  assert.equal((await vm.run(2n)).reason,Exit.Budget,`${mmx}/${mode}/${addressBits}/${segment}/${offset}/${mask}`);
  const expected=old.map((v,n)=>n<size&&(mask&(1<<n))?source[n]:v);equal(read(test,at,16),expected);equal(fp(test),mmx?transitioned(before):before);assert.equal(vm.flags,0xe92n);assert.equal(vm.register(7),gpr);assert.equal(vm.mxcsr,0x1f01);assert.equal(vm.work-work,2n);
  if(mask===0)assert.equal(test.calls.store_masked,stores);if(!mmx){equal(vm.xmm(0),source);equal(vm.xmm(1),maskBytes(mask,size));}
 }
 assert.equal(test.calls.resolve,0);assert.equal(test.calls.load,0);assert.equal(test.calls.store,0);vm.dispose();
}

for(const mmx of [true,false]){
 const test=program(code(mmx,64)),{vm}=test,size=mmx?8:16;vm.map(data,8192n);vm.write(data+4092n,old);vm.register(7,data+4092n);
 let before=operands(test,mmx,source,maskBytes(0xf,size));assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);
 assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),mmx?transitioned(before):before);equal(read(test,data+4092n,4),source.slice(0,4));
 assert.equal(core.wire_load(test.handle,data+4096n,1,1),0n);assert.equal(core.wire_fault_kind(test.handle),4,'masked-off guard remains unconsumed');assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);
 vm.ip=base;vm.write(data+4092n,old);before=operands(test,mmx,source,maskBytes((1<<size)-1,size));vm.flags=0x602n;vm.mxcsr=0x1f80;const work=vm.work,retired=vm.retired;assert.equal(core.wire_protect(test.handle,data+4096n,4096n,2),0);
 assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);equal(read(test,data+4092n,4),old.slice(0,4),'later write fault leaves earlier selected bytes intact');assert.equal(vm.flags,0x602n);assert.equal(vm.mxcsr,0x1f80);assert.equal(vm.work,work);assert.equal(vm.retired,retired);assert.equal(vm.ip,base);
 assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(read(test,data+4092n,16),old.map((v,n)=>n<size?source[n]:v));
 vm.ip=base;before=operands(test,mmx,source,maskBytes(0,size));vm.register(7,0xffffffffffffffffn);const calls=test.calls.store_masked;assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),mmx?transitioned(before):before);assert.equal(test.calls.store_masked,calls,'empty mask suppresses addressing faults');vm.dispose();
}

for(const mmx of [true,false]){
 const test=program(code(mmx,64)),{vm}=test,size=mmx?8:16,before=operands(test,mmx,source,maskBytes(1,size));before.cw=0x37e;fp(test,before);vm.map(data,4096n);vm.register(7,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 assert.equal((await vm.run(1n)).reason,mmx?Exit.Exception:Exit.Fault);equal(fp(test),before);assert.equal(test.calls.store_masked,mmx?0:1);if(mmx)assert.equal(core.wire_exception(test.handle),17);vm.dispose();
}

for(const mmx of [true,false]){
 const test=program(code(mmx,64)),{vm}=test,size=mmx?8:16;vm.map(data,4096n,2);vm.register(7,data);
 for(let run=0;run<2;run++){const before=operands(test,mmx,source,maskBytes(0x5555&((1<<size)-1),size));vm.ip=base;assert.equal((await vm.run(1n)).reason,Exit.Budget,'masked stores do not require destination read permission');equal(fp(test),mmx?transitioned(before):before);assert.equal(test.calls.store_masked,1);}
 assert.equal(core.wire_protect(test.handle,data,4096n,4),0);equal(read(test,data,16),source.map((v,n)=>n<size&&n%2===0?v:0));vm.dispose();
}

for(const mmx of [true,false])for(const mask of [0,1,16]){
 const test=program(code(mmx,64)),{vm}=test,size=mmx?8:16;vm.map(data,8192n,7);vm.write(data+4096n,[0xcc]);vm.ip=data+4096n;assert.equal(core.wire_compile(test.handle),0);const observed=core.wire_ticket(test.handle);vm.ip=base;vm.register(7,data+4092n);const before=operands(test,mmx,source,maskBytes(mask,size));
 assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),mmx?transitioned(before):before);vm.ip=data+4096n;assert.equal(core.wire_ready(test.handle,observed),mask===16?0:1,'only selected source pages are revoked');vm.dispose();
}
for(const mmx of [true,false]){
 const instruction=code(mmx,64),test=program([...instruction,0x04,1]),{vm}=test,size=mmx?8:16;vm.register(7,base+BigInt(instruction.length));const value=source.slice();value[0]=0xcc;const before=operands(test,mmx,value,maskBytes(1,size));vm.register(0,123n);
 assert.equal(core.wire_compile(test.handle),0);const ticket=core.wire_ticket(test.handle);assert.equal((await vm.run(10n)).reason,Exit.Unsupported);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.register(0),123n);assert.equal(vm.ip,base+BigInt(instruction.length));equal(fp(test),mmx?transitioned(before):before);assert.equal(core.wire_ready(test.handle,ticket),0);vm.dispose();
}

for(const mmx of [true,false]){
 const test=program([...code(mmx,64),0x48,0xf7,0xf1,...(mmx?[0x0f,0x2a,0xd0]:[0xf3,0x0f,0x58,0xc2])]),{vm}=test,size=mmx?8:16;vm.map(data,4096n);vm.register(7,data);vm.register(0,12n);vm.register(2,0n);vm.register(1,3n);
 const value=bytes(mmx?0x00000002ffffffffn:0x3f800000n),before=operands(test,mmx,value,maskBytes((1<<size)-1,size));vm.xmm(2,bytes(0x3f800000n));assert.equal((await vm.run(3n)).reason,Exit.Budget);assert.equal(vm.register(0),4n);equal(read(test,data,size),value.slice(0,size));equal(fp(test),mmx?transitioned(before):before);equal(vm.xmm(mmx?2:0),bytes(mmx?0x40000000bf800000n:0x40000000n));assert.equal(test.calls.store_masked,1);
 vm.ip=base;vm.register(0,12n);vm.register(2,0n);const stores=test.calls.store_masked;assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal(test.calls.store_masked,stores,'warm masked store uses inline byte stores');vm.dispose();
}

const high=program([0x66,0x45,0x0f,0xf7,0xc1]);high.vm.map(data,4096n);high.vm.register(7,data);high.vm.xmm(8,source);high.vm.xmm(9,maskBytes(0xaaaa,16));assert.equal((await high.vm.run(1n)).reason,Exit.Budget);equal(read(high,data,16),source.map((v,n)=>n%2?v:0));high.vm.dispose();

for(const mmx of [true,false]){
 const test=program([...(mmx?[]:[0x66]),0x0f,0xfc,0xc2,0xff,0xe2]),{vm}=test,size=mmx?8:16,target=0x30000n,value=Uint8Array.from({length:16},()=>1),mask=maskBytes(0xaaaa&((1<<size)-1),size);
 vm.map(data,4096n,7);vm.write(data,[...code(mmx,64),0x83,0xe9,1,0x74,2,0xff,0xe6,0xcc]);vm.map(target,4096n);vm.register(2,data);vm.register(6,base);vm.register(7,target);vm.register(1,2n);operands(test,mmx,value,mask);if(!mmx)vm.xmm(2,value);
 assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){
  const before=operands(test,mmx,value,mask);if(mmx){before.regs.set(value.slice(0,8),32);fp(test,before);}vm.ip=base;vm.register(1,BigInt(count));test.calls.begin=0;test.calls.link=0;test.calls.store_masked=0;
  test.watch.onLink=()=>{equal(fp(test),before);if(!mmx)equal(vm.xmm(0),value);};
  assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const out=value.map(x=>x+count);equal(read(test,target,16),out.map((x,n)=>n<size&&n%2?x:0));
  if(mmx){const expected=transitioned(before);expected.regs.set(out.slice(0,8),0);expected.regs.set([255,255,0,0,0,0,0,0],8);equal(fp(test),expected);}else{equal(vm.xmm(0),out);equal(fp(test),before);}
  assert.equal(test.calls.begin,1);assert.equal(test.calls.link,2);assert.equal(test.calls.store_masked,0);assert.deepEqual(vm.stats(),stats);
 }vm.dispose();
}
console.log(`${checks} masked-store checks passed: implicit/segment addresses, byte permissions, inline hits, precise faults, x87 transitions, and source revocation`);
