import assert from 'node:assert/strict';
import {Exit} from '../web/runtime.mjs';
import {core,stateLayout,base,data,program,fp} from './x87-harness.mjs';
import {cases,reference,specs} from './string-compare-reference.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const prefix=[0x83,0xc6,1];
const vector=Uint8Array.from({length:256},(_,n)=>(n*29+13)&255);
const fpu=()=>({cw:0x37e,sw:0x8081,tags:0x93a5,regs:Array.from({length:8},(_,n)=>BigInt(n+1)|(0x12345678n<<80n)),ip:0x12345678n,dp:0x87654321n,opcode:0x725});
function instruction(spec,control,{mode=64,memory=false,wide=false,left=1,right=2,baseRegister=3}={}){
 const rm=memory?baseRegister:right,rex=(wide?8:0)|(left>=8?4:0)|(rm>=8?1:0);
 return [0x66,...(mode===64&&rex?[0x40|rex]:[]),0x0f,0x3a,spec.opcode,(memory?0:0xc0)|((left&7)<<3)|(rm&7),control];
}
function simd(test,value){const p=core.wire_state(test.handle)+stateLayout[9],out=new Uint8Array(core.memory.buffer,p,256);if(value)out.set(value);return out.slice();}
function setup(test,input,options={}){
 const {vm}=test,{mode=64,memory=false,left=1,right=2,baseRegister=3}=options;vm.ip=base;vm.flags=0x602n;vm.register(0,input.la);vm.register(2,input.lb);vm.register(1,mode===64?0xfedcba9876543210n:0x76543210n);vm.register(6,0x7fffffffn);simd(test,vector);vm.xmm(left,input.a);if(memory){vm.register(baseRegister,data+1n);vm.write(data+1n,input.b);}else vm.xmm(right,input.b);vm.mxcsr=0x5f01;const before=fpu();fp(test,before);return {fp:before,vectors:simd(test),rax:vm.register(0),rdx:vm.register(2),rcx:vm.register(1)};
}
function verify(test,input,before,spec,control,options={}){
 const {vm}=test,expected=reference(spec,control,input.a,input.b,before.rax,before.rdx,options.wide),vectors=before.vectors.slice();if(spec.masked)vectors.set(expected.mask,0);
 equal(simd(test),vectors,'only the implicit mask destination changes');equal(vm.register(1),spec.masked?before.rcx:BigInt(expected.index));equal(vm.register(0),before.rax);equal(vm.register(2),before.rdx);equal(vm.flags,0x602n|BigInt(expected.flags));equal(vm.mxcsr,0x5f01);equal(fp(test),before.fp);return expected;
}

for(const mode of [32,64])for(const spec of specs)for(const memory of [false,true])for(let control=0;control<128;control++){
 const options={mode,memory},test=program([...prefix,...instruction(spec,control,options)],mode),{vm}=test;if(memory)vm.map(data,4096n);
 for(const input of cases){const before=setup(test,input,options),work=vm.work;assert.equal((await vm.run(2n)).reason,Exit.Budget,`${mode}/${spec.opcode}/${memory}/${control}`);verify(test,input,before,spec,control,options);equal(vm.work-work,2n);equal(vm.register(6),0x80000000n);}
 equal(test.calls.numeric,0);equal(test.calls.fp_area,0);equal(test.calls.load,0);equal(test.calls.store,0);equal(test.calls.resolve,memory?1:0);vm.dispose();
}
for(const spec of specs.filter(s=>s.explicit))for(let control=0;control<128;control++){
 const options={wide:true,left:14,right:15},test=program([...prefix,...instruction(spec,control,options)]),{vm}=test;
 for(const input of cases.slice(3)){const before=setup(test,input,options);assert.equal((await vm.run(2n)).reason,Exit.Budget);verify(test,input,before,spec,control,options);}vm.dispose();
}

for(const spec of specs)for(const control of [0,0x0c,0x18,0x3d,0x40,0x7f])for(const alias of ['left','right']){
 const options={left:alias==='left'?0:1,right:alias==='right'?0:2,wide:!spec.explicit},input=cases[4],test=program([...prefix,...instruction(spec,control|128,options)]),{vm}=test,before=setup(test,input,options);assert.equal((await vm.run(2n)).reason,Exit.Budget);verify(test,input,before,spec,control,options);vm.dispose();
}
for(const spec of specs)for(const control of [0,0x0c,0x18,0x3d,0x40,0x7f]){
 const input={...cases[4],b:cases[4].a},options={left:3,right:3},test=program([...prefix,...instruction(spec,control,options)]),{vm}=test,before=setup(test,input,options);assert.equal((await vm.run(2n)).reason,Exit.Budget);verify(test,input,before,spec,control,options);vm.dispose();
}

