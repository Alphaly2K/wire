import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
function machine(mode,options={}){let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode,...options});return {vm,get handle(){return handle;}};}
const state=test=>new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();
const ticks=0x12345678abcdef01n;

for(const mode of [32,64])for(const tls of [false,true])for(const relocation of ['preferred','explicit','collision']){
 const input=fixture(mode,{tls}),test=machine(mode,{clock:()=>ticks}),{vm}=test;
 if(relocation==='collision'){vm.map(input.preferred,0x5000n);vm.write(input.preferred,[73]);}
 const info=vm.load(input.bytes,{base:relocation==='explicit'?input.preferred+0x200000n:0n,command:'"C:\\試験.exe" --check'});equal(vm.image,info);equal(info.size,0x5000n);equal(info.entry,info.base+0x1000n);equal(info.tls!==0n,tls);equal(vm.readInt(info.base,2),0x5a4dn);equal(vm.readInt(info.base+0x3ff8n,8),0n,'virtual tails start zero');equal(vm.readInt(info.base+0x20a0n,mode/8),vm.bind('kernel32.dll','QueryPerformanceCounter'));
 if(relocation==='preferred')equal(info.base,input.preferred);else assert.notEqual(info.base,input.preferred);
 if(relocation==='collision')equal(vm.readInt(input.preferred,1),73n);
 const before=state(test);equal((await vm.run(0n)).reason,Exit.Budget);equal(state(test),before);equal(vm.stats().modules,0);
 if(tls){equal(vm.readInt(info.tls,4),0x11223344n);equal(vm.readInt(info.tls+4n,8),0n);equal(vm.readInt(info.tlsArray,mode/8),info.tls);equal(vm.readInt(info.base+0x300cn),0n);if(mode===32)equal([4n,8n,12n].map(offset=>vm.readInt(info.sp+offset)),[info.base,1n,0n]);else equal([1,2,8].map(n=>vm.register(n)),[info.base,1n,0n]);}
 assert.throws(()=>vm.write(info.base+0x1000n,[0x90]),/guest memory/);const end=await vm.run(1000n);equal(end.reason,Exit.Returned);equal(end.ip,vm.entryPoints.returned);equal(end.work,vm.work);equal(vm.work,vm.retired+1n);equal(vm.register(0),mode===32?ticks&0xffffffffn:ticks);equal(vm.readInt(info.base+0x3000n,8),ticks);equal(vm.readInt(info.base+0x3008n),tls?2n:0n);if(tls)equal(vm.readInt(info.base+0x3010n),0x11223344n);
 const saved=state(test);equal((await vm.run(10n)).reason,Exit.Returned);equal(state(test),saved);assert.throws(()=>vm.load(input.bytes),/idle VM/);vm.dispose();
}

for(const mode of [32,64]){
 const input=fixture(mode),test=machine(mode,{clock:()=>ticks}),{vm}=test,info=vm.load(input.bytes);let total=0n;
 for(let n=0;n<200;n++){const result=await vm.run(1n);total+=result.work;if(result.reason===Exit.Returned)break;equal(result.reason,Exit.Budget);assert.ok(n<199);}
 equal(vm.work,total);equal(vm.retired,total-1n);equal(vm.readInt(info.base+0x3008n),2n);equal(vm.readInt(info.base+0x3000n,8),ticks);vm.dispose();
}

