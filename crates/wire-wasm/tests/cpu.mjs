import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const mask=bits=>(1n<<BigInt(bits))-1n;
const prefix=[0x83,0xc6,1];
const seed=()=>({cw:0x37e,sw:0x8081,tags:0x93a5,regs:Array.from({length:8},(_,n)=>BigInt(n+1)|(0x12345678n<<80n)),ip:0x12345678n,dp:0x87654321n,opcode:0x725});
const vectors=Uint8Array.from({length:256},(_,n)=>(n*29+13)&255);
function simd(test,value){const p=core.wire_state(test.handle)+stateLayout[9],out=new Uint8Array(core.memory.buffer,p,256);if(value)out.set(value);return out.slice();}
function setup(test){const {vm}=test;vm.ip=base;vm.flags=0x602n;for(let n=0;n<8;n++)vm.register(n,0xfedcba9876543210n+BigInt(n));vm.register(6,0x7fffffffn);fp(test,seed());simd(test,vectors);vm.mxcsr=0x5f01;}
function isolated(test){equal(fp(test),seed());equal(simd(test),vectors);equal(test.vm.mxcsr,0x5f01);}
function bytes(test,address,length){return Uint8Array.from({length},(_,n)=>Number(core.wire_load(test.handle,address+BigInt(n),1,1)));}

const word=s=>Buffer.from(s).readUInt32LE();
const leaves=new Map([
 [0,[0x15,word('Wire'),word('CPU '),word('Virt')]],
 [1,[0x600,0x10000,0x982201,0x7808111]],
 [7,[0,0x2000,0,0]],
 [0x15,[1,1,10000000,0]],
 [0x80000000,[0x80000008,0,0,0]],
 [0x80000001,[0,0,1,0x28100000]],
 [0x80000002,['Wire',' Was','m Vi','rtua'].map(word)],
 [0x80000003,['l CP','U   ','    ','    '].map(word)],
 [0x80000004,Array(4).fill(word('    '))],
 [0x80000007,[0,0,0,256]],
 [0x80000008,[0x3030,0,0,0]],
]);
for(const mode of [32,64]){
 const test=program([...prefix,0x0f,0xa2],mode),{vm}=test;
 for(const leaf of [...leaves.keys(),2,4,0xb,0xd,0x16,0x40000000,0x80000005,0xffffffff])for(const subleaf of [0,1,0xffffffff]){
  setup(test);vm.register(0,(0x12345678n<<32n)|BigInt(leaf));vm.register(1,(0x87654321n<<32n)|BigInt(subleaf));const work=vm.work;assert.equal((await vm.run(2n)).reason,Exit.Budget);equal([0,3,1,2].map(n=>vm.register(n)),(leaf===7&&subleaf!==0?[0,0,0,0]:leaves.get(leaf)||[0,0,0,0]).map(BigInt));equal(vm.flags,0xe96n);equal(vm.register(6),0x80000000n);equal(vm.work-work,2n);isolated(test);
 }
 equal(test.calls.numeric,0);equal(test.calls.resolve,0);equal(test.calls.fp_area,0);vm.dispose();
}

