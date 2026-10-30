import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode,fp} from './x87-harness.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';
import {delayFixture} from './delay-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const text=value=>new TextEncoder().encode(value+'\0');
const snapshot=t=>{const view=new DataView(core.memory.buffer),at=core.wire_state(t.handle);return {registers:Array.from({length:16},(_,n)=>view.getBigUint64(at+stateLayout[1]+n*8,true)),xmm:new Uint8Array(core.memory.buffer,at+stateLayout[9],256).slice(),fields:stateLayout.slice(2,8).map(offset=>view.getBigUint64(at+offset,true)),exception:view.getUint32(at+stateLayout[8],true),mxcsr:view.getUint32(at+stateLayout[10],true),fpu:fp(t)};};
const plain=mode=>{const f=delayFixture(mode);f.put(f.dirs+13*8,0);f.put(f.dirs+13*8+4,0);return f;};
async function machine(mode,files={},main=delayFixture(mode),options={}){
 let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode,clock:()=>{throw new Error('unexpected clock sample');}});
 for(const [name,file] of Object.entries(files))vm.supplyModule(name,file.bytes);
 const image=vm.load(main.bytes,options);equal((await vm.run(1000n)).reason,Exit.Returned);vm.map(0x20000n,0x4000n,7);return {vm,handle,mode,image,sp:image.sp-256n};
}
function call(t,pc,args=[]){const {vm,mode,sp}=t;vm.ip=pc;vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else vm.register([1,2,8,9][n],args[n]);}
async function finish(t,budget=1000n){let work=0n;for(let n=0;n<10000;n++){const result=await t.vm.run(budget);work+=result.work;if(result.reason===Exit.Returned)return work;equal(result.reason,Exit.Budget);}throw new Error('lazy operation did not return');}
async function request(t,name,args=[],budget=1000n){call(t,t.vm.bind('kernel32.dll',name),args);await finish(t,budget);return t.vm.register(0);}
const count=(t,id,offset=0)=>t.vm.readInt(trace+BigInt(id*16+offset));

// Late forward dependencies must initialize once and detach after their owner.
for(const mode of [32,64])for(const budget of [1n,1000n])for(const ordinal of [false,true]){
 const t=await machine(mode,{'a.dll':dynamicFixture(mode,{id:1,forward:'b.GetValue'}),'b.dll':dynamicFixture(mode,{id:2,forward:ordinal?'leaf.#17':'leaf.GetValue',tls:true}),'leaf.dll':dynamicFixture(mode,{id:3,seed:73,tls:true})},plain(mode)),{vm}=t;
 vm.write(0x20000n,text('a.dll'));const a=await request(t,'LoadLibraryA',[0x20000n],budget);equal(vm.moduleBase('b.dll'),0n);vm.write(0x20040n,text('GetValue'));const target=await request(t,'GetProcAddress',[a,0x20040n],budget),b=vm.moduleBase('b.dll'),leaf=vm.moduleBase('leaf.dll');equal(target,leaf+0x1080n);equal([count(t,1),count(t,2),count(t,3)],[1n,1n,1n]);
 call(t,target);await finish(t,budget);equal(vm.register(0),73n);equal(await request(t,'GetProcAddress',[a,17n],budget),target);equal(await request(t,'FreeLibrary',[a],budget),1n);equal([vm.moduleBase('a.dll'),vm.moduleBase('b.dll'),vm.moduleBase('leaf.dll')],[0n,0n,0n]);equal(vm.readInt(trace+0x400n),0x123n,'late forward dependencies detach after their importer');equal([count(t,1,4),count(t,2,4),count(t,3,4)],[1n,1n,1n]);assert.ok(b);vm.dispose();
}
for(const mode of [32,64])for(const failure of ['missing','export','cycle','attach']){
 const files={'a.dll':dynamicFixture(mode,{id:1,forward:'b.GetValue'})};if(failure!=='missing')files['b.dll']=dynamicFixture(mode,{id:2,forward:failure==='cycle'?'a.GetValue':failure==='export'?'leaf.Absent':null,attach:failure!=='attach'});if(failure==='export')files['leaf.dll']=dynamicFixture(mode,{id:3});
 const t=await machine(mode,files,plain(mode)),{vm}=t;vm.write(0x20000n,text('a.dll'));const a=await request(t,'LoadLibraryA',[0x20000n]);vm.write(0x20040n,text('GetValue'));equal(await request(t,'GetProcAddress',[a,0x20040n],1n),0n);equal(vm.readInt(t.image.teb+BigInt(mode===32?0x34:0x68)),BigInt(failure==='missing'?126:failure==='attach'?1114:127));equal(await request(t,'FreeLibrary',[a],1n),1n);equal(vm.moduleBase('a.dll'),0n);equal(vm.moduleBase('b.dll'),0n);equal(vm.moduleBase('leaf.dll'),0n);vm.dispose();
}

