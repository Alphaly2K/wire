import assert from 'node:assert/strict';
import {Exit} from '../web/runtime.mjs';
import {core,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const mask=bits=>(1n<<BigInt(bits))-1n;
const values=[0n,1n<<79n,0x3fff8000000000000000n,0x7fff8000000000000000n,0x7fffc123456789abcdefn,0x3fff0000000000000001n,0x00008000000000000000n,1n];
const seed=()=>({cw:0x37f,sw:0x6f35,tags:0x93a5,regs:values.map((v,n)=>v|(BigInt(0xaabbccdd0000+n)<<80n)),ip:0x1234567887654321n,dp:0xfedcba9876543210n,opcode:0x725});
const specs=[{kind:'load-env',opcode:0xd9,modrm:0x23,restore:true,full:false},{kind:'store-env',opcode:0xd9,modrm:0x33,restore:false,full:false},{kind:'restore',opcode:0xdd,modrm:0x23,restore:true,full:true},{kind:'save',opcode:0xdd,modrm:0x33,restore:false,full:true}];
const code=(spec,short=false,prefix=[])=>[...prefix,...(short?[0x66]:[]),spec.opcode,spec.modrm];
const size=(spec,short)=>(short?14:28)+(spec.full?80:0);
const initial=before=>({...before,cw:0x37f,sw:0,tags:0xffff,ip:0n,dp:0n,opcode:0});
function status(cw,sw){return (sw&~0x8080)|((sw&~cw&63)?0x8080:0);}
function classification(bits){const exponent=Number(bits>>64n)&0x7fff,mantissa=bits&mask(64);return !exponent&&!mantissa?1:exponent>0&&exponent<0x7fff&&!!(mantissa>>63n)?0:2;}
function image(state,short,full,reserved=0){
 const bytes=new Uint8Array((short?14:28)+(full?80:0)).fill(reserved),v=new DataView(bytes.buffer),h=short?14:28;
 if(short){for(const [offset,value] of [[0,state.cw],[2,state.sw],[4,state.tags],[6,Number(state.ip&mask(16))],[8,0],[10,Number(state.dp&mask(16))],[12,0]])v.setUint16(offset,value,true);}
 else{for(const [offset,value] of [[0,state.cw],[4,state.sw],[8,state.tags],[16,0],[18,state.opcode&0x7ff],[24,0]])v.setUint16(offset,value,true);for(const offset of [2,6,10,26])v.setUint16(offset,0xffff,true);v.setUint32(12,Number(state.ip&mask(32)),true);v.setUint32(20,Number(state.dp&mask(32)),true);}
 if(full)for(let n=0;n<8;n++)bytes.set(encode(state.regs[((state.sw>>11)+n)&7],10),h+n*10);
 return bytes;
}
function restored(bytes,old,short,full){
 const v=new DataView(bytes.buffer,bytes.byteOffset,bytes.length),h=short?14:28,cw=(v.getUint16(0,true)&0x1f3f)|64,sw=v.getUint16(short?2:4,true),rawTags=v.getUint16(short?4:8,true),top=(sw>>11)&7;
 const regs=full?Array.from({length:8},(_,n)=>{const slot=(n+8-top)&7;return Array.from(bytes.slice(h+slot*10,h+slot*10+10)).reduceRight((a,b)=>(a<<8n)|BigInt(b),0n);}):[...old.regs];
 const tags=regs.reduce((out,bits,n)=>out|(((rawTags>>(n*2)&3)===3?3:classification(bits))<<(n*2)),0);
 return {...old,cw,sw:status(cw,sw),tags,regs,ip:BigInt(short?v.getUint16(6,true):v.getUint32(12,true)),dp:BigInt(short?v.getUint16(10,true):v.getUint32(20,true)),opcode:short?0:v.getUint16(18,true)&0x7ff};
}
function read(test,address,length){const bytes=new Uint8Array(length);for(let n=0;n<length;n++){bytes[n]=Number(core.wire_load(test.handle,address+BigInt(n),1,1));assert.equal(core.wire_fault_kind(test.handle),0);}return bytes;}
function simd(test,value){const p=core.wire_state(test.handle)+stateLayout[9],bytes=new Uint8Array(core.memory.buffer,p,256);if(value)bytes.set(value);return bytes.slice();}
const vector=Uint8Array.from({length:256},(_,n)=>(n*31+13)&255);

// Different TOPs and deliberately mismatched nonempty tags exercise physical
// tags versus the logical ten-byte register order in the saved image.
for(const mode of [32,64])for(const short of [false,true])for(const spec of specs){
 const test=program([0x04,1,...code(spec,short)],mode),{vm}=test;vm.map(data,8192n);
 for(let top=0;top<8;top++)for(const tags of [0xffff,0,0x5555,0xaaaa,0x93a5])for(const offset of [1,4089]){
  const before=seed(),source=seed(),address=data+BigInt(offset),length=size(spec,short);before.sw=(before.sw&~0x3800)|(top<<11);source.sw=(source.sw&~0x3800)|(((top+3)&7)<<11);source.tags=tags;source.cw=0xffff;source.opcode=0xffff;source.regs=source.regs.map((v,n)=>values[(n+3)&7]|(0x1234n<<80n));
  vm.write(address-1n,new Uint8Array(length+2).fill(0xa5));const input=image(source,short,spec.full,0xda);if(spec.restore)vm.write(address,input);else before.tags=tags;fp(test,before);simd(test,vector);vm.mxcsr=0x9fc1;vm.ip=base;vm.register(3,address);vm.register(0,127n);vm.flags=0x602n;const work=vm.work;
  assert.equal((await vm.run(2n)).reason,Exit.Budget,`${mode}/${short}/${spec.kind}/${top}/${tags}/${offset}`);equal(vm.work-work,2n);equal(vm.flags,0xe92n);equal(vm.mxcsr,0x9fc1);equal(simd(test),vector);
  const expected=spec.restore?restored(input,before,short,spec.full):spec.full?initial(before):{...before,cw:before.cw|63,sw:status(before.cw|63,before.sw)};equal(fp(test),expected);
  if(!spec.restore)equal(read(test,address,length),image(before,short,spec.full));else equal(read(test,address,length),input);
  equal(read(test,address-1n,1),Uint8Array.of(0xa5));equal(read(test,address+BigInt(length),1),Uint8Array.of(0xa5));
 }
 equal(test.calls.resolve,0);equal(test.calls.load,0);equal(test.calls.store,0);equal(test.calls.numeric,0);equal(test.calls.fp_area,80);equal(test.calls.fp_commit,spec.restore?0:80);vm.dispose();
}

for(const short of [false,true])for(const spec of specs){
 const length=size(spec,short),address=data+4096n-BigInt(length),test=program(code(spec,short)),{vm}=test,before=seed(),input=image(seed(),short,spec.full);vm.map(data,8192n);vm.write(address,input);fp(test,before);vm.register(3,address);assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);
 assert.equal((await vm.run(1n)).reason,Exit.Budget,'image ends at the exact format width');equal(fp(test),spec.restore?restored(input,before,short,spec.full):spec.full?initial(before):{...before,cw:before.cw|63});core.wire_load(test.handle,data+4096n,1,1);equal(core.wire_fault_kind(test.handle),4,'unused tail preserves guard');vm.dispose();
}
for(const short of [false,true])for(const spec of specs)for(const cross of [false,true]){
 const length=size(spec,short),address=data+(cross?4091n:1n),test=program(code(spec,short)),{vm}=test,before=seed();vm.map(data,8192n);vm.write(address,new Uint8Array(length).fill(0xa5));fp(test,before);simd(test,vector);vm.mxcsr=0x9fc1;vm.register(3,address);const page=data+(cross?4096n:0n);assert.equal(core.wire_protect(test.handle,page,4096n,cross?(spec.restore?1:2):0x104),0);
 assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);equal(simd(test),vector);equal(vm.mxcsr,0x9fc1);equal(vm.work,0n);equal(vm.retired,0n);equal(vm.ip,base);assert.equal(core.wire_protect(test.handle,page,4096n,4),0);equal(read(test,address,length),new Uint8Array(length).fill(0xa5));vm.dispose();
}