function reflect(value,bits){let result=0n;for(let n=0;n<bits;n++){result=(result<<1n)|(value&1n);value>>=1n;}return result;}
function crc(initial,value,bits){
 let dividend=(reflect(initial&mask(32),32)<<BigInt(bits))^(reflect(value&mask(bits),bits)<<32n);
 for(let n=31+bits;n>=32;n--)if(dividend&(1n<<BigInt(n)))dividend^=0x11edc6f41n<<BigInt(n-32);
 return reflect(dividend,32);
}
const pop=value=>BigInt(value.toString(2).replaceAll('0','').length);
const values=[0n,1n,mask(64),0x0123456789abcdefn,0x8000000080000080n,0xaaaaaaaa55555555n];
const crcCode=(bits,dest,memory=false,alias=false)=>[...(bits===16?[0x66]:[]),0xf2,...(dest===64?[0x48]:[]),0x0f,0x38,bits===8?0xf0:0xf1,memory?0x03:alias?0xc0:0xc3];
const popCode=(bits,memory=false,alias=false)=>[0xf3,...(bits===16?[0x66]:bits===64?[0x48]:[]),0x0f,0xb8,memory?0x03:alias?0xc0:0xc3];
const scanCode=(bits,reverse,memory=false)=>[0xf3,...(bits===16?[0x66]:bits===64?[0x48]:[]),0x0f,reverse?0xbd:0xbc,memory?0x03:0xc3];
for(const mode of [32,64])for(const bits of [16,32,64])for(const reverse of [false,true])for(const memory of [false,true]){
 if(mode===32&&bits===64)continue;
 const test=program([...prefix,...scanCode(bits,reverse,memory)],mode),{vm}=test;if(memory)vm.map(data,8192n);
 for(const source of [0n,1n,1n<<BigInt(bits-1),0x90n]){
  setup(test);const old=vm.register(0);if(memory){vm.register(3,data+4095n);vm.write(data+4095n,encode(source,bits/8));}else vm.register(3,source);
  let index=0;for(let n=0;n<bits;n++)if(source&(1n<<BigInt(n))){index=n;if(!reverse)break;}
  assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(vm.register(0),source===0n?old:bits===16?(old&~mask(16))|BigInt(index):BigInt(index),'unpublished BMI1/LZCNT encodings use BSF/BSR');equal(vm.flags,source===0n?0xed6n:0xe96n);isolated(test);
 }
 equal(test.calls.numeric,0);vm.dispose();
}
for(const reverse of [false,true]){
 const test=program([...prefix,...scanCode(64,reverse,true)]),{vm}=test;setup(test);vm.map(data,4096n);vm.register(3,data+4095n);const old=vm.register(0);assert.equal((await vm.run(2n)).reason,Exit.Fault);equal(vm.ip,base+3n);equal(vm.work,1n);equal(vm.register(0),old);equal(vm.flags,0xe96n);vm.dispose();
}
for(const [code,functions] of [
 [[0x0f,0xa2,...popCode(32),0x0f,0xae,0xe8,0x0f,0xae,0xf0,0x0f,0xae,0xf8,0x0f,0x18,0x03],[]],
 [[...crcCode(8,32),...crcCode(16,32),...crcCode(32,32),...crcCode(64,64)],['core.wire_crc8','core.wire_crc16','core.wire_crc32','core.wire_crc64']],
 [[0x0f,0x31,0x0f,0x01,0xf9],['platform.clock']],
]){
 const test=program(code);assert.equal(core.wire_compile(test.handle),0);const ticket=core.wire_ticket(test.handle),module=new WebAssembly.Module(new Uint8Array(core.memory.buffer,core.wire_module(test.handle,ticket),core.wire_module_len(test.handle,ticket)).slice());equal(WebAssembly.Module.imports(module).filter(i=>i.kind==='function').map(i=>`${i.module}.${i.name}`),[...functions,'jit.materialize'],'system instructions import only concrete value/time and FLAGS functions');test.vm.dispose();
}
for(const mode of [32,64])for(const kind of ['crc','pop'])for(const bits of kind==='crc'?[8,16,32,64]:[16,32,64])for(const dest of kind==='crc'?(bits===8?[32,64]:bits===64?[64]:[32]):[bits]){
 if(mode===32&&(dest===64||bits===64))continue;
 for(const memory of [false,true]){
  const code=kind==='crc'?crcCode(bits,dest,memory):popCode(bits,memory),test=program([...prefix,...code],mode),{vm}=test;if(memory)vm.map(data,8192n);
  for(const initial of values)for(const source of values)for(const offset of memory?[1,4093]:[0]){
   setup(test);vm.register(0,initial);if(memory){vm.register(3,data+BigInt(offset));vm.write(data+BigInt(offset),encode(source,bits/8));}else vm.register(3,source);const old=vm.register(0),input=source&mask(bits),expected=kind==='crc'?crc(old,input,bits):pop(input);assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(vm.register(0),kind==='pop'&&bits===16?(old&~mask(16))|expected:expected);equal(vm.flags,kind==='crc'?0xe96n:input===0n?0x642n:0x602n);isolated(test);
  }equal(test.calls.numeric,kind==='crc'?values.length**2*(memory?2:1):0);equal(test.calls.fp_area,0);vm.dispose();
 }
 const test=program(kind==='crc'?crcCode(bits,dest,false,true):popCode(bits,false,true),mode),{vm}=test;for(const initial of values){setup(test);vm.register(0,initial);const old=vm.register(0),result=kind==='crc'?crc(old,old,bits):pop(old&mask(bits));assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),kind==='pop'&&bits===16?(old&~mask(16))|result:result);}vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xf2,0x0f,0x38,0xf0,0xc4],mode),{vm}=test;for(const initial of values){vm.ip=base;vm.register(0,initial);const old=vm.register(0);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),crc(old,(old>>8n)&255n,8),'CRC32 reads AH before replacing EAX');}vm.dispose();
}
let check=mask(32);for(const ch of Buffer.from('123456789'))check=crc(check,BigInt(ch),8);equal(check^mask(32),0xe3069283n,'independent CRC32C check value');
for(const mode of [32,64]){
 const body=[...crcCode(8,32,true),...(mode===64?[0x48]:[]),0xff,0xc3,0x83,0xe9,1],test=program([...body,0x75,256-body.length-2],mode),{vm}=test;vm.map(data,4096n);vm.write(data,Buffer.from('123456789'));vm.register(3,data);vm.register(0,mask(32));vm.register(1,9n);assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);equal(vm.register(0)^mask(32),0xe3069283n);equal(vm.register(3),data+9n);equal(vm.work,36n);equal(test.calls.resolve,1);equal(test.calls.load,0);vm.dispose();
}