for(const mode of [32,64])for(const budget of [1n,1000n])for(const va of [false,true]){
 const main=delayFixture(mode,{va}),t=await machine(mode,{'leaf.dll':dynamicFixture(mode,{id:1,seed:73,tls:true})},main,va?{base:0x800000n}:{}),{vm,image}=t,width=mode/8,iat=image.base+0x3120n,hmod=image.base+0x3100n,thunk=vm.readInt(iat,width);
 equal(vm.moduleBase('leaf.dll'),0n,'unused delay imports do not load a supplied optional DLL');equal(vm.readInt(hmod,width),0n);equal(vm.readInt(image.base+0x3180n,width),thunk);equal(core.wire_service_clock(t.handle),0);
 call(t,thunk);const before=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);equal(vm.moduleBase('leaf.dll'),0n);equal(core.wire_service_clock(t.handle),1);
 const retired=vm.retired,work=vm.work;await finish(t,budget);const leaf=vm.moduleBase('leaf.dll');assert.ok(leaf);equal(vm.register(0),73n);equal(vm.work-work,vm.retired-retired,'lazy resolution adds no virtual instruction or service work');equal(vm.readInt(iat,width),leaf+0x1080n);equal(vm.readInt(hmod,width),leaf);equal(count(t,1),1n);equal(count(t,1,8),2n);
 for(const pc of [thunk,vm.readInt(iat+BigInt(width),width),vm.readInt(iat,width)]){call(t,pc);await finish(t,budget);equal(vm.register(0),73n);equal(count(t,1),1n);}
 equal(await request(t,'FreeLibrary',[leaf],budget),1n);equal(vm.moduleBase('leaf.dll'),0n,'all functions and retained thunks share one descriptor load reference');equal(vm.readInt(hmod,width),0n);equal(vm.readInt(iat,width),thunk);equal(vm.readInt(iat+BigInt(width),width),thunk+16n);equal(count(t,1,4),1n);
 call(t,thunk);await finish(t,budget);equal(vm.register(0),73n);equal(count(t,1),2n);equal(await request(t,'FreeLibrary',[vm.moduleBase('leaf.dll')],budget),1n);vm.dispose();
}

// Missing optional DLLs fail at the call site and can be supplied while paused.
for(const mode of [32,64]){
 const t=await machine(mode),{vm,image}=t,thunk=vm.readInt(image.base+0x3120n,mode/8);call(t,thunk);const before=snapshot(t),work=vm.work;
 await assert.rejects(vm.run(100n),/delay import resolution.*126/);equal(snapshot(t),before);equal(vm.work,work);vm.supplyModule('leaf.dll',dynamicFixture(mode,{id:1,seed:91}).bytes);await finish(t,1n);equal(vm.register(0),91n);equal(count(t,1),1n);assert.throws(()=>vm.supplyModule('LEAF.DLL',[]),/duplicate/);vm.dispose();
}