// A failed commit must not mask exceptions or perform the post-save reset.
for(const spec of specs.filter(s=>!s.restore)){
 const test=program(code(spec)),{vm}=test,before=seed();before.cw=0x37e;before.sw|=0x8080;fp(test,before);vm.map(data,4096n);vm.write(data,new Uint8Array(size(spec,false)).fill(0xa5));vm.register(3,data);test.watch.onArea=pointer=>{assert.ok(pointer);assert.equal(core.wire_protect(test.handle,data,4096n,2),0);};
 assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);equal(vm.work,0n);equal(vm.retired,0n);equal(test.calls.fp_commit,1);assert.equal(core.wire_protect(test.handle,data,4096n,4),0);equal(read(test,data,size(spec,false)),new Uint8Array(size(spec,false)).fill(0xa5));vm.dispose();
}
for(const short of [false,true])for(const spec of specs.filter(s=>!s.restore)){
 const test=program(code(spec,short)),{vm}=test,before=seed();fp(test,before);vm.map(data,4096n,2);vm.register(3,data+1n);assert.equal((await vm.run(1n)).reason,Exit.Budget,'save requires only write permission');assert.equal(core.wire_protect(test.handle,data,4096n,4),0);equal(read(test,data+1n,size(spec,short)),image(before,short,spec.full));vm.dispose();
}