for(const mode of [32,64])for(const spec of specs){
 const options={mode,memory:true,baseRegister:1},test=program([...prefix,...instruction(spec,0x18,options)],mode),{vm}=test,input=cases[2];vm.map(data,4096n);const before=setup(test,input,options);assert.equal((await vm.run(2n)).reason,Exit.Budget,'ECX destination can also address the memory source');verify(test,input,before,spec,0x18,options);vm.dispose();
}
for(const spec of specs)for(const control of [0,0x0c,0x18,0x3d,0x40,0x7f]){
 const opcodes=[0x90,0x92,0x94,0x98,0x9a,0x97],test=program([...prefix,0xf9,...instruction(spec,control),...opcodes.flatMap((opcode,n)=>[0x0f,opcode,0x47,n])]),{vm}=test;vm.map(data,4096n);
 for(const input of cases.slice(0,4)){
  const before=setup(test,input);vm.register(7,data);assert.equal((await vm.run(9n)).reason,Exit.Budget);const {flags}=verify(test,input,before,spec,control),expected=[(flags>>11)&1,flags&1,(flags>>6)&1,(flags>>7)&1,(flags>>2)&1,Number(!(flags&65))];equal(Uint8Array.from(opcodes,(_,n)=>Number(core.wire_load(test.handle,data+BigInt(n),1,1))),Uint8Array.from(expected),'SETcc consumes string FLAGS after a partially overridden arithmetic recipe');
 }vm.dispose();
}
for(const mode of [32,64])for(const spec of specs){
 const options={mode,memory:true},test=program([...prefix,...instruction(spec,0,options)],mode),{vm}=test,input=cases[0];vm.map(data,8192n);const before=setup(test,input,options),address=data+4088n;vm.register(3,address);vm.write(address,input.b);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,1),0);
 assert.equal((await vm.run(10n)).reason,Exit.Fault,'zero length and null elements do not suppress full m128 access');equal(vm.ip,base+3n);equal(vm.work,1n);equal(vm.retired,1n);equal(vm.flags,0xe96n);equal(simd(test),before.vectors);equal(vm.register(1),before.rcx);equal(fp(test),before.fp);equal(vm.mxcsr,0x5f01);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,4),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);verify(test,input,before,spec,0,options);vm.dispose();
}
for(const spec of specs)for(const guard of [false,true]){
 const options={memory:true},test=program(instruction(spec,0x3c,options)),{vm}=test,input=cases[0];vm.map(data,4096n);const before=setup(test,input,options);assert.equal(core.wire_protect(test.handle,data,4096n,guard?0x104:1),0);assert.equal((await vm.run(1n)).reason,Exit.Fault,'unaligned strings report data faults without an alignment #GP');equal(vm.work,0n);equal(vm.retired,0n);equal(vm.flags,0x602n);equal(simd(test),before.vectors);equal(vm.register(1),before.rcx);equal(fp(test),before.fp);vm.dispose();
}
for(const spec of specs){
 const options={memory:true},test=program(instruction(spec,0x4c,options)),{vm}=test,input=cases[3];vm.map(data,8192n);const before=setup(test,input,options);vm.register(3,data+4080n);vm.write(data+4080n,input.b);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);verify(test,input,before,spec,0x4c,options);core.wire_load(test.handle,data+4096n,1,1);equal(core.wire_fault_kind(test.handle),4,'m128 does not access the next byte');vm.dispose();
}

for(const mode of [32,64])for(const spec of specs){
 const options={mode,memory:true},bytes=instruction(spec,0,options),test=program([0x64,0x67,...bytes],mode),{vm}=test,input=cases[1];vm.map(data,4096n);const before=setup(test,input,options);new DataView(core.memory.buffer).setBigUint64(core.wire_state(test.handle)+stateLayout[6],data,true);vm.register(mode===64?3:5,0x1234000000000001n);if(mode===32)vm.register(7,0n);assert.equal((await vm.run(1n)).reason,Exit.Budget);verify(test,input,before,spec,0,options);vm.dispose();
}
for(const spec of specs){
 const empty=program(instruction(spec,0,{memory:true})),before=fpu();fp(empty,before);equal((await empty.vm.run(0n)).reason,Exit.Budget);equal(empty.calls.resolve,0);equal(fp(empty),before);empty.vm.dispose();
 const locked=program([0xf0,...instruction(spec,0)]);await assert.rejects(locked.vm.run(1n),/invalid instruction/);equal(locked.vm.work,0n);locked.vm.dispose();
}

for(const control of [0,4,8,12]){
 const test=program(specs.flatMap(spec=>instruction(spec,control)));assert.equal(core.wire_compile(test.handle),0);const ticket=core.wire_ticket(test.handle),bytes=new Uint8Array(core.memory.buffer,core.wire_module(test.handle,ticket),core.wire_module_len(test.handle,ticket)).slice(),imports=WebAssembly.Module.imports(new WebAssembly.Module(bytes));equal(imports.filter(i=>i.kind==='function'&&i.module==='core'),[],'register string comparisons import no core functions');test.vm.dispose();
}

for(const mode of [32,64]){
 const test=program([0x66,0x0f,0x6f,0xcb,...instruction(specs[2],0x40,{mode}),0xff,0xe2],mode),{vm}=test;vm.map(data,4096n,7);vm.write(data,[...instruction(specs[3],8,{mode,left:0}),0x83,0xed,1,0x74,2,0xff,0xe6,0xcc]);vm.register(2,data);vm.register(6,base);vm.register(5,2n);vm.xmm(2,cases[4].b);vm.xmm(3,cases[4].a);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){
  simd(test,vector);vm.xmm(2,cases[4].b);vm.xmm(3,cases[4].a);const before=simd(test),beforeFp=fpu();fp(test,beforeFp);vm.ip=base;vm.flags=0x602n;vm.register(5,BigInt(count));vm.mxcsr=0x5f01;test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>{equal(simd(test),before,'ordinary links preserve entry XMM backing');equal(fp(test),beforeFp);};
  assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const expected=before.slice(),mask=reference(specs[2],0x40,cases[4].a,cases[4].b).mask,index=reference(specs[3],8,mask,cases[4].b).index;expected.set(mask,0);expected.set(cases[4].a,16);equal(simd(test),expected);equal(vm.register(1),BigInt(index));equal(vm.flags,0x646n);equal(vm.mxcsr,0x5f01);equal(fp(test),beforeFp);equal(test.calls.begin,1);equal(test.calls.link,2);equal(vm.stats(),stats);
 }vm.dispose();
}

console.log(`${checks} SSE4.2 string-comparison checks passed`);