for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),test=machine(mode),{vm}=test;
 for(let n=0;n<16;n++){f.put(f.dirs+n*8,0);f.put(f.dirs+n*8+4,0);}
 f.bytes.set([0xb8,73,0,0,0,0xc3],0x400);f.put(f.sections+3*40+8,0);f.put(f.sections+3*40+16,0);f.put(f.sections+3*40+12,0xffffffff);
 vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),73n,'empty sections require no backing even with unused large RVA');vm.dispose();
 const low=fixture(mode,{tls:false});for(let n=0;n<16;n++){low.put(low.dirs+n*8,0);low.put(low.dirs+n*8+4,0);}low.put(low.opt+32,0x200);low.put(low.opt+56,0x1400);low.put(low.opt+16,0x400);low.bytes.set([0xb8,73,0,0,0,0xc3],0x400);for(const [n,rva,size] of [[0,0x400,0x400],[1,0x800,0x600],[2,0xe00,0x400],[3,0x1200,0x200]]){low.put(low.sections+n*40+12,rva);low.put(low.sections+n*40+8,size);}
 const shared=machine(mode);const info=shared.vm.load(low.bytes);equal(info.size,0x2000n);equal(shared.vm.readInt(info.base+0x1100n),0n);shared.vm.write(info.base+0x400n,[0xb8]);equal((await shared.vm.run(100n)).reason,Exit.Returned);equal(shared.vm.register(0),73n,'subpage sections combine page protections');shared.vm.dispose();
}
{
 const f=fixture(32,{tls:false});f.put(f.opt+70,0x40,2);f.put(f.opt+16,0x3000);for(let n=0;n<16;n++){f.put(f.dirs+n*8,0);f.put(f.dirs+n*8+4,0);}f.bytes.set([0xb8,73,0,0,0,0xc3],f.offset(0x3000));const test=machine(32),info=test.vm.load(f.bytes);equal((await test.vm.run(100n)).reason,Exit.Returned);equal(test.vm.register(0),73n,'PE32 without NX_COMPAT preserves the opt-in DEP profile');test.vm.ip=info.entry;test.vm.register(4,info.sp);test.vm.write(info.entry,[0xb8,74,0,0,0,0xc3]);equal((await test.vm.run(100n)).reason,Exit.Returned);equal(test.vm.register(0),74n,'code in readable data still participates in source revocation');test.vm.dispose();
}
for(const mode of [32,64]){
 let broken=true;const failure=new Error('time source failed'),input=fixture(mode),test=machine(mode,{clock(){if(broken)throw failure;return ticks;}}),{vm}=test,info=vm.load(input.bytes);
 await assert.rejects(vm.run(1000n),e=>e===failure);equal(vm.ip,vm.bind('kernel32.dll','QueryPerformanceCounter'));equal(vm.work,vm.retired);equal(vm.readInt(info.base+0x3008n),2n);const before=vm.work;broken=false;const result=await vm.run(1000n);equal(result.reason,Exit.Returned);equal(vm.work,before+result.work);equal(vm.readInt(info.base+0x3008n),2n,'retry does not repeat TLS callbacks');vm.dispose();
}
for(const mode of [32,64]){
 const input=fixture(mode),test=machine(mode,{clock:()=>ticks}),{vm}=test,info=vm.load(input.bytes);while(vm.ip!==vm.entryPoints.callback)equal((await vm.run(1n)).reason,Exit.Budget);
 const page=info.sp&~4095n;equal(core.wire_protect(test.handle,page,4096n,2),0);const before=state(test);equal((await vm.run(1n)).reason,Exit.Fault);equal(state(test),before,'failed next-stage return-frame write retains the callback queue and CPU');equal(core.wire_protect(test.handle,page,4096n,4),0);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(info.base+0x3008n),2n);vm.dispose();
}
for(const mode of [32,64]){
 const test=machine(mode,{clock:()=>ticks}),{vm}=test,info=vm.load(fixture(mode,{tls:false}).bytes);equal(core.wire_protect(test.handle,info.base+0x3000n,4096n,2),0);const failed=await vm.run(100n);equal(failed.reason,Exit.Fault);equal(failed.ip,vm.bind('kernel32.dll','QueryPerformanceCounter'));equal(failed.work,vm.retired);equal(vm.work,vm.retired);equal(core.wire_protect(test.handle,info.base+0x3000n,4096n,4),0);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.work,vm.retired+1n);vm.dispose();
}

const corruptions=[
 f=>f.put(0x3c,0xffffffff),f=>f.put(f.opt,0x999,2),f=>f.put(f.dirs-4,17),f=>f.put(f.opt+60,0x100),f=>f.put(f.opt+56,0xfffff000),f=>f.put(f.sections+12,0xfffff000),f=>f.put(f.sections+16,0x100000),f=>f.put(f.sections+40+12,0x1000),f=>f.put(f.dirs+8,0xfffffff0),f=>f.put(f.dirs+12,20),f=>f.rput(0x2080,(1n<<BigInt(f.ptr*8-1))|0x10000n,f.ptr),f=>f.rput(0x200c,0),f=>f.rput(0x4004,7),f=>f.rput(0x4008,0xf001,2),f=>f.rput(0x2200+f.ptr,1,f.ptr),f=>f.rput(0x2200+f.ptr*2,f.preferred+0x2000n,f.ptr),f=>f.rput(0x2200+f.ptr*3,f.preferred+0x4fffn,f.ptr),f=>f.rput(0x2200+f.ptr*4+4,15<<20),f=>f.rput(0x2300,f.preferred+0x3000n,f.ptr),f=>f.put(f.opt+72,0xffffffffffn,f.ptr),f=>f.put(f.dirs+13*8,0x2500),f=>f.put(f.dirs+13*8+4,32),f=>f.put(0x96,0x2102,2),f=>f.put(f.opt+16,0x3000),
];
for(const mode of [32,64])for(const mutate of corruptions){
 const f=fixture(mode);mutate(f);const test=machine(mode),{vm}=test;vm.register(0,123n);const before=state(test);assert.throws(()=>vm.load(f.bytes,{base:f.preferred+0x200000n}),Error);equal(state(test),before);equal(vm.image,null);vm.map(f.preferred+0x200000n,0x5000n);vm.dispose();
}
for(const mode of [32,64]){
 const f=fixture(mode),test=machine(mode),{vm}=test,first=vm.bind('kernel32.dll','GetTickCount');f.rput(0x2080+f.ptr,0x2500,f.ptr);f.rtext(0x2502,'WireMissingExport\0');assert.throws(()=>vm.load(f.bytes),/unknown Windows export/);equal(vm.bind('kernel32.dll','GetTickCount64'),first+16n,'failed imports do not publish staged bindings');vm.dispose();
 const fallback=machine(mode,{clock:()=>ticks});fallback.vm.load(fixture(mode,{lookup:false,tls:false}).bytes);equal((await fallback.vm.run(100n)).reason,Exit.Returned);fallback.vm.dispose();
 const wrong=machine(mode===32?64:32);assert.throws(()=>wrong.vm.load(f.bytes),/does not match/);wrong.vm.dispose();
}
{
 const f=fixture(64),test=machine(64);f.rput(0x2200,0,8);f.rput(0x2208,(1n<<64n)-1n,8);assert.throws(()=>test.vm.load(f.bytes),/TLS template/);equal(test.vm.image,null);test.vm.dispose();
}

console.log(`${checks} PE loading/relocation/import/TLS execution checks passed`);
