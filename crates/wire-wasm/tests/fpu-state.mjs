// Independent FP images exercise logical stack slots, physical tags, metadata,
// non-waiting controls, and the resident state boundary without an x86 executor.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine,Exit } from '../web/runtime.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const build=spawnSync('cargo',['build','--quiet','--locked','--offline','--lib','--manifest-path','crates/wire-wasm/Cargo.toml','--target','wasm32-unknown-unknown','--target-dir','target/wasm-rewrite'],{cwd:root,encoding:'utf8'});assert.equal(build.status,0,build.stderr);
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm'))),core=instance.exports;
const layout=Array.from(new Uint32Array(core.memory.buffer,core.wire_fpu_layout(),7)),stateLayout=Array.from(new Uint32Array(core.memory.buffer,core.wire_state_layout(),11)),base=0x10000n,data=0x20000n;
const mask=bits=>(1n<<BigInt(bits))-1n,encode=(v,size=16)=>Uint8Array.from({length:size},(_,n)=>Number((v>>BigInt(n*8))&255n));
let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
function program(code,mode=64){
 let handle;const calls={resolve:0,load:0,store:0,fp_area:0,fp_commit:0,link:0,begin:0},watch={onLink:null,onArea:null};
 const imports={...core,wire_create(...args){handle=core.wire_create(...args);return handle;}};
 for(const name of Object.keys(calls))imports[`wire_${name}`]=(...args)=>{
  calls[name]++;if(name==='link')watch.onLink?.();
  const pointer=core.wire_state(handle),before=new Uint8Array(core.memory.buffer,pointer,stateLayout[0]).slice(),result=core[`wire_${name}`](...args);
  if(name==='fp_area')watch.onArea?.(result,...args);
  if(['fp_area','fp_commit'].includes(name))assert.deepEqual(new Uint8Array(core.memory.buffer,pointer,stateLayout[0]),before,'image transfers do not transact CPU backing');return result;
 };
 const vm=new WasmMachine(imports,{mode,ip:base});vm.map(base,4096n,7);vm.write(base,[...code,0xcc]);return {vm,calls,watch,get handle(){return handle;}};
}
function fp(test,value){
 const pointer=core.wire_state(test.handle),view=new DataView(core.memory.buffer);
 if(value){core.wire_cancel_repeat(test.handle);for(const [name,n] of [['cw',0],['sw',1],['tags',2],['opcode',6]])view.setUint16(pointer+layout[n],value[name],true);for(const [name,n] of [['ip',4],['dp',5]])view.setBigUint64(pointer+layout[n],value[name],true);for(let n=0;n<8;n++){view.setBigUint64(pointer+layout[3]+n*16,value.regs[n]&mask(64),true);view.setBigUint64(pointer+layout[3]+n*16+8,value.regs[n]>>64n,true);}}
 return {cw:view.getUint16(pointer+layout[0],true),sw:view.getUint16(pointer+layout[1],true),tags:view.getUint16(pointer+layout[2],true),regs:Array.from({length:8},(_,n)=>view.getBigUint64(pointer+layout[3]+n*16,true)|(view.getBigUint64(pointer+layout[3]+n*16+8,true)<<64n)),ip:view.getBigUint64(pointer+layout[4],true),dp:view.getBigUint64(pointer+layout[5],true),opcode:view.getUint16(pointer+layout[6],true)};
}
function xmm(test,index,value){const pointer=core.wire_state(test.handle)+stateLayout[9]+index*16;if(value)new Uint8Array(core.memory.buffer,pointer,16).set(value);return new Uint8Array(core.memory.buffer,pointer,16).slice();}
const values=[0n,0x80000000000000000000n,0x3fff8000000000000000n,0x7fff8000000000000000n,0x7fffc123456789abcdefn,0x3fff0000000000000001n,0x00008000000000000000n,1n];
const seed=()=>({cw:0x37f,sw:0x6f35,tags:0x93a5,regs:values.map((value,n)=>value|(BigInt(0xaabbccdd0000+n)<<80n)),ip:0x1234567887654321n,dp:0xfedcba9876543210n,opcode:0x725});
const vector=n=>Uint8Array.from({length:16},(_,i)=>(n*29+i*13+7)&255);
const vectors=()=>Array.from({length:16},(_,n)=>vector(n));
const setVectors=(test,values)=>values.forEach((value,n)=>xmm(test,n,value));
function read(test,address,length){return Uint8Array.from({length},(_,n)=>{const v=core.wire_load(test.handle,address+BigInt(n),1,1);assert.equal(core.wire_fault_kind(test.handle),0);return Number(v);});}
const init=before=>({...before,cw:0x37f,sw:0,tags:0xffff,ip:0n,dp:0n,opcode:0});
const control=cw=>(cw&0x1f3f)|0x40;
const summary=(cw,sw)=>(sw&~0x8080)|((sw&~cw&0x3f)?0x8080:0);
const rotate=(before,delta)=>({...before,sw:(before.sw&~0x3a00)|((((before.sw>>11)+delta)&7)<<11)});