// Preserve the unknown target ABI, including the original return frame and FP state.
for(const mode of [32,64]){
 const leaf=dynamicFixture(mode,{id:1,tls:true});leaf.bytes.set(mode===32?[0x8b,0x44,0x24,4,0x03,0x44,0x24,8,0xc2,8,0]:[0x89,0xc8,0x01,0xd0,0xc3],leaf.offset(0x1080));leaf.relocations.splice(0,leaf.relocations.length,...leaf.relocations.filter(rva=>rva<0x1080||rva>=0x1100));leaf.rebuildRelocations();
 const t=await machine(mode,{'leaf.dll':leaf}),{vm,image}=t,thunk=vm.readInt(image.base+0x3120n,mode/8);call(t,thunk,[11n,22n]);vm.flags=0xed7n;vm.xmm(0,new Uint8Array(16).fill(0xa7));const fpu=fp(t);fpu.regs[0]=0x3fff8000000000000000n;fpu.tags=0xfffcn;fp(t,{...fpu,tags:Number(fpu.tags)});const before=snapshot(t),work=vm.work;
 equal(vm.service().work,0n);equal(vm.work,work);
 let target=0n;for(let n=0;n<1000;n++){target=vm.moduleBase('leaf.dll')+0x1080n;if(vm.ip===target)break;if(vm.ip===vm.entryPoints.callback)equal(core.wire_startup(t.handle),0);else equal((await vm.run(1n)).reason,Exit.Budget);assert.ok(n<999);}
 before.fields[1]=target;before.fields[2]=vm.work;before.fields[3]=vm.retired;equal(snapshot(t),before,'resolution restores every architectural caller field before tail transfer');await finish(t,1n);equal(vm.register(0),33n);equal(vm.register(4),t.sp+BigInt(mode===32?12:8));vm.dispose();
}