for(const mode of [32,64])for(const [kind,bits,code] of [['crc',64,crcCode(64,64,true)],['crc',32,crcCode(32,32,true)],['pop',16,popCode(16,true)],['pop',32,popCode(32,true)],['pop',64,popCode(64,true)]]){
 if(mode===32&&bits===64)continue;
 const test=program([...prefix,...code],mode),{vm}=test;setup(test);vm.map(data,8192n);const address=data+4095n;vm.write(address,encode(mask(bits),bits/8));vm.register(3,address);const old=vm.register(0);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,1),0);assert.equal((await vm.run(10n)).reason,Exit.Fault);equal(vm.ip,base+3n);equal(vm.work,1n);equal(vm.retired,1n);equal(vm.register(0),old);equal(vm.flags,0xe96n);isolated(test);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),kind==='crc'?crc(old,mask(bits),bits):bits===16?(old&~mask(16))|BigInt(bits):BigInt(bits));vm.dispose();
}
for(const [bits,code] of [[8,crcCode(8,32,true)],[16,crcCode(16,32,true)],[32,crcCode(32,32,true)],[64,crcCode(64,64,true)],[16,popCode(16,true)],[32,popCode(32,true)],[64,popCode(64,true)]]){
 const test=program(code),{vm}=test,address=data+4096n-BigInt(bits/8);vm.map(data,8192n);vm.write(address,encode(mask(bits),bits/8));vm.register(3,address);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);core.wire_load(test.handle,data+4096n,1,1);equal(core.wire_fault_kind(test.handle),4,'integer source accesses only its operand width');vm.dispose();
}

for(const mode of [32,64])for(const bits of [32,64]){
 if(mode===32&&bits===64)continue;
 const code=[...(bits===64?[0x48]:[]),0x0f,0xc3,0x03],test=program([...prefix,...code],mode),{vm}=test;vm.map(data,8192n);for(const offset of [0,1,4093]){setup(test);vm.register(0,0x123456789abcdef0n);vm.register(3,data+BigInt(offset));assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(bytes(test,data+BigInt(offset),bits/8),encode(0x123456789abcdef0n,bits/8));equal(vm.flags,0xe96n);isolated(test);}vm.dispose();
 const fail=program(code,mode),old=seed();fp(fail,old);fail.vm.map(data,8192n);fail.vm.write(data+4093n,new Uint8Array(bits/8).fill(0xa5));fail.vm.register(0,mask(bits));fail.vm.register(3,data+4093n);assert.equal(core.wire_protect(fail.handle,data+4096n,4096n,2),0);assert.equal((await fail.vm.run(1n)).reason,Exit.Fault);equal(fail.vm.work,0n);equal(fp(fail),old);assert.equal(core.wire_protect(fail.handle,data+4096n,4096n,4),0);equal(bytes(fail,data+4093n,bits/8),new Uint8Array(bits/8).fill(0xa5));fail.vm.dispose();
}
for(const mode of [32,64]){
 const code=[...prefix,0x0f,0xae,0xe8,0x0f,0xae,0xf0,0x0f,0xae,0xf8,...[0x03,0x0b,0x13,0x1b].flatMap(modrm=>[0x0f,0x18,modrm]),0x0f,0x0d,0x0b],test=program(code,mode),{vm}=test;setup(test);vm.register(3,0xffff000000000001n);assert.equal((await vm.run(9n)).reason,Exit.Budget);equal(vm.work,9n);equal(vm.flags,0xe96n);equal(test.calls.resolve,0,'prefetch hints do not access guest data');isolated(test);vm.dispose();
 const invalid=program([...prefix,0x0f,0x01,0xd0],mode);setup(invalid);const eax=invalid.vm.register(0),edx=invalid.vm.register(2);assert.equal((await invalid.vm.run(2n)).reason,Exit.Exception);equal(core.wire_exception(invalid.handle),7,'XGETBV is #UD when XSAVE/OSXSAVE are absent');equal(invalid.vm.work,1n);equal(invalid.vm.ip,base+3n);equal(invalid.vm.register(0),eax);equal(invalid.vm.register(2),edx);equal(invalid.vm.flags,0xe96n);invalid.vm.dispose();
}