for(const mode of [32,64])for(const spec of specs){
 const test=program(code(spec),mode),{vm}=test,before=seed();before.cw=0x37e;before.sw|=0x8080;fp(test,before);vm.map(data,4096n);vm.register(3,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);
 assert.equal((await vm.run(1n)).reason,spec.restore?Exit.Exception:Exit.Fault);equal(fp(test),before);equal(vm.work,0n);equal(test.calls.fp_area,spec.restore?0:1);if(spec.restore){equal(core.wire_exception(test.handle),17);core.wire_load(test.handle,data,1,1);equal(core.wire_fault_kind(test.handle),4,'pending #MF precedes guard');}vm.dispose();
}
for(const mode of [32,64])for(const short of [false,true])for(const spec of specs.filter(s=>!s.restore)){
 const test=program([...code(spec,short),0x9b],mode),{vm}=test,before=seed();before.cw=0x37e;before.sw|=0x8080;fp(test,before);vm.map(data,4096n);vm.register(3,data);assert.equal((await vm.run(1n)).reason,Exit.Budget,'non-waiting save handles pending exception');equal(read(test,data,size(spec,short)),image(before,short,spec.full));const after=spec.full?initial(before):{...before,cw:0x37f,sw:before.sw&~0x8080};equal(fp(test),after);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),after);vm.dispose();
 const waiting=program([0x9b,...code(spec,short)],mode);fp(waiting,before);waiting.vm.register(3,data);assert.equal((await waiting.vm.run(2n)).reason,Exit.Exception);equal(fp(waiting),before);equal(waiting.vm.work,0n);equal(waiting.calls.fp_area,0);waiting.vm.dispose();
}
for(const short of [false,true])for(const spec of specs.filter(s=>s.restore)){
 const test=program([...code(spec,short),0x04,1,0x9b]),{vm}=test,before=seed(),source=seed();source.cw=0x37e;source.sw=(source.sw&~0x8080)|1;const input=image(source,short,spec.full);vm.map(data,4096n);vm.write(data,input);fp(test,before);vm.register(3,data);vm.register(0,127n);vm.flags=0x602n;const after=restored(input,before,short,spec.full);
 assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),after);equal(vm.register(0),128n);equal(vm.flags,0xe92n);assert.equal((await vm.run(1n)).reason,Exit.Exception);equal(core.wire_exception(test.handle),17);equal(vm.ip,base+BigInt(code(spec,short).length+2));equal(vm.work,2n);equal(fp(test),after);vm.dispose();
}