for(const mode of [32,64])for(const [bytes,kind] of [[[0xdb,0xe3],'init'],[[0xdb,0xe2],'clear'],[[0xd9,0xd0],'nop'],[[0xd9,0xf7],'inc'],[[0xd9,0xf6],'dec'],[[0x9b],'wait']]){
 const test=program([0x04,1,...bytes],mode),{vm}=test;
 for(let top=0;top<8;top++){
  const before=seed();before.sw=(before.sw&~0x3800)|(top<<11);fp(test,before);vm.ip=base;vm.flags=0x602n;vm.register(0,127n);vm.mxcsr=0x1f01;const oldVectors=vectors();setVectors(test,oldVectors);
  assert.equal((await vm.run(2n)).reason,Exit.Budget);let expected=before;
  if(kind==='init')expected=init(before);if(kind==='clear')expected={...before,sw:before.sw&0x7f00};
  if(kind==='nop')expected={...before,ip:base+2n,opcode:0x1d0};
  if(kind==='inc'||kind==='dec')expected={...rotate(before,kind==='inc'?1:7),ip:base+2n,opcode:kind==='inc'?0x1f7:0x1f6};
  equal(fp(test),expected,`${kind}/${mode}/${top}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f01);oldVectors.forEach((v,n)=>equal(xmm(test,n),v));
 }vm.dispose();
}
for(const mode of [32,64])for(const pop of [false,true])for(let index=0;index<8;index++){
 const test=program([0x2e,pop?0xdf:0xdd,0xc0+index],mode),{vm}=test;
 for(let top=0;top<8;top++){
  const before=seed();before.sw=(before.sw&~0x3800)|(top<<11);fp(test,before);vm.ip=base;const tags=before.tags|(3<<(2*((top+index)&7)))|(pop?3<<(2*top):0),expected={...(pop?rotate(before,1):before),tags,ip:base,opcode:(pop?0x7c0:0x5c0)|index};
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected);assert.equal(vm.flags,0x202n);
 }vm.dispose();
}

for(const mode of [32,64])for(const [code,field] of [[[0xd9,0x3b],'cw'],[[0xdd,0x3b],'sw'],[[0xdf,0xe0],'ax']]){
 const test=program([0x04,1,...code],mode),{vm}=test;vm.map(data,8192n);
 for(const offset of [0,1,4095]){
  const before=seed();before.cw=0x37e;fp(test,before);vm.ip=base;vm.register(3,data+BigInt(offset));const initial=mode===64?0x123456789abc007fn:0x9abc007fn;vm.register(0,initial);vm.flags=0x602n;
  assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),before);assert.equal(vm.flags,0xe92n);
  if(field==='ax')assert.equal(vm.register(0),(initial&~0xffffn)|BigInt(before.sw));else assert.equal(core.wire_load(test.handle,data+BigInt(offset),2,1),BigInt(before[field]));
 }vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xd9,0x2b,0x9b],mode),{vm}=test;vm.map(data,8192n);vm.register(3,data+4095n);
 for(const cw of [0,0x37f,0x37e,0x77f,0xb7f,0xf7f,0xffff]){
  const before=seed();fp(test,before);vm.ip=base;vm.write(data+4095n,encode(BigInt(cw),2));const work=vm.work;
  const expected={...before,cw:control(cw),sw:summary(cw,before.sw)};
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected);assert.equal(vm.work-work,1n);
  const pending=!!(before.sw&~cw&0x3f);assert.equal((await vm.run(1n)).reason,pending?Exit.Exception:Exit.Budget);equal(fp(test),expected);if(pending){assert.equal(core.wire_exception(test.handle),17);assert.equal(vm.ip,base+2n);}
 }vm.dispose();
}
for(const code of [[0x9b],[0xd9,0xd0],[0xd9,0xf7],[0xd9,0xf6],[0xdd,0xc3],[0xdf,0xc3],[0xd9,0x2b]]){
 const test=program([0x04,1,...code]),{vm}=test,before=seed();before.cw=0x37e;fp(test,before);vm.register(0,127n);vm.flags=0x602n;vm.register(3,data);vm.map(data,4096n);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 assert.equal((await vm.run(10n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),17);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.resolve,0);equal(fp(test),before);vm.dispose();
}
for(const [code,write] of [[[0xd9,0x2b],false],[[0xd9,0x3b],true],[[0xdd,0x3b],true]]){
 const test=program(code),{vm}=test,before=seed();fp(test,before);vm.map(data,8192n);vm.write(data+4095n,[0xff,0x0f]);vm.register(3,data+4095n);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,write?2:1),0);
 assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);assert.equal(vm.work,0n);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);equal(read(test,data+4095n,2),Uint8Array.of(0xff,0x0f));assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.dispose();
}
const waitInit=program([0x9b,0xdb,0xe3]),waitBefore=seed();waitBefore.cw=0x37e;fp(waitInit,waitBefore);assert.equal((await waitInit.vm.run(2n)).reason,Exit.Exception);equal(fp(waitInit),waitBefore);assert.equal(waitInit.vm.work,0n);waitBefore.cw=0x37f;fp(waitInit,waitBefore);assert.equal((await waitInit.vm.run(1n)).reason,Exit.Budget);equal(fp(waitInit),waitBefore);assert.equal((await waitInit.vm.run(1n)).reason,Exit.Budget);equal(fp(waitInit),init(waitBefore));waitInit.vm.dispose();
const clearWait=program([0xdb,0xe2,0x9b]),clearBefore=seed();clearBefore.cw=0x37e;fp(clearWait,clearBefore);assert.equal((await clearWait.vm.run(2n)).reason,Exit.Budget);equal(fp(clearWait),{...clearBefore,sw:clearBefore.sw&0x7f00});clearWait.vm.dispose();

const fx=(restore,wide)=>[...(wide?[0x48]:[]),0x0f,0xae,restore?0x0b:0x03];
const compact=tags=>Array.from({length:8},(_,n)=>(tags>>(2*n)&3)!==3?1<<n:0).reduce((a,b)=>a|b,0);
function image(state,xm,mode,wide){
 const bytes=new Uint8Array(512).fill(0xa5),view=new DataView(bytes.buffer);bytes.fill(0,0,464);
 view.setUint16(0,state.cw,true);view.setUint16(2,state.sw,true);bytes[4]=compact(state.tags);view.setUint16(6,state.opcode&0x7ff,true);view.setBigUint64(8,wide?state.ip:state.ip&mask(32),true);view.setBigUint64(16,wide?state.dp:state.dp&mask(32),true);view.setUint32(24,0x9fc1,true);view.setUint32(28,0xffff,true);
 for(let slot=0;slot<8;slot++)bytes.set(encode(state.regs[((state.sw>>11)+slot)&7]&mask(80),10),32+slot*16);
 for(let n=0;n<(mode===64?16:8);n++)bytes.set(xm[n],160+n*16);return bytes;
}
function decoded(bytes,old,oldXmm,mode,wide){
 const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),sw=view.getUint16(2,true),regs=Array.from({length:8},(_,physical)=>{const slot=(physical+8-(sw>>11))&7;return Array.from(bytes.slice(32+slot*16,42+slot*16)).reduceRight((a,b)=>(a<<8n)|BigInt(b),0n);});
 const tags=regs.reduce((tags,value,n)=>{const exp=Number(value>>64n)&0x7fff,significand=value&mask(64),tag=!(bytes[4]&(1<<n))?3:exp===0&&significand===0n?1:exp>0&&exp<0x7fff&&!!(significand>>63n)?0:2;return tags|(tag<<(n*2));},0);
 const cw=control(view.getUint16(0,true));return {fp:{...old,cw,sw:summary(cw,sw),tags,regs,ip:wide?view.getBigUint64(8,true):BigInt(view.getUint32(8,true)),dp:wide?view.getBigUint64(16,true):BigInt(view.getUint32(16,true)),opcode:view.getUint16(6,true)&0x7ff},xmm:oldXmm.map((v,n)=>n<(mode===64?16:8)?bytes.slice(160+n*16,176+n*16):v),mxcsr:view.getUint32(24,true)};
}
for(const [mode,wide] of [[32,false],[64,false],[64,true]])for(const restore of [false,true]){
 const test=program(fx(restore,wide),mode),{vm}=test;vm.map(data,8192n);
 for(let top=0;top<8;top++)for(const tags of [0xffff,0,0x5555,0xaaaa,0x93a5])for(const offset of [0,3808]){
  const before=seed(),xm=vectors();before.sw=(before.sw&~0x3800)|(top<<11);before.tags=tags;before.cw=0x37e;fp(test,before);setVectors(test,xm);vm.mxcsr=0x9fc1;vm.ip=base;vm.register(3,data+BigInt(offset));vm.flags=0x602n;const work=vm.work;
  const bytes=image(before,xm,mode,wide);
  if(restore){bytes[6]=0xff;bytes[7]=0xff;for(let n=0;n<8;n++)bytes.fill(0xde,42+n*16,48+n*16);vm.write(data+BigInt(offset),bytes);fp(test,seed());setVectors(test,Array.from({length:16},(_,n)=>vector(n+17)));vm.mxcsr=0x1f80;}
  else vm.write(data+BigInt(offset),new Uint8Array(512).fill(0xa5));
  const original=fp(test),oldXmm=Array.from({length:16},(_,n)=>xmm(test,n));assert.equal((await vm.run(1n)).reason,Exit.Budget,`${mode}/${wide}/${restore}/${top}/${tags}/${offset}`);assert.equal(vm.work-work,1n);assert.equal(vm.flags,0x602n);
  if(restore){const expected=decoded(bytes,original,oldXmm,mode,wide);equal(fp(test),expected.fp);expected.xmm.forEach((v,n)=>equal(xmm(test,n),v));assert.equal(vm.mxcsr,expected.mxcsr);}
  else{equal(fp(test),before);equal(read(test,data+BigInt(offset),512),bytes);xm.forEach((v,n)=>equal(xmm(test,n),v));}
 }
 assert.equal(test.calls.resolve,0);assert.equal(test.calls.load,0);assert.equal(test.calls.store,0);assert.equal(test.calls.fp_area,80);assert.equal(test.calls.fp_commit,restore?0:80);vm.dispose();
}

for(const [cw,sw] of [[0x37f,0x8081],[0x37e,1],[0x37f,0x8000],[0x37f,0x80],[0xffc0,0xffff]]){
 const test=program([...fx(true,true),0x9b]),{vm}=test,source={...seed(),cw,sw},bytes=image(source,vectors(),64,true);vm.map(data,4096n);vm.write(data,bytes);vm.register(3,data);const before=fp(test),expected=decoded(bytes,before,vectors(),64,true).fp;assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,'FXRSTOR normalizes reserved CW bits and exception summary');assert.equal((await vm.run(1n)).reason,(sw&~cw&63)?Exit.Exception:Exit.Budget);equal(fp(test),expected);vm.dispose();
}

for(const restore of [false,true])for(const offset of [0,3808]){
 const test=program(fx(restore,true)),{vm}=test,before=seed(),xm=vectors();fp(test,before);setVectors(test,xm);vm.mxcsr=0x1f80;vm.map(data,8192n);vm.register(3,data+BigInt(offset));const bytes=image(seed(),vectors(),64,true);new DataView(bytes.buffer).setUint32(24,0x10000,true);vm.write(data+BigInt(offset),restore?bytes:new Uint8Array(512).fill(0xa5));
 const page=offset?data+4096n:data;assert.equal(core.wire_protect(test.handle,page,4096n,restore?1:2),0);assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);xm.forEach((v,n)=>equal(xmm(test,n),v));assert.equal(vm.mxcsr,0x1f80);assert.equal(vm.ip,base);assert.equal(vm.work,0n);assert.equal(vm.retired,0n);
 assert.equal(core.wire_protect(test.handle,page,4096n,4),0);if(!restore)equal(read(test,data+BigInt(offset),512),new Uint8Array(512).fill(0xa5));assert.equal((await vm.run(1n)).reason,restore?Exit.Exception:Exit.Budget);if(restore){assert.equal(core.wire_exception(test.handle),14);equal(fp(test),before);xm.forEach((v,n)=>equal(xmm(test,n),v));assert.equal(vm.mxcsr,0x1f80);}vm.dispose();
}
for(const restore of [false,true]){
 const test=program(fx(restore,true)),{vm}=test,before=seed();before.cw=0x37e;fp(test,before);vm.map(data,4096n);vm.register(3,data+1n);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),14);assert.equal(test.calls.fp_area,0);equal(fp(test),before);vm.dispose();
 const empty=program(fx(restore,true)),emptyBefore=seed();fp(empty,emptyBefore);assert.equal((await empty.vm.run(0n)).reason,Exit.Budget);equal(fp(empty),emptyBefore);assert.equal(empty.calls.fp_area,0);empty.vm.dispose();
}
const saved=program(fx(false,true));saved.vm.map(data,4096n,2);saved.vm.register(3,data);fp(saved,seed());assert.equal((await saved.vm.run(1n)).reason,Exit.Budget);assert.equal(core.wire_protect(saved.handle,data,4096n,4),0);const savedExpected=image(seed(),Array.from({length:16},()=>new Uint8Array(16)),64,true);new DataView(savedExpected.buffer).setUint32(24,0x1f80,true);equal(read(saved,data,464),savedExpected.slice(0,464));saved.vm.dispose();

for(const [mode,wide] of [[32,false],[64,false],[64,true]])for(const restore of [false,true]){
 const test=program(fx(restore,wide),mode),{vm}=test,length=restore?(mode===64?416:288):464,address=data+4096n-BigInt(length),before=seed(),xm=vectors(),bytes=image(before,xm,mode,wide);vm.map(data,8192n);vm.write(address,bytes);vm.register(3,address);fp(test,before);setVectors(test,xm);vm.mxcsr=0x9fc1;assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);
 assert.equal((await vm.run(1n)).reason,Exit.Budget,'unused image tail does not access the next page');equal(fp(test),restore?decoded(bytes,before,xm,mode,wide).fp:before);assert.equal(core.wire_load(test.handle,data+4096n,1,1),0n);assert.equal(core.wire_fault_kind(test.handle),4);vm.dispose();
}

const revoked=program([...fx(false,true),0x04,1]);fp(revoked,seed());revoked.vm.register(0,123n);revoked.vm.register(3,base+16n);assert.equal(core.wire_compile(revoked.handle),0);const ticket=core.wire_ticket(revoked.handle);assert.equal((await revoked.vm.run(10n)).reason,Exit.Unsupported);assert.equal(revoked.vm.work,2n);assert.equal(revoked.vm.retired,2n);assert.equal(revoked.vm.register(0),124n);assert.equal(core.wire_ready(revoked.handle,ticket),0);revoked.vm.dispose();

const resident=program([0xdb,0xe3,0x0f,0xfc,0xc1,0xff,0xe2]),{vm}=resident,target=0x30000n;vm.map(data,4096n,7);vm.write(data,[...fx(false,true),0xd9,0xf7,0xdd,0xc2,0xdf,0xe0,0x83,0xe9,1,0x74,2,0xff,0xe6,0xcc]);vm.map(target,4096n);vm.register(2,data);vm.register(6,base);vm.register(3,target);vm.register(1,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
for(const count of [12,2]){
 const before=seed(),xm=vectors();before.regs[0]=(0x1234n<<64n)|0x0101010101010101n;before.regs[1]=(0x4321n<<64n)|0x0202020202020202n;fp(resident,before);setVectors(resident,xm);vm.ip=base;vm.register(1,BigInt(count));vm.mxcsr=0x9fc1;resident.calls.begin=0;resident.calls.link=0;
 resident.watch.onLink=()=>{equal(fp(resident),before);xm.forEach((v,n)=>equal(xmm(resident,n),v));assert.equal(vm.mxcsr,0x9fc1);};
 assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const expected=init(before);expected.regs=[...before.regs];expected.regs[0]=(0xffffn<<64n)|BigInt(1+count*2)*0x0101010101010101n;expected.tags=0;const snapshot=image(expected,xm,64,true);equal(read(resident,target,464),snapshot.slice(0,464));expected.sw=0x800;expected.tags=3<<6;expected.ip=data+6n;expected.opcode=0x5c2;equal(fp(resident),expected);assert.equal(vm.register(0)&0xffffn,0x800n);assert.equal(vm.mxcsr,0x9fc1);assert.equal(resident.calls.begin,1);assert.equal(resident.calls.link,2);assert.deepEqual(vm.stats(),stats);
}vm.dispose();

const domains=program([...fx(true,true),0xf3,0x0f,0x58,0xc1,0x9b]),domainFp=seed(),domainXmm=vectors();domainFp.cw=0x37e;domainXmm[0]=encode(0x3f800000n);domainXmm[1]=encode(0x3f800000n);const domainImage=image(domainFp,domainXmm,64,true);new DataView(domainImage.buffer).setUint32(24,0x1f01,true);domains.vm.map(data,4096n);domains.vm.write(data,domainImage);domains.vm.register(3,data);
assert.equal((await domains.vm.run(1n)).reason,Exit.Budget);const restored=decoded(domainImage,seed(),vectors(),64,true).fp;equal(fp(domains),restored);assert.equal(domains.vm.mxcsr,0x1f01);assert.equal((await domains.vm.run(1n)).reason,Exit.Budget,'SSE ignores pending x87 and old SIMD sticky exceptions');equal(xmm(domains,0),encode(0x40000000n));assert.equal(domains.vm.mxcsr,0x1f01);assert.equal((await domains.vm.run(1n)).reason,Exit.Exception);assert.equal(core.wire_exception(domains.handle),17);equal(fp(domains),restored);assert.equal(domains.vm.work,2n);domains.vm.dispose();

const isolated=[program(fx(true,true)),program(fx(true,true))],isolationStates=isolated.map((test,n)=>{const state=seed(),xm=vectors();state.sw=(state.sw&~0x3800)|(n<<11);state.ip+=BigInt(n);const bytes=image(state,xm,64,true);test.vm.map(data,4096n);test.vm.write(data,bytes);test.vm.register(3,data);return decoded(bytes,seed(),Array.from({length:16},()=>new Uint8Array(16)),64,true);});
isolated[0].watch.onArea=pointer=>{const other=core.wire_fp_area(isolated[1].handle,data,416,1);assert.notEqual(pointer,other,'each VM owns its image buffer');};assert.equal((await isolated[0].vm.run(1n)).reason,Exit.Budget);equal(fp(isolated[0]),isolationStates[0].fp);assert.equal((await isolated[1].vm.run(1n)).reason,Exit.Budget);equal(fp(isolated[1]),isolationStates[1].fp);isolated.forEach(test=>test.vm.dispose());

const mixed=program([...fx(true,true),0x0f,0xf7,0xc1,0x48,0xf7,0xf1,0x0f,0x2a,0xd8]),mixedState=seed(),mixedXmm=vectors();mixedState.sw=0;mixedState.tags=0;mixedState.regs[0]=(0xffffn<<64n)|0x00000002ffffffffn;mixedState.regs[1]=(0xffffn<<64n)|0x8080808080808080n;const mixedImage=image(mixedState,mixedXmm,64,true);new DataView(mixedImage.buffer).setUint32(24,0x1f80,true);mixed.vm.map(data,4096n);mixed.vm.map(target,4096n);mixed.vm.write(data,mixedImage);mixed.vm.register(3,data);mixed.vm.register(7,target);mixed.vm.register(0,12n);mixed.vm.register(2,0n);mixed.vm.register(1,3n);
assert.equal((await mixed.vm.run(4n)).reason,Exit.Budget,'FP images, masked stores, integer helpers and SSE helpers share import indices');assert.equal(mixed.vm.register(0),4n);assert.equal(mixed.vm.register(2),0n);equal(read(mixed,target,8),encode(0x00000002ffffffffn,8));const mixedResult=mixedXmm[3].slice();mixedResult.set(encode(0x40000000bf800000n,8));equal(xmm(mixed,3),mixedResult);equal(fp(mixed),{...decoded(mixedImage,seed(),vectors(),64,true).fp,tags:0});mixed.vm.dispose();

console.log(`${checks} FP-state checks passed: non-waiting controls, TOP/tags, metadata, FX layouts, precise faults, resident snapshots, and source revocation`);