// Preflight and post-initialization faults must neither publish partial outputs nor reattach.
for(const mode of [32,64]){
 const t=await machine(mode,{'leaf.dll':dynamicFixture(mode,{id:1,tls:true})}),{vm,image}=t,iat=image.base+0x3120n,hmod=image.base+0x3100n,page=hmod&~4095n,thunk=vm.readInt(iat,mode/8);
 call(t,thunk);equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.moduleBase('leaf.dll'),0n);equal(vm.readInt(iat,mode/8),thunk);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().work,0n);
 for(let n=0;n<1000;n++){if(vm.ip===vm.entryPoints.callback&&count(t,1)===1n)break;equal((await vm.run(1n)).reason,Exit.Budget);assert.ok(n<999);}
 equal(core.wire_protect(t.handle,page,4096n,2),0);equal((await vm.run(100n)).reason,Exit.Fault);equal(vm.readInt(hmod,mode/8),0n);equal(vm.readInt(iat,mode/8),thunk);equal(count(t,1),1n);const paused=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),paused);equal(core.wire_protect(t.handle,page,4096n,4),0);await finish(t,1n);equal(count(t,1),1n);
 const base=vm.moduleBase('leaf.dll');call(t,vm.bind('kernel32.dll','FreeLibrary'),[base]);equal(core.wire_protect(t.handle,page,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal(count(t,1,4),1n);equal(vm.moduleBase('leaf.dll'),base);equal(vm.readInt(hmod,mode/8),base);equal(core.wire_protect(t.handle,page,4096n,4),0);await finish(t,1n);equal(vm.moduleBase('leaf.dll'),0n);equal(count(t,1,4),1n);equal(vm.readInt(iat,mode/8),thunk);
 for(const pc of [thunk+1n,thunk+0xfff0n]){vm.ip=pc;vm.register(4,0xdeadbeefn);const saved=snapshot(t);equal(core.wire_service_clock(t.handle),0);assert.throws(()=>vm.service(),/unbound/);equal(snapshot(t),saved);equal(core.wire_service(t.handle,0n),2);equal(snapshot(t),saved);}
 vm.dispose();
}

// A recursive call during DllMain shares the already-acquired descriptor reference.
for(const mode of [32,64])for(const attach of [true,false]){
 const main=delayFixture(mode),gateway=mode===32?0x70500010n:0x7ffe21000010n,leaf=dynamicFixture(mode,{id:1,attach});
 const code=mode===32?[0x83,0x7c,0x24,8,1,0x75,7,0xb8,...encode(gateway,4),0xff,0xd0,0xb8,attach?1:0,0,0,0,0xc2,12,0]:[0x83,0xfa,1,0x75,20,0x48,0x83,0xec,40,0x48,0xb8,...encode(gateway,8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,attach?1:0,0,0,0,0xc3];
 leaf.bytes.set(code,leaf.offset(0x1000));leaf.bytes.set([0xb8,77,0,0,0,0xc3],leaf.offset(0x1080));leaf.relocations.splice(0,leaf.relocations.length,...leaf.relocations.filter(rva=>rva<0x1000||rva>=0x1100));leaf.rebuildRelocations();
 const t=await machine(mode,{'leaf.dll':leaf},main),{vm,image}=t,thunk=vm.readInt(image.base+0x3120n,mode/8),baseline=vm.stats();call(t,thunk);
 if(attach){await finish(t,1n);equal(vm.register(0),77n);equal(count(t,1),1n);equal(await request(t,'FreeLibrary',[vm.moduleBase('leaf.dll')],1n),1n);equal(vm.moduleBase('leaf.dll'),0n);}
 else for(let n=1;n<=3;n++){await assert.rejects(vm.run(1000n),/delay import resolution.*1114/);equal(vm.ip,thunk);equal(vm.moduleBase('leaf.dll'),0n);equal(count(t,1),BigInt(n));equal(count(t,1,4),BigInt(n));equal(vm.readInt(image.base+0x3100n,mode/8),0n);equal(vm.readInt(image.base+0x3120n+BigInt(mode/8),mode/8),gateway);equal(vm.stats(),baseline,'failed reentrant delay loads release all generated instances');}
 vm.dispose();
}

// Descriptor references follow LoadLibrary ownership; owner retirement only removes its gateways.
for(const mode of [32,64]){
 const owner=delayFixture(mode,{dll:true}),t=await machine(mode,{'owner.dll':owner,'leaf.dll':dynamicFixture(mode,{id:1,seed:59,tls:true})},plain(mode)),{vm}=t;vm.write(0x20000n,text('owner.dll'));const baseline=vm.stats();let previous=0n;
 for(let n=0;n<3;n++){const base=await request(t,'LoadLibraryA',[0x20000n],1n),thunk=vm.readInt(base+0x3120n,mode/8);if(previous)equal(thunk,previous,'retired owners release their bounded delay slots');previous=thunk;call(t,thunk);await finish(t,1n);const leaf=vm.moduleBase('leaf.dll');equal(vm.register(0),59n);equal(await request(t,'FreeLibrary',[base],1n),1n);equal(vm.moduleBase('owner.dll'),0n);equal(vm.moduleBase('leaf.dll'),leaf);vm.ip=thunk;equal(core.wire_service_clock(t.handle),0);equal(await request(t,'FreeLibrary',[leaf],1n),1n);equal(vm.moduleBase('leaf.dll'),0n);equal(vm.stats(),baseline);}
 vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode,{},delayFixture(mode,{library:'kernel32.dll',symbols:['GetCurrentProcessId']})),{vm,image}=t,thunk=vm.readInt(image.base+0x3120n,mode/8);call(t,thunk);const work=vm.work,retired=vm.retired;equal(await finish(t,1n),1n);equal(vm.register(0),1n);equal(vm.work-work,1n);equal(vm.retired,retired,'a provider delay call charges only the actual API');vm.write(0x20000n,text('kernel32'));equal(await request(t,'GetModuleHandleA',[0x20000n]),vm.readInt(image.base+0x3100n,mode/8));vm.dispose();
 for(const symbol of ['WireMissingExport',17]){const t=await machine(mode,{},delayFixture(mode,{library:'kernel32.dll',symbols:[symbol]}));call(t,t.vm.readInt(t.image.base+0x3120n,mode/8));await assert.rejects(t.vm.run(100n),/delay import resolution.*127/);equal(t.vm.retired,2n);t.vm.dispose();}
 const forward=await machine(mode,{'leaf.dll':dynamicFixture(mode,{id:1,forward:'target.#17'}),'target.dll':dynamicFixture(mode,{id:2,seed:88})});call(forward,forward.vm.readInt(forward.image.base+0x3120n,mode/8));await finish(forward,1n);equal(forward.vm.register(0),88n);equal(await request(forward,'FreeLibrary',[forward.vm.moduleBase('leaf.dll')],1n),1n);equal(forward.vm.moduleBase('target.dll'),0n);forward.vm.dispose();
 const missing=await machine(mode,{'leaf.dll':dynamicFixture(mode,{id:1})},delayFixture(mode,{symbols:['Absent']}));call(missing,missing.vm.readInt(missing.image.base+0x3120n,mode/8));for(let n=0;n<2;n++){await assert.rejects(missing.vm.run(1000n),/delay import resolution.*127/);equal(count(missing,1),1n);}equal(await request(missing,'FreeLibrary',[missing.vm.moduleBase('leaf.dll')],1n),1n);equal(missing.vm.moduleBase('leaf.dll'),0n);missing.vm.dispose();
}

// Reject malformed metadata before publishing any image or service binding.
for(const mode of [32,64])for(const mutate of [
 f=>f.rput(0x2100,2),f=>f.rput(0x2104,0),f=>f.rput(0x2108,0),f=>f.rput(0x2110,0),
 f=>f.rput(0x2114,0x4fff),f=>f.rput(0x2118,0x4fff),f=>f.put(f.dirs+13*8+4,31),
 f=>{f.put(f.dirs+13*8+4,32);f.rput(0x2120,1);},
 f=>f.rput(0x2200,(1n<<BigInt(mode-1))|0x10000n,mode/8),f=>f.rput(0x2200,0x5000,mode/8),
 f=>f.rput(0x210c,0x4fff),f=>f.rput(0x2108,0x21a0),f=>f.rput(0x2108,0x3121),f=>f.rput(0x2118,0x3120),
 f=>{f.bytes.copyWithin(f.offset(0x2120),f.offset(0x2100),f.offset(0x2120));f.put(f.dirs+13*8+4,96);},
 f=>{f.requested=mode===32?0x70500000n:0x7ffe21000000n;},
]){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode}),f=delayFixture(mode),t={vm,handle,mode};mutate(f);const before=snapshot(t);assert.throws(()=>vm.load(f.bytes,{base:f.requested}),Error,mutate.toString());equal(vm.image,null);equal(snapshot(t),before);equal(vm.moduleBase('program.exe'),0n);vm.dispose();
}

