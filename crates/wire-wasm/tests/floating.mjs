// Explicit SSE execution checks use integer-ratio references and generated
// instruction bytes, including the numerical call's CPU-state boundary.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine,Exit } from '../web/runtime.mjs';
import { mask,format,read,reported,round,arithmetic,sqrt,predicate,conversion,integer,integral,dot } from './floating-reference.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const build=spawnSync('cargo',['build','--quiet','--offline','--locked','--lib','--manifest-path','crates/wire-wasm/Cargo.toml','--target','wasm32-unknown-unknown','--target-dir','target/wasm-rewrite'],{cwd:root,encoding:'utf8'});
assert.equal(build.status,0,build.stderr);
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm')));
const core=instance.exports,base=0x10000n,data=0x20000n;
const layout=Array.from(new Uint32Array(core.memory.buffer,core.wire_state_layout(),11));
let checks=0;
function program(code,mode=64){
  let handle,ticket;const calls={numeric:0,resolve:0,load:0,store:0,link:0,begin:0,compile:0};const watch={onLink:null};
  const imports={...core,wire_create(...a){handle=core.wire_create(...a);return handle;},wire_ticket(...a){ticket=core.wire_ticket(...a);return ticket;}};
  for(const name of Object.keys(core).filter(name=>/^wire_(?:f32|f64|i64_f|i32x_f)/.test(name)))imports[name]=(...args)=>{
    const pointer=core.wire_state(handle),before=new Uint8Array(core.memory.buffer,pointer,layout[0]).slice();calls.numeric++;
    const result=core[name](...args);
    assert.deepEqual(new Uint8Array(core.memory.buffer,pointer,layout[0]),before,'numeric calls do not transact CPU backing');return result;
  };
  for(const name of ['resolve','load','store','link','begin','compile'])imports[`wire_${name}`]=(...args)=>{calls[name]++;if(name==='link')watch.onLink?.();return core[`wire_${name}`](...args);};
  const vm=new WasmMachine(imports,{mode,ip:base});vm.map(base,4096n,7);vm.write(base,[...code,0xcc]);
  return {vm,calls,watch,get handle(){return handle;},get ticket(){return ticket;}};
}
const pack=(values,bits)=>{const value=values.reduceRight((a,b)=>(a<<BigInt(bits))|(b&mask(bits)),0n);return Uint8Array.from({length:16},(_,n)=>Number((value>>BigInt(n*8))&255n));};
const unpack=(bytes,bits)=>{const value=[...bytes].reduceRight((a,b)=>(a<<8n)|BigInt(b),0n);return Array.from({length:128/bits},(_,n)=>(value>>BigInt(n*bits))&mask(bits));};
const equal=(a,b,label)=>{assert.deepEqual([...a],[...b],label);checks++;};
function values(bits){const f=format(bits),sign=1n<<BigInt(bits-1),one=BigInt(f.bias)<<BigInt(f.p),inf=BigInt(f.emax+f.bias+1)<<BigInt(f.p);
  return [0n,sign,one,one|sign,one+1n,one-(1n<<BigInt(f.p)),1n,sign|1n,mask(f.p),1n<<BigInt(f.p),(1n<<BigInt(f.p))+1n,inf-1n,sign|(inf-1n),inf,sign|inf,inf|(1n<<BigInt(f.p-1))|123n,inf|321n,BigInt(f.bias-f.p-1)<<BigInt(f.p)];}