const timestamps=[[0x0f,0x31],[0x0f,0x01,0xf9]];
for(const mode of [32,64])for(const op of timestamps){
 let sample=0n,calls=0,test,before;test=program([...prefix,...op],mode,{clock(){calls++;equal(new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]),before,'clock reads do not transact CPU backing');return sample;}});const {vm}=test;equal(vm.clockFrequency,10000000);
 for(const value of [0n,1n,0x12345678abcdef01n,mask(64)]){setup(test);sample=value;before=new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();const old=vm.register(1),count=calls;assert.equal((await vm.run(0n)).reason,Exit.Budget);equal(calls,count);assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(calls,count+1);equal(vm.register(0),value&mask(32));equal(vm.register(2),value>>32n);equal(vm.register(1),op.length===3?0n:old);equal(vm.flags,0xe96n);isolated(test);}
 vm.dispose();assert.throws(()=>vm.now(),/disposed/);
}
for(const bad of [()=>{throw new Error('clock failed');},()=>-1n,()=>1n<<64n,()=>1]){
 let failing=true,calls=0;const test=program([...prefix,...timestamps[1]],64,{clock(){calls++;return failing?bad():9n;}}),{vm}=test;setup(test);const before=[0,1,2].map(n=>vm.register(n));await assert.rejects(vm.run(2n));equal(vm.work,1n);equal(vm.retired,1n);equal(vm.ip,base+3n);equal(vm.flags,0xe96n);equal([0,1,2].map(n=>vm.register(n)),before);isolated(test);failing=false;assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),9n);equal(vm.register(2),0n);equal(vm.register(1),0n);equal(vm.work,2n);equal(calls,2);vm.dispose();
}
{
 let ticks=100n;const test=program(timestamps[0],64,{clock:()=>ticks}),{vm}=test;assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.ip=base;const before=[0,1,2].map(n=>vm.register(n));ticks=99n;await assert.rejects(vm.run(1n),/backwards/);equal(vm.work,1n);equal(vm.ip,base);equal([0,1,2].map(n=>vm.register(n)),before);ticks=100n;assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.work,2n);vm.dispose();
}
{
 const saved=Object.getOwnPropertyDescriptor(globalThis,'performance');let ms=5000;Object.defineProperty(globalThis,'performance',{configurable:true,value:{now:()=>ms}});try{const test=program(timestamps[0]),{vm}=test;equal(vm.now(),0n);ms+=1.5;equal(vm.now(),15000n);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),15000n);vm.dispose();}finally{Object.defineProperty(globalThis,'performance',saved);}
 assert.throws(()=>new WasmMachine(core,{clock:3}),/clock function/);
}
{
 let ticks=1n,observed=0;const test=program([0x89,0x07,...timestamps[1]],64,{clock(){observed++;equal(core.wire_load(test.handle,data,4,1),0x12345678n,'stores precede the platform clock sample');return ticks;}}),{vm}=test;vm.map(data,4096n);vm.register(7,data);vm.register(0,0x12345678n);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(observed,0);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(observed,1);equal(vm.register(0),ticks);vm.dispose();
}
for(const code of [[0x83,0xc7,1,0x0f,0xa2,0x83,0xd6,0],[0x83,0xc7,1,...timestamps[1],0x83,0xd6,0],[0x83,0xc7,1,...crcCode(32,32),0x83,0xd6,0]]){
 const test=program(code,64,{clock:()=>10n}),{vm}=test;vm.register(7,0xffffffffn);vm.register(6,5n);vm.flags=0x602n;assert.equal((await vm.run(3n)).reason,Exit.Budget);equal(vm.register(6),6n,'feature/time/CRC operations preserve incoming lazy carry');equal(vm.flags,0x606n);vm.dispose();
}

for(const mode of [32,64]){
 let calls=0;const test=program([...timestamps[0],0xff,0xe5],mode,{clock:()=>BigInt(++calls)}),{vm}=test;vm.map(data,4096n,7);vm.write(data,[0x0f,0xa2,...timestamps[1],0x83,0xef,1,0x74,2,0xff,0xe6,0xcc]);vm.register(5,data);vm.register(6,base);vm.register(7,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){setup(test);vm.register(5,data);vm.register(6,base);vm.register(7,BigInt(count));const before=new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();const start=calls;test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>equal(new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]),before,'clock and CPUID links preserve entry CPU backing');assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);equal(calls-start,count*2);equal(vm.register(0),BigInt(calls)&mask(32));equal(vm.register(2),0n);equal(vm.register(1),0n);equal(vm.register(7),0n);equal(vm.flags,0x646n);isolated(test);equal(test.calls.begin,1);equal(test.calls.link,2);equal(vm.stats(),stats);}vm.dispose();
}
for(const code of [[0x0f,0xa2],...timestamps,popCode(32),crcCode(32,32),[0x0f,0xae,0xf0],[0x0f,0xc3,0x03]]){
 const test=program([0xf0,...code]);await assert.rejects(test.vm.run(1n),/invalid instruction/);equal(test.vm.work,0n);test.vm.dispose();
}

console.log(`${checks} CPU feature/time/integer checks passed`);