function capacityFixture(mode,count){
 const f=delayFixture(mode,{symbols:[17],unload:false}),bytes=new Uint8Array(0x21000);bytes.set(f.bytes);const view=new DataView(bytes.buffer),put=(at,value,width=4)=>bytes.set(encode(BigInt(value),width),at),rput=(rva,value,width=4)=>put(rva<0x3000?f.offset(rva):0xe00+rva-0x3000,value,width);
 put(f.opt+56,0x24000);put(f.sections+2*40+8,0x20000);put(f.sections+2*40+16,0x20000);put(f.sections+3*40+12,0x23000);put(f.sections+3*40+16,0);put(f.sections+3*40+20,0);put(f.dirs+5*8,0);put(f.dirs+5*8+4,0);rput(0x2110,0x4000);rput(0x210c,0xd000);rput(0x2118,0);for(let n=0;n<count;n++)rput(0x4000+n*mode/8,(1n<<BigInt(mode-1))|17n,mode/8);return {bytes,view};
}
for(const mode of [32,64]){
 const t=await machine(mode,{'owner.dll':delayFixture(mode,{dll:true,symbols:[17]})},capacityFixture(mode,4095)),{vm}=t;vm.write(0x20000n,text('owner.dll'));let slot;
 for(let n=0;n<2;n++){const base=await request(t,'LoadLibraryA',[0x20000n]);assert.ok(base);const pc=vm.readInt(base+0x3120n,mode/8);if(slot)equal(pc,slot);slot=pc;equal(await request(t,'FreeLibrary',[base]),1n);}vm.dispose();
 const full=await machine(mode,{'owner.dll':delayFixture(mode,{dll:true,symbols:[17]})},capacityFixture(mode,4096));full.vm.write(0x20000n,text('owner.dll'));equal(await request(full,'LoadLibraryA',[0x20000n]),0n);equal(full.vm.moduleBase('owner.dll'),0n);equal(full.vm.readInt(full.image.teb+BigInt(mode===32?0x34:0x68)),193n);full.vm.dispose();
 const invalid=new WasmMachine(core,{mode});assert.throws(()=>invalid.load(capacityFixture(mode,4097).bytes),/delay import capacity/);equal(invalid.image,null);invalid.dispose();
}

console.log(`${checks} lazy DLL/forwarder checks passed`);