const prefixes=(bits,scalar)=>scalar?[bits===32?0xf3:0xf2]:bits===64?[0x66]:[];
const ops=[['add',0x58],['sub',0x5c],['mul',0x59],['div',0x5e],['min',0x5d],['max',0x5f],['sqrt',0x51]];
for(const mode of [32,64])for(const bits of [32,64])for(const scalar of [false,true])for(const memory of [false,true])for(const [kind,opcode] of ops){
  const test=program([0x04,1,...prefixes(bits,scalar),0x0f,opcode,memory?0x03:0xc1],mode),{vm}=test;
  if(memory)vm.map(data,4096n);const list=values(bits),lanes=scalar?1:128/bits;
  for(const control of mode===32||memory?[0x1f80]:[0x1f80,0x3f80,0x5f80,0x7f80,0x9fc0,0x1fa0])for(let n=0;n<list.length;n++){
    const a=Array.from({length:128/bits},(_,lane)=>list[(n+lane*3)%list.length]),b=Array.from({length:128/bits},(_,lane)=>list[(n*7+lane*5+3)%list.length]);
    vm.ip=base;vm.flags=0x602n;vm.register(0,127n);vm.register(3,data);vm.xmm(0,pack(a,bits));vm.xmm(1,pack(b,bits));vm.mxcsr=control;
    if(memory)vm.write(data,pack(b,bits).slice(0,scalar?bits/8:16));
    const expected=[...a];let flags=0;
    for(let lane=0;lane<lanes;lane++){const result=kind==='sqrt'?sqrt(b[lane],bits,control):arithmetic(kind,a[lane],b[lane],bits,control);expected[lane]=result.value;flags|=result.flags;}
    const before=vm.work,numeric=test.calls.numeric;
    assert.equal((await vm.run(2n)).reason,Exit.Budget,`${kind}/${bits}/${scalar}/${mode}`);
    equal(vm.xmm(0),pack(expected,bits),`${kind}/${bits}/${scalar} case ${n} MXCSR ${control.toString(16)}`);
    assert.equal(vm.mxcsr,reported(control,flags),`${kind}/${bits} flags case ${n}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.work-before,2n);assert.equal(test.calls.numeric-numeric,1,'one numerical call for the entire vector');
  }
  if(memory){assert.equal(test.calls.resolve,1);assert.equal(test.calls.load,0);}
  vm.dispose();
}

let seed=0x91ab72cd;
const random=()=>{seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;return BigInt(seed>>>0);};
for(const bits of [32,64])for(const [kind,opcode] of ops.slice(0,4)){
  const test=program([...prefixes(bits,false),0x0f,opcode,0xc1]),{vm}=test;
  for(let n=0;n<160;n++)for(const csr of [0x1f80,0x1fa0,0x0f80,0x0fa0]){
    const next=()=>bits===32?random():(random()<<32n)|random(),a=Array.from({length:128/bits},next),b=Array.from({length:128/bits},next);let flags=0;
    const out=a.map((value,lane)=>{const r=arithmetic(kind,value,b[lane],bits,csr);flags|=r.flags;return r.value;});const fault=!!(flags&~(csr>>7));
    vm.ip=base;vm.xmm(0,pack(a,bits));vm.xmm(1,pack(b,bits));vm.mxcsr=csr;
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),pack(fault?a:out,bits),`${kind}/${bits} generated ${n}/${csr}`);assert.equal(vm.mxcsr,reported(csr,flags));
  }
  vm.dispose();
}

for(const bits of [32,64])for(const [kind,opcode] of ops.slice(0,4)){
  const test=program([...prefixes(bits,true),0x0f,opcode,0xc1]),{vm}=test,f=format(bits),one=BigInt(f.bias)<<BigInt(f.p),small=1n<<BigInt(f.p),list=values(bits);
  const cases=[[small,one-1n],[small,one-(1n<<BigInt(f.p))],[1n,one-(1n<<BigInt(f.p))],[list[11],one+(1n<<BigInt(f.p))],[list[11],list[11]],[list[16],list[15]],[list[15],list[16]],[1n,0n],[small,(1n<<BigInt(bits-1))|1n]];
  for(const csr of [0x1f80,0x1780,0x1b80,0x1e80,0x0f80,0,0x9f80,0x9780,0x3f80,0x5f80,0x7f80])for(const [a,b] of cases){
    const old=pack([a,...Array(128/bits-1).fill(list[16])],bits),r=arithmetic(kind,a,b,bits,csr),fault=!!(r.flags&~(csr>>7)),expected=unpack(old,bits);if(!fault)expected[0]=r.value;
    vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack([b],bits));vm.mxcsr=csr;
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget,`${kind}/${bits} range ${a.toString(16)}/${b.toString(16)}/${csr.toString(16)}`);equal(vm.xmm(0),pack(expected,bits));assert.equal(vm.mxcsr,reported(csr,r.flags));
  }
  vm.dispose();
}
{
  const test=program([0xf2,0x0f,0x5a,0xc1]),{vm}=test,small=897n<<52n;
  for(const csr of [0x1f80,0x1780,0x9f80,0x9780,0x3f80,0x5f80,0x7f80])for(const value of [small-(1n<<29n),small-(1n<<28n),small-(1n<<28n)-1n,small-1n,873n<<52n,872n<<52n]){
    const old=pack(values(32).slice(14),32),r=conversion(value,64,32,csr),fault=!!(r.flags&~(csr>>7)),expected=unpack(old,32);if(!fault)expected[0]=r.value;
    vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack([value],64));vm.mxcsr=csr;
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),pack(expected,32));assert.equal(vm.mxcsr,reported(csr,r.flags));
  }
  vm.dispose();
}

for(const bits of [32,64])for(const scalar of [false,true])for(const p of [0,1,2,3,4,5,6,7,255]){
  const test=program([...prefixes(bits,scalar),0x0f,0xc2,0xc1,p]),{vm}=test;const list=values(bits),count=scalar?1:128/bits;
  for(const [a,b] of [[2,2],[2,4],[4,2],[15,2],[16,2],[2,6]]){
    const left=Array(128/bits).fill(list[a]),right=Array(128/bits).fill(list[b]);vm.ip=base;vm.xmm(0,pack(left,bits));vm.xmm(1,pack(right,bits));vm.mxcsr=0x1f80;
    const result=predicate(list[a],list[b],bits,p,0x1f80),expected=[...left];for(let lane=0;lane<count;lane++)expected[lane]=result.value?mask(bits):0n;
    assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(expected,bits));assert.equal(vm.mxcsr,0x1f80|result.flags);
  }
  vm.dispose();
}
for(const bits of [32,64])for(const unordered of [false,true]){
  const test=program([0x04,1,...(bits===64?[0x66]:[]),0x0f,unordered?0x2e:0x2f,0xc1,0x0f,0x90,0xc0,0x0f,0x92,0xc1]),{vm}=test;const list=values(bits);
  for(const [a,b] of [[2,2],[2,4],[4,2],[15,2],[16,2]])for(const masked of [false,true]){
    vm.ip=base;vm.flags=0x602n;vm.register(0,127n);vm.xmm(0,pack([list[a],...Array(128/bits-1).fill(list[16])],bits));vm.xmm(1,pack([list[b]],bits));vm.mxcsr=masked?0x1f80:0x1f00;
    const result=predicate(list[a],list[b],bits,0,vm.mxcsr,!unordered),fault=!masked&&(result.flags&1),before=vm.work;
    assert.equal((await vm.run(2n)).reason,fault?Exit.Exception:Exit.Budget);assert.equal(vm.work-before,fault?1n:2n);
    assert.equal(vm.mxcsr,(masked?0x1f80:0x1f00)|result.flags);
    if(fault){assert.equal(vm.flags,0xe92n);assert.equal(core.wire_exception(test.handle),20);}
    else{const flags=result.order===null?0x45n:result.order<0?1n:result.order===0?0x40n:0n;assert.equal(vm.flags,0x602n|flags);assert.equal((await vm.run(2n)).reason,Exit.Budget);assert.equal(vm.register(0)&255n,0n);assert.equal(vm.register(1)&255n,flags&1n);}
    checks++;
  }
  vm.dispose();
}

for(const source of [32,64])for(const scalar of [false,true])for(const memory of [false,true]){
  const dest=source===32?64:32,prefix=scalar?(source===32?0xf3:0xf2):source===64?0x66:0;
  const test=program([...(prefix?[prefix]:[]),0x0f,0x5a,memory?0x03:0xc1]),{vm}=test;
  if(memory)vm.map(data,4096n);const list=values(source),count=scalar?1:2;
  for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x9fc0])for(let n=0;n<list.length;n++){
    const input=Array.from({length:128/source},(_,lane)=>list[(n+lane*7)%list.length]),old=pack(values(dest).slice(13),dest);
    vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack(input,source));vm.register(3,data);vm.mxcsr=csr;
    if(memory)vm.write(data,pack(input,source).slice(0,count*source/8));
    const out=scalar?unpack(old,dest):Array(128/dest).fill(0n);let flags=0;
    for(let lane=0;lane<count;lane++){const result=conversion(input[lane],source,dest,csr);out[lane]=result.value;flags|=result.flags;}
    assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(out,dest),`convert ${source}/${scalar}/${memory}/${n}/${csr}`);assert.equal(vm.mxcsr,reported(csr,flags));
  }
  vm.dispose();
}
for(const dest of [32,64])for(const width of [32,64])for(const memory of [false,true]){
  const test=program([dest===32?0xf3:0xf2,...(width===64?[0x48]:[]),0x0f,0x2a,memory?0x03:0xc1]),{vm}=test;
  if(memory)vm.map(data,4096n);
  const inputs=[0n,1n,-1n,1n<<BigInt(width-2),(1n<<BigInt(width-1))-1n,-(1n<<BigInt(width-1)),(1n<<24n)+1n,-((1n<<24n)+1n)];
  for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x0f80])for(const value of inputs){
    const old=pack(values(dest).slice(13),dest),result=round(value,1n,false,dest,csr),fault=!!(result.flags&~(csr>>7));
    vm.ip=base;vm.xmm(0,old);vm.register(1,BigInt.asUintN(64,value));vm.register(3,data);vm.mxcsr=csr;
    if(memory)vm.write(data,pack([value],width).slice(0,width/8));
    const expected=unpack(old,dest);if(!fault)expected[0]=result.value;
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),pack(expected,dest));assert.equal(vm.mxcsr,reported(csr,result.flags));
    if(fault){assert.equal(vm.ip,base);assert.equal(core.wire_exception(test.handle),20);}
  }
  vm.dispose();
}
for(const source of [32,64])for(const width of [32,64])for(const trunc of [false,true])for(const memory of [false,true]){
  const test=program([source===32?0xf3:0xf2,...(width===64?[0x48]:[]),0x0f,trunc?0x2c:0x2d,memory?0x03:0xc1]),{vm}=test;
  if(memory)vm.map(data,4096n);
  const f=format(source),list=[...values(source),BigInt(f.bias+width-1)<<BigInt(f.p),(1n<<BigInt(source-1))|(BigInt(f.bias+width-1)<<BigInt(f.p)),(BigInt(f.bias+width-1)<<BigInt(f.p))-1n];
  for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x9fc0,0x1f00,0x0f80])for(const value of list){
    vm.ip=base;vm.register(0,mask(64));vm.register(3,data);vm.xmm(1,pack([value],source));vm.mxcsr=csr;
    if(memory)vm.write(data,pack([value],source).slice(0,source/8));
    const result=integer(value,source,width,csr,trunc),fault=!!(result.flags&~(csr>>7)),before=vm.work;
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget,`to int ${source}/${width}/${trunc}/${memory}`);
    assert.equal(vm.register(0),fault?mask(64):result.value,`integer result ${source}/${width}/${value.toString(16)}`);assert.equal(vm.mxcsr,reported(csr,result.flags));assert.equal(vm.work-before,fault?0n:1n);checks++;
  }
  vm.dispose();
}
for(const dest of [32,64])for(const memory of [false,true]){
  const test=program([...(dest===64?[0xf3]:[]),0x0f,dest===64?0xe6:0x5b,memory?0x03:0xc1]),{vm}=test;
  if(memory)vm.map(data,4096n);const inputs=[0n,mask(32),0x7fffffffn,0x80000000n,0x1000001n],count=128/dest;
  for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x0f80])for(let n=0;n<inputs.length;n++){
    const input=Array.from({length:4},(_,lane)=>inputs[(n+lane)%inputs.length]),old=pack(values(dest),dest);let flags=0;
    const out=input.slice(0,count).map(value=>{const r=round(BigInt.asIntN(32,value),1n,false,dest,csr);flags|=r.flags;return r.value;});const fault=!!(flags&~(csr>>7));
    vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack(input,32));vm.mxcsr=csr;vm.register(3,data);
    if(memory)vm.write(data,pack(input,32).slice(0,count*4));
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),fault?old:pack(out,dest));assert.equal(vm.mxcsr,reported(csr,flags));
  }
  vm.dispose();
}
for(const source of [32,64])for(const trunc of [false,true])for(const memory of [false,true]){
  const test=program([source===32?(trunc?0xf3:0x66):(trunc?0x66:0xf2),0x0f,source===32?0x5b:0xe6,memory?0x03:0xc1]),{vm}=test;
  if(memory)vm.map(data,4096n);const list=values(source),count=128/source;
  for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x9fc0,0x1f00,0x0f80])for(let n=0;n<list.length;n++){
    const input=Array.from({length:count},(_,lane)=>list[(n+lane*5)%list.length]),old=pack(values(32),32);let flags=0;
    const out=input.map(value=>{const r=integer(value,source,32,csr,trunc);flags|=r.flags;return r.value;});const fault=!!(flags&~(csr>>7));
    vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack(input,source));vm.mxcsr=csr;vm.register(3,data);if(memory)vm.write(data,pack(input,source));
    assert.equal((await vm.run(1n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),fault?old:pack(out,32));assert.equal(vm.mxcsr,reported(csr,flags));
  }
  vm.dispose();
}

for(const mode of [32,64]){
  const test=program([0x04,1,0x0f,0xae,0x13,0x0f,0xae,0x5f,4,0x0f,0xae,0x5b,8],mode),{vm}=test;
  vm.map(data,4096n);vm.register(3,data);vm.register(7,data+4096n-8n);
  for(const value of [0,0xffff,0x5fa0,0x10000,0xffffffff]){
    vm.ip=base;vm.mxcsr=0x1f80;vm.flags=0x602n;vm.register(0,127n);vm.write(data,pack([BigInt(value)],32).slice(0,4));
    const before=vm.work;
    assert.equal((await vm.run(4n)).reason,value>0xffff?Exit.Exception:Exit.Budget);
    assert.equal(vm.mxcsr,value>0xffff?0x1f80:value);assert.equal(vm.flags,0xe92n);assert.equal(vm.work-before,value>0xffff?1n:4n);
    if(value>0xffff){assert.equal(vm.ip,base+2n);assert.equal(core.wire_exception(test.handle),14);}
    else assert.equal(vm.readInt(data+8n),BigInt(value));checks++;
  }
  vm.dispose();
}
// A data fault must precede numerical exceptions and leave the entire failed
// destination unchanged, even after the first half of a source was read.
for(const [code,size,vector] of [
  [[0xf3,0x0f,0x58,0x03],4,true],[[0xf2,0x0f,0x58,0x03],8,true],
  [[0x66,0x0f,0x5a,0x03],16,true],[[0xf3,0x48,0x0f,0x2d,0x03],4,false],
  [[0x0f,0xae,0x13],4,false],[[0x0f,0xae,0x1b],4,false],
])for(const cross of [false,true]){
  const test=program([0x04,1,...code]),{vm}=test,old=pack(values(32).slice(14),32),address=data+(cross?4096n-BigInt(size/2):0n);
  vm.map(data,8192n);vm.write(address,pack([0x7f800123n,0x7ff0000000000001n],64));vm.register(0,127n);vm.register(3,address);vm.flags=0x602n;vm.mxcsr=0x1f00;vm.xmm(0,old);
  // Wide legacy operands require alignment before either page is accessed.
  const alignment=size===16&&cross;
  assert.equal(core.wire_protect(test.handle,cross?data+4096n:data,4096n,cross?1:0x104),0);
  assert.equal((await vm.run(10n)).reason,alignment?Exit.Exception:Exit.Fault);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f00);assert.equal(test.calls.numeric,0);equal(vm.xmm(0),old);
  if(alignment)assert.equal(core.wire_exception(test.handle),14);
  else assert.equal(core.wire_fault_kind(test.handle),cross?3:4);
  if(!vector)assert.equal(vm.register(0),128n);vm.dispose();
}
for(const bits of [32,64])for(const scalar of [false,true]){
  const list=values(bits),count=scalar?1:128/bits;
  // Distinct lanes exercise invalid, denormal, overflow and precision status.
  for(const [kind,opcode] of [['add',0x58],['div',0x5e],['sqrt',0x51]])for(const maskBit of [7,8,9,10,11,12]){
    const test=program([0x04,1,...prefixes(bits,scalar),0x0f,opcode,0xc1]),{vm}=test;
    const a=[list[11],list[6],list[16],list[2]].slice(0,count),b=[list[11],0n,list[2],list[4]].slice(0,count),old=pack(a,bits),csr=0x1f80&~(1<<maskBit);let flags=0;
    const out=[...unpack(old,bits)];
    for(let lane=0;lane<count;lane++){const r=kind==='sqrt'?sqrt(b[lane],bits,csr):arithmetic(kind,a[lane],b[lane],bits,csr);out[lane]=r.value;flags|=r.flags;}
    const fault=!!(flags&~(csr>>7));vm.xmm(0,old);vm.xmm(1,pack(b,bits));vm.mxcsr=csr;vm.register(0,127n);vm.flags=0x602n;
    assert.equal((await vm.run(2n)).reason,fault?Exit.Exception:Exit.Budget);equal(vm.xmm(0),fault?old:pack(out,bits),`${kind} precise ${bits}/${scalar}/${maskBit}`);assert.equal(vm.mxcsr,reported(csr,flags));assert.equal(vm.retired,fault?1n:2n);assert.equal(vm.flags,0xe92n);vm.dispose();
  }
}

const resident=program([0x0f,0xae,0x13,0xff,0xe2]),{vm}=resident;
const fp=0x30000n,integerBlock=0x40000n;
vm.map(data,4096n);vm.map(fp,4096n,7);vm.write(fp,[0xf3,0x0f,0x58,0xc1,0xff,0xe6]);
vm.map(integerBlock,4096n,7);vm.write(integerBlock,[0x04,1,0x83,0xe9,1,0x74,2,0xff,0xe7,0x0f,0xae,0x5d,0,0xcc]);
vm.register(3,data);vm.register(2,fp);vm.register(6,integerBlock);vm.register(7,fp);vm.register(5,data+8n);vm.write(data,[0x80,0x5f,0,0]);
const original=pack([0x3f800000n,0x7f800123n,0x7f800123n,0x7f800123n],32);vm.xmm(1,pack([0x33800000n],32));vm.xmm(0,original);vm.register(1,2n);
assert.equal((await vm.run(100n)).reason,Exit.Unsupported);
const installed=vm.stats();
for(const count of [11,3]){
  vm.ip=base;vm.xmm(0,original);vm.mxcsr=0x1f80;vm.register(1,BigInt(count));resident.calls.begin=0;resident.calls.link=0;
  resident.watch.onLink=()=>{equal(vm.xmm(0),original,'resident XMM backing');assert.equal(vm.mxcsr,0x1f80,'resident MXCSR backing');};
  assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);
  const expected=unpack(original,32);expected[0]+=BigInt(count);equal(vm.xmm(0),pack(expected,32));assert.equal(vm.mxcsr,0x5fa0);assert.equal(vm.readInt(data+8n),0x5fa0n);assert.equal(resident.calls.begin,1);assert.equal(resident.calls.link,3);assert.deepEqual(vm.stats(),installed);
}
vm.dispose();
const owners=[program([0xf3,0x0f,0x58,0xc1]),program([0xf3,0x0f,0x58,0xc1])];
owners.forEach(({vm},n)=>{vm.xmm(0,original);vm.xmm(1,pack([0x33800000n],32));vm.mxcsr=n===0?0x3f80:0x5f80;});
await Promise.all(owners.map(({vm})=>vm.run(1n)));
owners.forEach(({vm},n)=>{const expected=unpack(original,32);expected[0]+=BigInt(n);equal(vm.xmm(0),pack(expected,32));assert.equal(vm.mxcsr,n===0?0x3fa0:0x5fa0);vm.dispose();});
const source=program([0x0f,0xae,0x1b]);source.vm.register(3,base+0x100n);source.vm.mxcsr=0x5fa0;
assert.equal((await source.vm.run(1n)).reason,Exit.Budget);assert.equal(source.vm.readInt(base+0x100n),0x5fa0n);assert.equal(source.vm.retired,1n);assert.equal(core.wire_ready(source.handle,source.ticket),0);assert.equal(source.calls.store,1);source.vm.dispose();checks++;

const mixed=program([0x48,0xf7,0xe1,0xf3,0x0f,0x58,0x03,0xf3,0x44,0x0f,0x2d,0xc0,0x0f,0x92,0xc2]);
mixed.vm.map(data,4096n);mixed.vm.write(data,[0,0,0x80,0x33]);mixed.vm.register(0,mask(64));mixed.vm.register(1,2n);mixed.vm.register(3,data);mixed.vm.xmm(0,original);
assert.equal((await mixed.vm.run(4n)).reason,Exit.Budget);assert.equal(mixed.vm.register(0),mask(64)-1n);assert.equal(mixed.vm.register(2),1n);assert.equal(mixed.vm.register(8),1n);assert.equal(mixed.vm.mxcsr,0x1fa0);equal(mixed.vm.xmm(0),original);mixed.vm.dispose();

const isolated=program([0x0f,0x2f,0xc1]);isolated.vm.flags=0xad7n;isolated.vm.mxcsr=0x1f00;isolated.vm.xmm(0,original);isolated.vm.xmm(1,pack([0x7fc12345n],32));
assert.equal((await isolated.vm.run(0n)).reason,Exit.Budget);assert.equal(isolated.vm.mxcsr,0x1f00);
assert.equal((await isolated.vm.run(1n)).reason,Exit.Exception);assert.equal(isolated.vm.mxcsr,0x1f01);assert.equal(isolated.vm.flags,0xad7n);assert.equal(isolated.vm.retired,0n);equal(isolated.vm.xmm(0),original);isolated.vm.dispose();

for(const mode of [32,64])for(const bits of [32,64])for(const [kind,opcode] of [['hadd',0x7c],['hsub',0x7d],['addsub',0xd0]])for(const memory of [false,true]){
  const test=program([0x04,1,bits===32?0xf2:0x66,0x0f,opcode,memory?0x03:0xc1],mode),{vm}=test,list=values(bits),count=128/bits;
  if(memory)vm.map(data,4096n);
  for(const csr of mode===32||memory?[0x1f80]:[0x1f80,0x3f80,0x5f80,0x7f80,0x1f00,0x1e80,0x0f80,0x1780])for(let n=0;n<list.length;n++){
    const a=Array.from({length:count},(_,k)=>list[(n+k*5)%list.length]),b=Array.from({length:count},(_,k)=>list[(n*3+k*7+1)%list.length]);let flags=0;
    const expected=Array.from({length:count},(_,k)=>{
      const input=k<count/2?a:b,pair=2*(k%(count/2)),left=kind==='addsub'?a[k]:input[pair],right=kind==='addsub'?b[k]:input[pair+1],op=kind==='hsub'||kind==='addsub'&&k%2===0?'sub':'add';
      const r=arithmetic(op,left,right,bits,csr);flags|=r.flags;return r.value;
    }),fault=!!(flags&~(csr>>7));
    vm.ip=base;vm.xmm(0,pack(a,bits));vm.xmm(1,pack(b,bits));vm.mxcsr=csr;vm.flags=0x602n;vm.register(0,127n);vm.register(3,data);if(memory)vm.write(data,pack(b,bits));const before=vm.work,numeric=test.calls.numeric;
    assert.equal((await vm.run(2n)).reason,fault?Exit.Exception:Exit.Budget,`${kind}/${bits}/${mode}/${memory}`);equal(vm.xmm(0),pack(fault?a:expected,bits));equal(vm.xmm(1),pack(b,bits));assert.equal(vm.mxcsr,reported(csr,flags));assert.equal(vm.flags,0xe92n);assert.equal(vm.work-before,fault?1n:2n);assert.equal(test.calls.numeric-numeric,1);
  }
  assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
  if(!memory){
    const {vm}=program([bits===32?0xf2:0x66,0x0f,opcode,0xc0],mode),a=list.slice(2,2+count);vm.xmm(0,pack(a,bits));let flags=0;
    const expected=Array.from({length:count},(_,k)=>{const pair=2*(k%(count/2)),op=kind==='hsub'||kind==='addsub'&&k%2===0?'sub':'add',r=arithmetic(op,kind==='addsub'?a[k]:a[pair],kind==='addsub'?a[k]:a[pair+1],bits,0x1f80);flags|=r.flags;return r.value;});
    assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(expected,bits));assert.equal(vm.mxcsr,reported(0x1f80,flags));vm.dispose();
  }
}

for(const mode of [32,64])for(const bits of [32,64])for(const scalar of [false,true])for(const memory of [false,true]){
  const controls=[...Array.from({length:16},(_,n)=>n),255],opcode=scalar?(bits===32?0x0a:0x0b):(bits===32?8:9);
  const test=program(controls.flatMap(control=>[0x04,1,0x66,0x0f,0x3a,opcode,memory?0x03:0xc1,control]),mode),{vm}=test,f=format(bits),one=BigInt(f.bias)<<BigInt(f.p),sign=1n<<BigInt(bits-1),two=BigInt(f.bias+1)<<BigInt(f.p);
  const list=[...values(bits),one+(1n<<BigInt(f.p-1)),two+(1n<<BigInt(f.p-2)),two+(3n<<BigInt(f.p-2)),sign|(one+(1n<<BigInt(f.p-1))),sign|(one-1n),one-1n],count=scalar?1:128/bits,width=scalar?bits/8:16,address=data+4096n-BigInt(width);
  if(memory)vm.map(data,4096n);vm.register(3,address);
  for(let controlIndex=0;controlIndex<controls.length;controlIndex++)for(const csr of mode===32||memory?[0x1f80]:[0x1f80,0x3f80,0x5f80,0x7f80,0x1e80,0x0f80,0x1f00,0x9fc0,0x0fa0])for(let n=0;n<list.length;n++){
    const control=controls[controlIndex],a=values(bits).slice(14,14+128/bits),b=Array.from({length:128/bits},(_,lane)=>list[(n+lane*7)%list.length]),expected=[...a];let flags=0;
    for(let lane=0;lane<count;lane++){const r=integral(b[lane],bits,csr,control);expected[lane]=r.value;flags|=r.flags;}
    const fault=!!(flags&~(csr>>7));vm.ip=base+BigInt(controlIndex*8);vm.xmm(0,pack(a,bits));vm.xmm(1,pack(b,bits));vm.mxcsr=csr;vm.flags=0x602n;vm.register(0,127n);if(memory)vm.write(address,pack(b,bits).slice(0,width));const before=vm.work,numeric=test.calls.numeric;
    assert.equal((await vm.run(2n)).reason,fault?Exit.Exception:Exit.Budget,`round/${bits}/${scalar}/${mode}/${control}/${csr.toString(16)}`);equal(vm.xmm(0),pack(fault?a:expected,bits));assert.equal(vm.mxcsr,reported(csr,flags));assert.equal(vm.flags,0xe92n);assert.equal(vm.work-before,fault?1n:2n);assert.equal(test.calls.numeric-numeric,1);
  }
  assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
}

for(const bits of [32,64])for(const memory of [false,true]){
  const test=program(Array.from({length:256},(_,control)=>[0x66,0x0f,0x3a,bits===32?0x40:0x41,memory?0x03:0xc1,control]).flat()),{vm}=test,count=128/bits,f=format(bits);
  const a=Array.from({length:count},(_,n)=>BigInt(f.bias+n)<<BigInt(f.p));
  if(memory)vm.map(data,4096n);vm.register(3,data);if(memory)vm.write(data,pack(a,bits));
  for(let control=0;control<256;control++){
    vm.ip=base+BigInt(control*6);vm.xmm(0,pack(a,bits));vm.xmm(1,pack(a,bits));vm.mxcsr=0x1f80;
    const expected=dot(a,a,bits,0x1f80,control);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.xmm(0),pack(expected.values,bits));assert.equal(vm.mxcsr,expected.mxcsr);
  }
  assert.equal(test.calls.compile,1);vm.dispose();
}
for(const mode of [32,64])for(const bits of [32,64])for(const memory of [false,true]){
  const controls=[0,1,15,16,17,0x31,0x33,0x71,0xf0,0xff],test=program(controls.flatMap(control=>[0x04,1,0x66,0x0f,0x3a,bits===32?0x40:0x41,memory?0x03:0xc1,control]),mode),{vm}=test,count=128/bits,list=values(bits);
  if(memory)vm.map(data,4096n);vm.register(3,data);
  const cases=Array.from({length:list.length},(_,n)=>[Array.from({length:count},(_,lane)=>list[(n+lane*3)%list.length]),Array.from({length:count},(_,lane)=>list[(n*7+lane*5+2)%list.length])]);
  cases.push([[list[11],list[11],list[13],list[14]].slice(0,count),Array(count).fill(list[2])]);
  cases.push([[list[9],list[9],list[11],list[11]].slice(0,count),[list[5],list[5],list[2],list[2]].slice(0,count)]);
  for(let index=0;index<controls.length;index++)for(const csr of mode===32||memory?[0x1f80]:[0x1f80,0x3f80,0x5f80,0x7f80,0x1f00,0x1e80,0x1b80,0x1780,0x0f80,0,0x9fc0])for(const [a,b] of cases){
    const control=controls[index],expected=dot(a,b,bits,csr,control);vm.ip=base+BigInt(index*8);vm.register(0,127n);vm.flags=0x602n;vm.mxcsr=csr;vm.xmm(0,pack(a,bits));vm.xmm(1,pack(b,bits));if(memory)vm.write(data,pack(b,bits));const before=vm.work,numeric=test.calls.numeric;
    assert.equal((await vm.run(2n)).reason,expected.exception?Exit.Exception:Exit.Budget,`dot/${bits}/${mode}/${memory}/${control}/${csr}`);equal(vm.xmm(0),pack(expected.exception?a:expected.values,bits));assert.equal(vm.mxcsr,expected.mxcsr);assert.equal(vm.flags,0xe92n);assert.equal(vm.work-before,expected.exception?1n:2n);assert.equal(test.calls.numeric-numeric,1);
  }
  vm.dispose();
}

function approximate(input,output,squareRoot){
  const a=read(input,32,0),b=read(output,32,0),sign=input&0x80000000n;
  if(a.nan)assert.equal(output,input|0x400000n);
  else if((input&0x7f800000n)===0n)assert.equal(output,sign|0x7f800000n);
  else if(squareRoot&&a.sign)assert.equal(output,0xffc00000n);
  else if(a.infinity)assert.equal(output,sign);
  else if(b.zero){
    assert.equal(squareRoot,false);assert.equal(output,sign);
    // A reciprocal can flush inside the ISA's allowed approximation band
    // around the smallest normal result, or for smaller magnitudes.
    assert.ok((a.n<0n?-a.n:a.n)*8192n>=(8192n-3n)*a.d*(1n<<126n));
  }else{
    assert.equal(b.nan||b.infinity,false);assert.notEqual(output&0x7f800000n,0n);assert.equal(b.sign,squareRoot?false:a.sign);
    if(squareRoot){const numerator=b.n*b.n*a.n*8192n**2n,denominator=b.d*b.d*a.d;assert.ok(numerator>=(8192n-3n)**2n*denominator);assert.ok(numerator<=(8192n+3n)**2n*denominator);}
    else{const error=b.n*a.n-b.d*a.d;assert.ok((error<0n?-error:error)*8192n<=3n*b.d*a.d);}
  }
  checks++;
}
for(const mode of [32,64])for(const scalar of [false,true])for(const squareRoot of [false,true])for(const memory of [false,true]){
  const test=program([0x04,1,...(scalar?[0xf3]:[]),0x0f,squareRoot?0x52:0x53,memory?0x03:0xc1],mode),{vm}=test,list=[...values(32),0x7e800000n-1n,0x7e800000n,0x7e800001n,0x7e7ff000n,0x7e801000n],width=scalar?4:16,address=data+4096n-BigInt(width),old=pack([0x3f800000n,0x7f800123n,0xff800321n,0x7fc12345n],32);
  for(let n=0;n<96;n++)list.push(random());
  if(memory)vm.map(data,4096n);vm.register(3,address);
  for(let n=0;n<list.length;n++){
    const input=Array.from({length:4},(_,k)=>list[(n+k*7)%list.length]);let baseline;
    for(const csr of n<23?[0,0x1f80,0xffff,0x3f80,0x5f80,0x7f80,0x9f80,0x9fc0]:[0]){
      vm.ip=base;vm.xmm(0,old);vm.xmm(1,pack(input,32));vm.mxcsr=csr;vm.flags=0x602n;vm.register(0,127n);if(memory)vm.write(address,pack(input,32).slice(0,width));const before=vm.work,numeric=test.calls.numeric;
      assert.equal((await vm.run(2n)).reason,Exit.Budget);assert.equal(vm.work-before,2n);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,csr);assert.equal(test.calls.numeric,numeric,'approximation stays in generated Wasm');const output=unpack(vm.xmm(0),32);
      for(let lane=0;lane<(scalar?1:4);lane++)approximate(input[lane],output[lane],squareRoot);
      if(scalar)equal(vm.xmm(0).slice(4),old.slice(4));
      if(baseline)equal(vm.xmm(0),baseline,'MXCSR controls do not affect approximate instructions');else baseline=vm.xmm(0);
    }
  }
  assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
}
for(const [opcode,squareRoot] of [[0x52,true],[0x53,false]])for(const scalar of [false,true]){
  const {vm}=program([...(scalar?[0xf3]:[]),0x45,0x0f,opcode,0xfe]),old=pack([0x7f800123n,0xffc00123n,1n,0x7f800000n],32),input=[0x40800000n,0n,0x3f800000n,0xff800000n];vm.xmm(15,old);vm.xmm(14,pack(input,32));vm.mxcsr=0;
  assert.equal((await vm.run(1n)).reason,Exit.Budget);const output=unpack(vm.xmm(15),32);for(let lane=0;lane<(scalar?1:4);lane++)approximate(input[lane],output[lane],squareRoot);if(scalar)equal(vm.xmm(15).slice(4),old.slice(4));assert.equal(vm.mxcsr,0);vm.dispose();
}
for(const code of [
  [0x66,0x0f,0x3a,0x0a,0x03,8],[0x66,0x0f,0x3a,0x0b,0x03,8],
  [0x66,0x0f,0x3a,0x40,0x03,0],[0x66,0x0f,0x3a,0x41,0x03,0],
  [0xf3,0x0f,0x52,0x03],[0xf3,0x0f,0x53,0x03],[0x0f,0x52,0x03],[0x0f,0x53,0x03],
]){
  const test=program([0x04,1,...code]),{vm}=test,old=pack([0x7f800123n],32);vm.map(data,4096n);vm.write(data,old);vm.xmm(0,old);vm.register(3,data);vm.mxcsr=0;vm.flags=0x602n;vm.register(0,127n);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
  assert.equal((await vm.run(10n)).reason,Exit.Fault);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.mxcsr,0);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.numeric,0);equal(vm.xmm(0),old);vm.dispose();
}

console.log(`${checks} SSE floating-point checks passed: rounding, dot products, reciprocal accuracy, conversions, precise exceptions, resident MXCSR, and numerical state isolation`);
