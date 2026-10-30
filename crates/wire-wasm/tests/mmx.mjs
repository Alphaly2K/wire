// Generated MMX inputs and independent lane arithmetic exercise the physical
// x87 alias, exception boundary, and resident state without an x86 executor.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve,dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WasmMachine,Exit } from '../web/runtime.mjs';
import { integer,round,reported } from './floating-reference.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const build=spawnSync('cargo',['build','--quiet','--locked','--offline','--lib','--manifest-path','crates/wire-wasm/Cargo.toml','--target','wasm32-unknown-unknown','--target-dir','target/wasm-rewrite'],{cwd:root,encoding:'utf8'});assert.equal(build.status,0,build.stderr);
const {instance}=await WebAssembly.instantiate(readFileSync(resolve(root,'target/wasm-rewrite/wasm32-unknown-unknown/debug/wire_wasm.wasm'))),core=instance.exports;
const layout=Array.from(new Uint32Array(core.memory.buffer,core.wire_fpu_layout(),7)),base=0x10000n,data=0x20000n;
const mask=bits=>(1n<<BigInt(bits))-1n,sign=(v,bits)=>BigInt.asIntN(bits,v),clamp=(v,a,b)=>v<a?a:v>b?b:v;
const encode=(v,size=8)=>Uint8Array.from({length:size},(_,n)=>Number((v>>BigInt(n*8))&255n));
const lanes=(v,bits)=>Array.from({length:64/bits},(_,n)=>(v>>BigInt(n*bits))&mask(bits));
const pack=(v,bits)=>v.reduceRight((x,y)=>(x<<BigInt(bits))|(y&mask(bits)),0n)&mask(64);
let checks=0;
function program(code,mode=64){
 let handle,ticket;const calls={resolve:0,load:0,store:0,link:0,begin:0,compile:0,numeric:0},watch={onLink:null,onMemory:null};
 const imports={...core,wire_create(...a){handle=core.wire_create(...a);return handle;},wire_ticket(...a){ticket=core.wire_ticket(...a);return ticket;}};
 for(const name of ['resolve','load','store','link','begin','compile'])imports[`wire_${name}`]=(...args)=>{calls[name]++;if(name==='link')watch.onLink?.();if(['resolve','load','store'].includes(name))watch.onMemory?.();return core[`wire_${name}`](...args);};
 for(const name of Object.keys(core).filter(n=>/^wire_(?:f32|f64|i64_f|i32x_f|mul_high|imul_high|div64|idiv64)/.test(n)))imports[name]=(...args)=>{
  const pointer=core.wire_state(handle),length=new Uint32Array(core.memory.buffer,core.wire_state_layout(),1)[0],before=new Uint8Array(core.memory.buffer,pointer,length).slice();calls.numeric++;const result=core[name](...args);
  assert.deepEqual(new Uint8Array(core.memory.buffer,pointer,length),before,'numeric calls do not transact CPU backing');return result;
 };
 const vm=new WasmMachine(imports,{mode,ip:base});vm.map(base,4096n,7);vm.write(base,[...code,0xcc]);
 return {vm,calls,watch,get handle(){return handle;},get ticket(){return ticket;}};
}
function fp(test){
 const view=new DataView(core.memory.buffer),state=core.wire_state(test.handle);
 return {cw:view.getUint16(state+layout[0],true),sw:view.getUint16(state+layout[1],true),tags:view.getUint16(state+layout[2],true),regs:Array.from({length:8},(_,n)=>view.getBigUint64(state+layout[3]+n*16,true)|(view.getBigUint64(state+layout[3]+n*16+8,true)<<64n)),ip:view.getBigUint64(state+layout[4],true),dp:view.getBigUint64(state+layout[5],true),opcode:view.getUint16(state+layout[6],true)};
}
function setFp(test,state){
 core.wire_cancel_repeat(test.handle);const view=new DataView(core.memory.buffer),pointer=core.wire_state(test.handle);
 for(const [key,n] of [['cw',0],['sw',1],['tags',2],['opcode',6]])view.setUint16(pointer+layout[n],state[key],true);
 for(const [key,n] of [['ip',4],['dp',5]])view.setBigUint64(pointer+layout[n],state[key],true);
 for(let n=0;n<8;n++){view.setBigUint64(pointer+layout[3]+n*16,state.regs[n]&mask(64),true);view.setBigUint64(pointer+layout[3]+n*16+8,state.regs[n]>>64n,true);}
}
const seed=()=>({cw:0x037f,sw:0x6f35,tags:0x93a5,regs:Array.from({length:8},(_,n)=>(BigInt(0x7312+n)<<64n)|BigInt(n+1)*0x19283a4b5c6d7e8fn),ip:0x123456789abcden,dp:0xfedcba987654321n,opcode:0x725});
const changed=(before,dest,value)=>({...before,sw:before.sw&~0x3800,tags:0,regs:before.regs.map((v,n)=>n===dest?(0xffffn<<64n)|(value&mask(64)):v)});
const equal=(actual,expected,message)=>{assert.deepEqual(actual,expected,message);checks++;};
const samples=[0n,mask(64),0x0011223344556677n,0x80ff00017f000001n,0x8000800080008000n,0x7fff7fff80008000n,0x7fffffff80000000n,0xfedcba9876543210n];
const binary=[
 ['and',128,0xdb],['andnot',128,0xdf],['or',128,0xeb],['xor',128,0xef],
 ['add',8,0xfc],['add',16,0xfd],['add',32,0xfe],['add',64,0xd4],['sub',8,0xf8],['sub',16,0xf9],['sub',32,0xfa],['sub',64,0xfb],
 ['adds',8,0xec],['adds',16,0xed],['addu',8,0xdc],['addu',16,0xdd],['subs',8,0xe8],['subs',16,0xe9],['subu',8,0xd8],['subu',16,0xd9],
 ['eq',8,0x74],['eq',16,0x75],['eq',32,0x76],['gt',8,0x64],['gt',16,0x65],['gt',32,0x66],
 ['mins',16,0xea],['minu',8,0xda],['maxs',16,0xee],['maxu',8,0xde],['avg',8,0xe0],['avg',16,0xe3],['mul',16,0xd5],
 ['packs',16,0x63],['packu',16,0x67],['packs',32,0x6b],['mulhi',16,0xe5],['mulhiu',16,0xe4],['mulq',32,0xf4],['madd',16,0xf5],['maddubs',8,0x04,true],['mulhrs',16,0x0b,true],['sad',8,0xf6],
 ['hadd',16,0x01,true],['hadd',32,0x02,true],['hadds',16,0x03,true],['hsub',16,0x05,true],['hsub',32,0x06,true],['hsubs',16,0x07,true],
 ['sign',8,0x08,true],['sign',16,0x09,true],['sign',32,0x0a,true],['abs',8,0x1c,true],['abs',16,0x1d,true],['abs',32,0x1e,true],
];
function reference(kind,bits,left,right){
 if(bits===128)return (kind==='and'?left&right:kind==='andnot'?~left&right:kind==='or'?left|right:left^right)&mask(64);
 const a=lanes(left,bits),b=lanes(right,bits);
 if(kind==='packs'||kind==='packu'){const out=bits/2;return pack([...a,...b].map(v=>clamp(sign(v,bits),kind==='packu'?0n:-(1n<<BigInt(out-1)),kind==='packu'?mask(out):mask(out-1))),out);}
 if(kind==='mulq')return (a[0]*b[0])&mask(64);
 if(kind==='madd')return pack([0,1].map(n=>sign(a[2*n],16)*sign(b[2*n],16)+sign(a[2*n+1],16)*sign(b[2*n+1],16)),32);
 if(kind==='maddubs')return pack([0,1,2,3].map(n=>clamp(a[2*n]*sign(b[2*n],8)+a[2*n+1]*sign(b[2*n+1],8),-32768n,32767n)),16);
 if(kind==='sad')return a.reduce((s,x,n)=>s+(x>b[n]?x-b[n]:b[n]-x),0n);
 if(kind.startsWith('h'))return pack([a,b].flatMap(v=>Array.from({length:v.length/2},(_,n)=>{const x=sign(v[2*n],bits),y=sign(v[2*n+1],bits),z=kind.startsWith('hsub')?x-y:x+y;return kind.endsWith('s')?clamp(z,-(1n<<BigInt(bits-1)),mask(bits-1)):z;})),bits);
 return pack(a.map((x,n)=>{const y=b[n],sx=sign(x,bits),sy=sign(y,bits);
  switch(kind){case'add':return x+y;case'sub':return x-y;case'adds':return clamp(sx+sy,-(1n<<BigInt(bits-1)),mask(bits-1));case'subs':return clamp(sx-sy,-(1n<<BigInt(bits-1)),mask(bits-1));case'addu':return clamp(x+y,0n,mask(bits));case'subu':return clamp(x-y,0n,mask(bits));case'eq':return x===y?mask(bits):0n;case'gt':return sx>sy?mask(bits):0n;case'mins':return sx<sy?x:y;case'minu':return x<y?x:y;case'maxs':return sx>sy?x:y;case'maxu':return x>y?x:y;case'avg':return(x+y+1n)>>1n;case'mul':return x*y;case'mulhi':return(sx*sy)>>16n;case'mulhiu':return(x*y)>>16n;case'mulhrs':return(sx*sy+0x4000n)>>15n;case'sign':return sy===0n?0n:sy<0n?-x:x;case'abs':return sy<0n?-sy:sy;default:throw new Error(kind);}
 }),bits);
}
for(const mode of [32,64])for(const [kind,bits,opcode,extended] of binary)for(const memory of [false,true]){
 const test=program([0x04,1,0x0f,...(extended?[0x38]:[]),opcode,memory?0x03:0xc1],mode),{vm}=test;if(memory)vm.map(data,4096n);vm.register(3,data);
 for(let n=0;n<samples.length;n++){
  const a=samples[n],b=samples[(n+3)%samples.length],before=seed();before.regs[0]=(0x1234n<<64n)|a;before.regs[1]=(0x5678n<<64n)|b;setFp(test,before);vm.ip=base;vm.flags=0x602n;vm.mxcsr=0x9fc0;vm.register(0,127n);if(memory)vm.write(data,encode(b));
  assert.equal((await vm.run(2n)).reason,Exit.Budget,`${kind}/${bits}/${mode}/${memory}`);equal(fp(test),changed(before,0,reference(kind,bits,a,b)),`${kind}/${bits}/${n}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x9fc0);
 }
 assert.equal(test.calls.numeric,0);assert.equal(test.calls.resolve,memory?1:0);assert.equal(test.calls.load,0);vm.dispose();
 if(!memory){const test=program([0x0f,...(extended?[0x38]:[]),opcode,0xc0],mode),before=seed();before.regs[0]=(0x7531n<<64n)|samples[4];setFp(test,before);assert.equal((await test.vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,reference(kind,bits,samples[4],samples[4])));test.vm.dispose();}
}
for(const [size,load,store] of [[4,0x6e,0x7e],[8,0x6f,0x7f]])for(const memory of [false,true])for(const write of [false,true]){
 const test=program([0x0f,write?store:load,memory?(write?0x0b:0x03):0xc1]),{vm}=test,before=seed(),input=0xfedcba9876543210n;if(memory){vm.map(data,4096n);vm.register(3,data+4096n-BigInt(size));vm.write(data+4096n-BigInt(size),encode(input,size));}vm.register(1,input);before.regs[0]=(0x1234n<<64n)|samples[2];before.regs[1]=(0x5678n<<64n)|input;setFp(test,before);
 assert.equal((await vm.run(1n)).reason,Exit.Budget);if(write&&memory)assert.equal(vm.readInt(data+4096n-BigInt(size),size),input&mask(size*8));else if(write&&size===4)assert.equal(vm.register(1),samples[2]&mask(32));
 equal(fp(test),write?(memory||size===4?changed(before,-1,0n):changed(before,1,samples[2])):changed(before,0,input&mask(size*8)));vm.dispose();
}
for(const highGpr of [false,true])for(const write of [false,true]){
 const test=program([highGpr?0x49:0x48,0x0f,write?0x7e:0x6e,highGpr?0xc0:0xc1]),{vm}=test,before=seed(),value=0xfedcba9876543210n;before.regs[0]=(0x3210n<<64n)|samples[2];setFp(test,before);vm.register(highGpr?8:1,value);
 assert.equal((await vm.run(1n)).reason,Exit.Budget);if(write)assert.equal(vm.register(highGpr?8:1),samples[2]);equal(fp(test),changed(before,write?-1:0,value));vm.dispose();
}
for(const high of [false,true])for(const toMmx of [false,true]){
 const code=[toMmx?0xf2:0xf3,...(high?[toMmx?0x41:0x44]:[]),0x0f,0xd6,toMmx?(high?0xc7:0xc1):(high?0xf8:0xc8)],test=program(code),{vm}=test,before=seed(),value=0xfedcba9876543210n;before.regs[0]=(0x4321n<<64n)|value;setFp(test,before);vm.xmm(high?15:1,encode(0x12345678n<<64n|samples[2],16));
 assert.equal((await vm.run(1n)).reason,Exit.Budget);if(toMmx)equal(fp(test),changed(before,0,samples[2]));else{equal(vm.xmm(high?15:1),encode(value,16));equal(fp(test),changed(before,-1,0n));}vm.dispose();
}
for(const mode of [32,64])for(const [bits,kind,opcode,group] of [[16,'left',0xf1,6],[32,'left',0xf2,6],[64,'left',0xf3,6],[16,'right',0xd1,2],[32,'right',0xd2,2],[64,'right',0xd3,2],[16,'signed',0xe1,4],[32,'signed',0xe2,4]])for(const form of ['register','memory','immediate']){
 const counts=[0n,1n,BigInt(bits-1),BigInt(bits),BigInt(bits+1),63n,64n,255n,...(form==='immediate'?[]:[0x100000001n,mask(64)])];
 for(const count of counts){
  const code=form==='immediate'?[0x0f,bits===16?0x71:bits===32?0x72:0x73,0xc0|(group<<3),Number(count)]:[0x0f,opcode,form==='memory'?0x03:0xc1],test=program(code,mode),{vm}=test,before=seed(),a=samples[3];before.regs[0]=(0x1234n<<64n)|a;before.regs[1]=(0x4321n<<64n)|count;setFp(test,before);vm.flags=0xad7n;
  if(form==='memory'){vm.map(data,4096n);vm.register(3,data+4088n);vm.write(data+4088n,encode(count));}
  const out=pack(lanes(a,bits).map(v=>count>=BigInt(bits)?kind==='signed'&&sign(v,bits)<0n?mask(bits):0n:kind==='left'?v<<count:kind==='signed'?sign(v,bits)>>count:v>>count),bits);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,out));assert.equal(vm.flags,0xad7n);vm.dispose();
 }
}
for(const [bits,opcode,high] of [[8,0x60,false],[16,0x61,false],[32,0x62,false],[8,0x68,true],[16,0x69,true],[32,0x6a,true]])for(const memory of [false,true]){
 const test=program([0x0f,opcode,memory?0x03:0xc1]),{vm}=test,before=seed(),a=samples[2],b=samples[7],width=high?8:4;before.regs[0]=(0x1357n<<64n)|a;before.regs[1]=(0x2468n<<64n)|b;setFp(test,before);
 if(memory){vm.map(data,4096n);vm.register(3,data+4096n-BigInt(width));vm.write(data+4096n-BigInt(width),encode(b,width));}
 const start=high?32/bits:0,left=lanes(a,bits),right=lanes(b,bits),out=pack(left.slice(start,start+32/bits).flatMap((v,n)=>[v,right[start+n]]),bits);
 assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,out));assert.equal(test.calls.load,0);vm.dispose();
}
for(const memory of [false,true]){
 const test=program(Array.from({length:256},(_,control)=>[0x0f,0x70,memory?0x03:0xc1,control]).flat()),{vm}=test;if(memory){vm.map(data,4096n);vm.register(3,data);vm.write(data,encode(samples[2]));}
 for(let control=0;control<256;control++){const before=seed();before.regs[1]=(0x1234n<<64n)|samples[2];setFp(test,before);vm.ip=base+BigInt(control*4);const input=lanes(samples[2],16),out=pack(input.map((_,n)=>input[(control>>(n*2))&3]),16);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,out));}
 assert.equal(test.calls.compile,1);vm.dispose();
}
for(let control=0;control<256;control+=8){
 const test=program([0x0f,0x38,0,0xc1]),{vm}=test,before=seed(),a=samples[2],controls=Array.from({length:8},(_,n)=>BigInt(control+n));before.regs[0]=(0x1234n<<64n)|a;before.regs[1]=(0x4321n<<64n)|pack(controls,8);setFp(test,before);const bytes=lanes(a,8),out=pack(controls.map(v=>v&128n?0n:bytes[Number(v&7n)]),8);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,out));vm.dispose();
}
for(const memory of [false,true])for(const count of [0,1,7,8,9,15,16,17,31,255]){
 const test=program([0x0f,0x3a,0x0f,memory?0x03:0xc1,count]),{vm}=test,before=seed(),a=samples[2],b=samples[7];before.regs[0]=(0x1234n<<64n)|a;before.regs[1]=(0x4321n<<64n)|b;setFp(test,before);if(memory){vm.map(data,4096n);vm.register(3,data+4088n);vm.write(data+4088n,encode(b));}
 assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,count>=16?0n:((a<<64n)|b)>>BigInt(count*8)));vm.dispose();
}
for(const memory of [false,true])for(const control of [0,1,2,3,255]){
 const test=program([0x0f,0xc4,memory?0x03:0xc1,control]),{vm}=test,before=seed(),value=0xfedcba9876543210n;setFp(test,before);vm.register(1,value);if(memory){vm.map(data,4096n);vm.register(3,data+4094n);vm.write(data+4094n,encode(value,2));}const at=BigInt((control&3)*16),out=((before.regs[0]&mask(64))&~(0xffffn<<at))|((value&0xffffn)<<at);
 assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),changed(before,0,out));vm.dispose();
 const output=program([0x0f,0xc5,0xc1,control]);setFp(output,before);output.vm.register(0,mask(64));assert.equal((await output.vm.run(1n)).reason,Exit.Budget);assert.equal(output.vm.register(0),(before.regs[1]>>at)&0xffffn);equal(fp(output),changed(before,-1,0n));output.vm.dispose();
}
for(const value of samples){const test=program([0x0f,0xd7,0xc1]),before=seed();before.regs[1]=(0xffffn<<64n)|value;setFp(test,before);test.vm.register(0,mask(64));assert.equal((await test.vm.run(1n)).reason,Exit.Budget);assert.equal(test.vm.register(0),lanes(value,8).reduce((v,x,n)=>v|((x>>7n)<<BigInt(n)),0n));equal(fp(test),changed(before,-1,0n));test.vm.dispose();}
for(const offset of [0,1,4088,4092]){
 const test=program([0x0f,0xe7,0x03]),{vm}=test,before=seed();setFp(test,before);vm.map(data,8192n);vm.register(3,data+BigInt(offset));assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal(vm.readInt(data+BigInt(offset),8),before.regs[0]&mask(64));equal(fp(test),changed(before,-1,0n));vm.dispose();
}
for(const code of [[0x0f,0x6f,0x03],[0x0f,0x77],[0xf2,0x0f,0xd6,0xc1],[0xf3,0x0f,0xd6,0xc8]]){
 const test=program([0x04,1,...code]),{vm}=test,before=seed();before.cw=0x37e;setFp(test,before);vm.register(0,127n);vm.flags=0x602n;vm.map(data,4096n);vm.register(3,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 assert.equal((await vm.run(10n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),17);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.resolve,0);assert.equal(test.calls.load,0);equal(fp(test),before);
 if(code[1]===0x6f){setFp(test,{...before,cw:0x37f});assert.equal((await vm.run(1n)).reason,Exit.Fault);assert.equal(core.wire_fault_kind(test.handle),4,'#MF did not consume the memory guard');equal(fp(test),{...before,cw:0x37f});}vm.dispose();
}
for(const [code,size,write] of [[[0x0f,0x6f,0x03],8,false],[[0x0f,0x60,0x03],4,false],[[0x0f,0xc4,0x03,3],2,false],[[0x0f,0x7f,0x03],8,true],[[0x0f,0x7e,0x03],4,true],[[0x0f,0xe7,0x03],8,true]])for(const cross of [false,true]){
 const test=program([0x04,1,...code]),{vm}=test,before=seed();setFp(test,before);vm.flags=0x602n;vm.register(0,127n);vm.map(data,8192n);const address=data+(cross?4096n-BigInt(size/2):0n),bytes=encode(samples[7],size);vm.write(address,bytes);vm.register(3,address);const page=cross?data+4096n:data;assert.equal(core.wire_protect(test.handle,page,4096n,cross?(write?2:1):0x104),0);
 assert.equal((await vm.run(10n)).reason,Exit.Fault);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);equal(fp(test),before,'failed operand access cannot commit MMX tags, TOP or payload');assert.equal(core.wire_protect(test.handle,page,4096n,4),0);assert.equal(vm.readInt(address,size),samples[7]&mask(size*8));assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.dispose();
}
const empty=program([0x0f,0x77]),emptyBefore=seed();setFp(empty,emptyBefore);assert.equal((await empty.vm.run(0n)).reason,Exit.Budget);equal(fp(empty),emptyBefore);assert.equal((await empty.vm.run(1n)).reason,Exit.Budget);equal(fp(empty),{...emptyBefore,tags:0xffff});empty.vm.dispose();

const pending=program([0xf3,0x0f,0x58,0xc1,0xf3,0x0f,0xd6,0xd0]),pendingBefore=seed();pendingBefore.cw=0x37e;setFp(pending,pendingBefore);pending.vm.xmm(0,encode(0x3f800000n,16));pending.vm.xmm(1,encode(0x33800000n,16));pending.vm.xmm(2,encode(samples[2],16));assert.equal((await pending.vm.run(2n)).reason,Exit.Exception);assert.equal(pending.vm.ip,base+4n);assert.equal(pending.vm.work,1n);assert.equal(pending.vm.retired,1n);assert.equal(pending.vm.mxcsr,0x1fa0,'SSE proceeds independently of the pending x87 exception');equal(fp(pending),pendingBefore);equal(pending.vm.xmm(2),encode(samples[2],16));pending.vm.dispose();
const simdStatus=program([0x0f,0xfc,0xc1]),simdBefore=seed();setFp(simdStatus,simdBefore);simdStatus.vm.mxcsr=0x20;assert.equal((await simdStatus.vm.run(1n)).reason,Exit.Budget);assert.equal(simdStatus.vm.mxcsr,0x20,'unmasked sticky SSE precision does not affect MMX');equal(fp(simdStatus),changed(simdBefore,0,reference('add',8,simdBefore.regs[0]&mask(64),simdBefore.regs[1]&mask(64))));simdStatus.vm.dispose();

const resident=program([0x0f,0xfc,0xc1,0xff,0xe2]),{vm}=resident;vm.map(data,4096n,7);vm.write(data,[0xf3,0x0f,0x58,0xc1,0xf3,0x0f,0xd6,0xd0,0x83,0xe9,1,0x74,2,0xff,0xe7,0x0f,0x77,0xcc]);vm.register(2,data);vm.register(7,base);vm.register(1,2n);vm.xmm(1,encode(0x33800000n,16));
assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const installed=vm.stats();
for(const count of [12,2]){
 const before=seed();before.regs[0]=(0x1234n<<64n)|0x0707070707070707n;before.regs[1]=(0x4321n<<64n)|0x0101010101010101n;setFp(resident,before);vm.ip=base;vm.register(1,BigInt(count));vm.mxcsr=0x5f80;vm.xmm(0,encode(0x3f800000n,16));vm.xmm(2,encode(samples[2],16));resident.calls.begin=0;resident.calls.link=0;
 resident.watch.onLink=()=>{equal(fp(resident),before,'x87 backing remains at entry across resident chunks');equal(vm.xmm(0),encode(0x3f800000n,16));equal(vm.xmm(2),encode(samples[2],16));assert.equal(vm.mxcsr,0x5f80);};
 assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const value=BigInt(7+count)*0x0101010101010101n;equal(fp(resident),{...changed(before,0,value),tags:0xffff});equal(vm.xmm(2),encode(value,16));equal(vm.xmm(0),encode(0x3f800000n+BigInt(count),16));assert.equal(vm.mxcsr,0x5fa0);assert.equal(resident.calls.begin,1);assert.equal(resident.calls.link,2);assert.deepEqual(vm.stats(),installed);
}vm.dispose();

const owners=[program([0x0f,0xfc,0xc1]),program([0x0f,0xfc,0xc1])],states=owners.map((test,n)=>{const before=seed();before.regs[0]=(0x1234n<<64n)|BigInt(n+1);before.regs[1]=(0x4321n<<64n)|0x0101010101010101n;setFp(test,before);return before;});await Promise.all(owners.map(({vm})=>vm.run(1n)));owners.forEach((test,n)=>{equal(fp(test),changed(states[n],0,reference('add',8,BigInt(n+1),0x0101010101010101n)));test.vm.dispose();});
const rewritten=program([0x0f,0x7f,0x03,0x04,1]),rewriteBefore=seed();rewriteBefore.regs[0]=(0x1234n<<64n)|0xccn;setFp(rewritten,rewriteBefore);rewritten.vm.register(0,123n);rewritten.vm.register(3,base+3n);assert.equal(core.wire_compile(rewritten.handle),0);const oldTicket=core.wire_ticket(rewritten.handle);assert.equal((await rewritten.vm.run(10n)).reason,Exit.Unsupported);assert.equal(rewritten.vm.work,1n);assert.equal(rewritten.vm.retired,1n);assert.equal(rewritten.vm.ip,base+3n);assert.equal(rewritten.vm.register(0),123n);equal(fp(rewritten),changed(rewriteBefore,-1,0n));assert.equal(core.wire_ready(rewritten.handle,oldTicket),0);rewritten.vm.dispose();

const isolated=program([0x0f,0x77]),isolatedBefore=seed();setFp(isolated,isolatedBefore);assert.equal((await isolated.vm.run(0n)).reason,Exit.Budget);equal(fp(isolated),isolatedBefore);assert.equal((await isolated.vm.run(1n)).reason,Exit.Budget);equal(fp(isolated),{...isolatedBefore,tags:0xffff});isolated.vm.dispose();

const conversions=[
 {name:'cvtpi2ps',opcode:0x2a,bits:32,toFloat:true},
 {name:'cvtpi2pd',opcode:0x2a,bits:64,toFloat:true},
 {name:'cvtps2pi',opcode:0x2d,bits:32},
 {name:'cvttps2pi',opcode:0x2c,bits:32,trunc:true},
 {name:'cvtpd2pi',opcode:0x2d,bits:64},
 {name:'cvttpd2pi',opcode:0x2c,bits:64,trunc:true},
];
const conversionCode=(op,memory)=>[...(op.bits===64?[0x66]:[]),0x0f,op.opcode,memory?0x03:0xc1];
const conversionSize=op=>op.toFloat||op.bits===32?8:16;
const changesFp=(op,memory)=>op.name!=='cvtpi2pd'||!memory;
const floatInputs=bits=>bits===32?
 [0n,0x80000000n,0x3fc00000n,0xc0200000n,0x7fc00123n,0x7f800123n,1n,0x80000001n,0x4effffffn,0x4f000000n,0xcf000000n,0xcf000001n,0x7f800000n,0x7f7fffffn]:
 [0n,0x8000000000000000n,0x3ff8000000000000n,0xc004000000000000n,0x7ff8000000000123n,0x7ff0000000000123n,1n,0x8000000000000001n,0x41dfffffffe00000n,0x41dfffffffe00001n,0x41e0000000000000n,0xc1e0000000000000n,0xc1e0000000100000n,0x7ff0000000000000n,0x7fefffffffffffffn];
const pair=(values,bits)=>values[0]|(values[1]<<BigInt(bits));
for(const op of conversions)for(const mode of [32,64])for(const memory of [false,true]){
 const test=program([0x04,1,...conversionCode(op,memory)],mode),{vm}=test;if(memory)vm.map(data,4096n);
 const inputs=op.toFloat?[0n,1n,0xffffffffn,0x7fffffffn,0x80000000n,0x01000001n,0xfeffffffn,0x01000003n]:floatInputs(op.bits),sourceBits=op.toFloat?32:op.bits;
 for(const csr of [0x1f80,0x3f80,0x5f80,0x7f80,0x9fc0,0x1f00,0x0f80,0x1e80,0,0x1fa1])for(let n=0;n<inputs.length;n++){
  const values=[inputs[n],inputs[(n+3)%inputs.length]],raw=pair(values,sourceBits),before=seed(),oldXmm=0x7f8001237fc004560102030405060708n;
  if(op.toFloat)before.regs[1]=(0x1745n<<64n)|raw;setFp(test,before);vm.xmm(0,encode(oldXmm,16));vm.xmm(1,encode(raw|(sourceBits===32?0x7f8001237f800123n<<64n:0n),16));
  vm.ip=base;vm.mxcsr=csr;vm.register(0,127n);vm.flags=0x602n;vm.register(3,data);if(memory)vm.write(data,encode(raw,conversionSize(op)));
  const results=values.map(value=>op.toFloat?round(sign(value,32),1n,false,op.bits,csr):integer(value,op.bits,32,csr,!!op.trunc)),flags=results.reduce((v,r)=>v|r.flags,0),fault=!!(flags&~(csr>>7)),out=pair(results.map(r=>r.value),op.toFloat?op.bits:32),work=vm.work,retired=vm.retired,numeric=test.calls.numeric;
  const label=`${op.name}/${mode}/${memory}/${csr.toString(16)}/${n}`;
  assert.equal((await vm.run(2n)).reason,fault?Exit.Exception:Exit.Budget,label);assert.equal(vm.mxcsr,reported(csr,flags),label);assert.equal(vm.flags,0xe92n);assert.equal(vm.work-work,fault?1n:2n);assert.equal(vm.retired-retired,fault?1n:2n);assert.equal(test.calls.numeric-numeric,1);
  const expectedFp=fault||!changesFp(op,memory)?before:changed(before,op.toFloat?-1:0,out);
  equal(fp(test),expectedFp,label);
  const expectedXmm=!op.toFloat||fault?oldXmm:op.bits===32?(oldXmm&~mask(64))|out:out;
  equal(vm.xmm(0),encode(expectedXmm,16),label);equal(vm.xmm(1),encode(raw|(sourceBits===32?0x7f8001237f800123n<<64n:0n),16),'source remains unchanged');
  if(fault){assert.equal(core.wire_exception(test.handle),20);assert.equal(vm.ip,base+2n);}
 }
 if(memory){assert.equal(test.calls.resolve,1);assert.equal(test.calls.load,0);}vm.dispose();
}

for(const op of conversions)for(const memory of [false,true]){
 const test=program(conversionCode(op,memory)),{vm}=test,before=seed();before.cw=0x37e;setFp(test,before);vm.mxcsr=0;vm.xmm(0,encode(samples[3],16));vm.xmm(1,encode(mask(128),16));vm.map(data,4096n);vm.register(3,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 const wait=changesFp(op,memory);assert.equal((await vm.run(1n)).reason,wait?Exit.Exception:Exit.Fault,op.name);equal(fp(test),before);assert.equal(vm.mxcsr,0);assert.equal(vm.work,0n);assert.equal(vm.retired,0n);assert.equal(test.calls.numeric,0);
 if(wait){assert.equal(core.wire_exception(test.handle),17);assert.equal(test.calls.resolve,0);}else assert.equal(core.wire_fault_kind(test.handle),4);
 if(memory&&wait){before.cw=0x37f;setFp(test,before);assert.equal((await vm.run(1n)).reason,Exit.Fault);assert.equal(core.wire_fault_kind(test.handle),4,'#MF leaves guard available');equal(fp(test),before);}
 vm.dispose();
}

for(const op of conversions)for(const offset of conversionSize(op)===8?[4088,4092,4095]:[4080]){
 const test=program(conversionCode(op,true)),{vm}=test,before=seed(),raw=op.toFloat?0x00000002ffffffffn:op.bits===32?0x40000000bf800000n:0x4000000000000000bff0000000000000n;
 vm.map(data,8192n);vm.write(data+BigInt(offset),encode(raw,conversionSize(op)));vm.register(3,data+BigInt(offset));setFp(test,before);vm.xmm(0,encode(mask(128),16));assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);
 const crosses=offset+conversionSize(op)>4096;
 assert.equal((await vm.run(1n)).reason,crosses?Exit.Fault:Exit.Budget,op.name);
 if(crosses){equal(fp(test),before);equal(vm.xmm(0),encode(mask(128),16));assert.equal(vm.mxcsr,0x1f80);assert.equal(vm.work,0n);assert.equal(test.calls.numeric,0);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);}
 if(op.toFloat){const values=lanes(raw,32).map(value=>round(sign(value,32),1n,false,op.bits,0x1f80).value),out=pair(values,op.bits);equal(vm.xmm(0),encode(op.bits===32?(mask(64)<<64n)|out:out,16));equal(fp(test),changesFp(op,true)?changed(before,-1,0n):before);}
 else equal(fp(test),changed(before,0,0x00000002ffffffffn));
 assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);vm.dispose();
}
for(const op of conversions.filter(op=>!op.toFloat&&op.bits===64)){
 const test=program(conversionCode(op,true)),before=seed();setFp(test,before);test.vm.map(data,4096n);test.vm.register(3,data+1n);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 assert.equal((await test.vm.run(1n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),14);equal(fp(test),before);assert.equal(test.calls.resolve,0);assert.equal(test.calls.numeric,0);test.vm.dispose();
}

for(const op of conversions){
 const code=[...(op.bits===64?[0x66]:[]),op.toFloat?0x44:0x41,0x0f,op.opcode,op.toFloat?0xc7:0xf8],test=program(code),{vm}=test,before=seed();before.regs[7]=(0x7312n<<64n)|0x00000002ffffffffn;setFp(test,before);
 const raw=op.bits===32?0x40000000bf800000n:0x4000000000000000bff0000000000000n;vm.xmm(8,encode(raw,16));assert.equal((await vm.run(1n)).reason,Exit.Budget);
 if(op.toFloat){equal(fp(test),changed(before,-1,0n));equal(vm.xmm(8),encode(op.bits===32?0x40000000bf800000n:raw,16));}else equal(fp(test),changed(before,7,0x00000002ffffffffn));vm.dispose();
}

const conversionResident=program([0x0f,0x2d,0xc1,0xff,0xe2]),conversionVm=conversionResident.vm;
conversionVm.map(data,4096n,7);conversionVm.write(data,[0x0f,0x2a,0xd0,0x83,0xe9,1,0x74,2,0xff,0xe7,0x0f,0x77,0xcc]);conversionVm.register(2,data);conversionVm.register(7,base);conversionVm.register(1,2n);conversionVm.xmm(1,encode(0xc02000003fc00000n,16));
assert.equal((await conversionVm.run(100n)).reason,Exit.Unsupported);const conversionStats=conversionVm.stats();
for(const count of [2,12]){
 const before=seed();setFp(conversionResident,before);conversionVm.ip=base;conversionVm.register(1,BigInt(count));conversionVm.xmm(2,encode(mask(128),16));conversionVm.mxcsr=0x1f80;conversionResident.calls.begin=0;conversionResident.calls.link=0;
 conversionResident.watch.onLink=()=>{equal(fp(conversionResident),before);equal(conversionVm.xmm(2),encode(mask(128),16));assert.equal(conversionVm.mxcsr,0x1f80);};
 assert.equal((await conversionVm.run(1000n)).reason,Exit.Unsupported);equal(fp(conversionResident),{...changed(before,0,0xfffffffe00000002n),tags:0xffff});equal(conversionVm.xmm(2),encode((mask(64)<<64n)|0xc000000040000000n,16));assert.equal(conversionVm.mxcsr,0x1fa0);assert.equal(conversionResident.calls.begin,1);assert.equal(conversionResident.calls.link,2);assert.deepEqual(conversionVm.stats(),conversionStats);
}conversionVm.dispose();

console.log(`${checks} MMX execution checks passed: integer/float conversions, physical x87 aliases, tags/TOP, #MF/#XM, precise faults, resident isolation, and source revocation`);