// Operand-size prefixes select the legacy image; REX.W does not add a 64-bit
// pointer layout. Address-size and FS still select the effective address.
for(const [short,prefix] of [[false,[0x48]],[true,[0x48,0x66]],[false,[0x66,0x48]]])for(const spec of specs){
 const test=program([...prefix,...code(spec)]),{vm}=test,before=seed();fp(test,before);vm.map(data,4096n);vm.register(3,data);const input=image(seed(),short,spec.full);vm.write(data,input);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),spec.restore?restored(input,before,short,spec.full):spec.full?initial(before):before);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0x64,0x67,...code(specs[1])],mode),{vm}=test,before=seed();fp(test,before);vm.map(data,4096n);new DataView(core.memory.buffer).setBigUint64(core.wire_state(test.handle)+stateLayout[6],data,true);vm.register(mode===64?3:5,0x1234000000000001n);if(mode===32)vm.register(7,2n);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(read(test,data+(mode===64?1n:3n),28),image(before,false,false));vm.dispose();
}
for(const spec of specs){
 const test=program([0xf0,...code(spec)]),{vm}=test;await assert.rejects(vm.run(1n),/invalid instruction/);equal(test.calls.fp_area,0);vm.dispose();
 const empty=program(code(spec)),before=seed();fp(empty,before);equal((await empty.vm.run(0n)).reason,Exit.Budget);equal(fp(empty),before);equal(empty.calls.fp_area,0);empty.vm.dispose();
}

for(const spec of specs.filter(s=>!s.restore)){
 const test=program([...code(spec),0x04,1]),{vm}=test,before=seed();before.cw=0x37e;before.sw|=0x8080;fp(test,before);vm.register(0,123n);vm.register(3,base+16n);assert.equal(core.wire_compile(test.handle),0);const ticket=core.wire_ticket(test.handle);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.work,1n);equal(vm.retired,1n);equal(vm.register(0),123n);equal(core.wire_ready(test.handle,ticket),0);equal(fp(test),spec.full?initial(before):{...before,cw:0x37f,sw:before.sw&~0x8080});vm.dispose();
}

for(const mode of [32,64])for(const short of [false,true]){
 const load=code(specs[2],short),store=code(specs[1],short),save=code(specs[3],short),test=program([...load,0xff,0xe2],mode),{vm}=test,target=0x30000n;
 vm.map(data,4096n,7);vm.write(data,[...store.slice(0,-1),0x37,...save.slice(0,-1),0x36,0x83,0xe9,1,0x74,2,0xff,0xe5,0xcc]);vm.map(target,4096n);vm.register(2,data);vm.register(5,base);vm.register(3,target);vm.register(7,target+256n);vm.register(6,target+512n);const source=seed();source.cw=0x37e;source.sw|=0x8080;source.tags=0;const input=image(source,short,true);vm.write(target,input);vm.register(1,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){
  const before=seed();fp(test,before);simd(test,vector);vm.mxcsr=0x9fc1;vm.ip=base;vm.flags=0x602n;vm.register(1,BigInt(count));test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>{equal(fp(test),before,'ordinary links preserve entry FP backing');equal(simd(test),vector);};
  assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const loaded=restored(input,before,short,true),masked={...loaded,cw:loaded.cw|63,sw:loaded.sw&~0x8080};equal(read(test,target+256n,short?14:28),image(loaded,short,false));equal(read(test,target+512n,size(specs[3],short)),image(masked,short,true));equal(fp(test),initial(loaded));equal(vm.mxcsr,0x9fc1);equal(simd(test),vector);equal(vm.flags,0x646n);equal(test.calls.begin,1);equal(test.calls.link,2);equal(vm.stats(),stats);
 }vm.dispose();
}

console.log(`${checks} legacy FP-environment checks passed`);
