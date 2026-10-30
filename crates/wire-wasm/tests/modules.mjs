import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout} from './x87-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
function machine(mode){let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode,clock:()=>123450000n});return {vm,get handle(){return handle;}};}
const snapshot=test=>new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();

for(const mode of [32,64]){
 const {vm}=machine(mode),main=moduleFixture(mode,{dependency:'leaf.dll',tls:true}),leaf=moduleFixture(mode,{dll:true,tls:true});vm.supplyModule('leaf.dll',leaf.bytes);vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);
 const base=vm.moduleBase('leaf.dll');equal([0x3040n,0x3048n,0x3050n].map(rva=>vm.readInt(base+rva,mode/8)),[base,1n,1n],'static DllMain receives module, process attach and nonzero reserved');
 for(const image of [base,vm.image.base])equal([0x3060n,0x3068n,0x3070n].map(rva=>vm.readInt(image+rva,mode/8)),[image,1n,0n],'TLS callbacks retain their own notification arguments');vm.dispose();
}

for(const mode of [32,64])for(const tls of [false,true])for(const budget of [1n,1000n]){
 const test=machine(mode),{vm}=test,main=moduleFixture(mode,{dependency:'bridge.dll',tls,seed:1}),bridge=moduleFixture(mode,{dll:true,dependency:'leaf.dll',delta:3,tls,seed:2}),leaf=moduleFixture(mode,{dll:true,seed:7,tls});
 vm.supplyModule('LEAF.DLL',leaf.bytes);vm.supplyModule('bridge',bridge.bytes);const info=vm.load(main.bytes);const b=vm.moduleBase('BRIDGE.DLL'),l=vm.moduleBase('leaf');assert.ok(b&&l);assert.notEqual(b,l);assert.notEqual(l,info.base);equal(vm.moduleBase('missing.dll'),0n);equal(vm.moduleBase('program.exe'),info.base);
 equal(vm.readInt(info.base+0x20a0n,mode/8),b+0x1080n);equal(vm.readInt(b+0x20a0n,mode/8),l+0x1080n,'all DLL IATs are bound before initialization');
 const initial=snapshot(test);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(test),initial);let work=0n;
 for(let n=0;n<1000;n++){const result=await vm.run(budget);work+=result.work;if(result.reason===Exit.Returned)break;equal(result.reason,Exit.Budget);assert.ok(n<999);}
 equal(vm.register(0),10n,'dependency DllMain runs before its importer');equal(vm.readInt(l+0x3000n),7n);equal(vm.readInt(b+0x3000n),10n);equal(vm.readInt(l+0x3030n),1n);equal(vm.readInt(b+0x3030n),1n);equal(vm.work,work);equal(vm.retired,work,'loader transitions add no synthetic retirement');
 if(tls){equal([l,b,info.base].map(base=>vm.readInt(base+0x300cn)),[0n,1n,2n]);for(const [base,value] of [[l,7],[b,2],[info.base,1]]){equal(vm.readInt(base+0x3020n),2n);equal(vm.readInt(base+0x3010n),BigInt(0x11110000+value));const slot=vm.readInt(base+0x300cn),block=vm.readInt(info.tlsArray+slot*BigInt(mode/8),mode/8);equal(vm.readInt(block),BigInt(0x11110000+value));}}
 vm.supplyModule('later.dll',leaf.bytes);equal(vm.moduleBase('later.dll'),0n);vm.dispose();
}
for(const mode of [32,64])for(const request of [{ordinal:true},{data:true}]){
 const {vm}=machine(mode),main=moduleFixture(mode,{dependency:'leaf.dll',...request}),leaf=moduleFixture(mode,{dll:true,seed:91});vm.supplyModule('leaf.dll',leaf.bytes);vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),91n,'ordinal functions and exported data retain their distinct address semantics');vm.dispose();
}
for(const mode of [32,64])for(const forward of ['leaf.GetValue','leaf.#17','kernel32.GetTickCount64']){
 const {vm}=machine(mode),main=moduleFixture(mode,{dependency:'forward.dll'}),bridge=moduleFixture(mode,{dll:true,entry:false,forward}),leaf=moduleFixture(mode,{dll:true,seed:81});vm.supplyModule('forward.dll',bridge.bytes);vm.supplyModule('leaf.dll',leaf.bytes);vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),forward.startsWith('kernel32')?12345n:81n);if(!forward.startsWith('kernel32'))assert.ok(vm.moduleBase('leaf.dll'));vm.dispose();
}
for(const mode of [32,64]){
 const {vm}=machine(mode),main=moduleFixture(mode,{dependency:'a.dll'}),a=moduleFixture(mode,{dll:true,dependency:'b.dll',delta:3}),b=moduleFixture(mode,{dll:true,dependency:'a.dll',delta:2});vm.supplyModule('a.dll',a.bytes);vm.supplyModule('b.dll',b.bytes);vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),5n,'ordinary circular imports bind before deterministic attach traversal');vm.dispose();
}
for(const mode of [32,64]){
 const test=machine(mode),{vm}=test,main=moduleFixture(mode,{dependency:'bridge.dll'}),bridge=moduleFixture(mode,{dll:true,dependency:'leaf.dll',delta:3}),leaf=moduleFixture(mode,{dll:true});main.put(main.dirs+12,60);main.rput(0x2014,0x2120);main.rput(0x2020,0x2100);main.rput(0x2024,0x2140);main.rtext(0x2100,'leaf.dll\0');main.rput(0x2120,0x20e0,mode/8);vm.supplyModule('bridge.dll',bridge.bytes);vm.supplyModule('leaf.dll',leaf.bytes);vm.load(main.bytes);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.readInt(vm.moduleBase('leaf.dll')+0x3030n),1n,'shared dependencies attach exactly once');vm.dispose();
 const retry=machine(mode);retry.vm.supplyModule('bridge.dll',bridge.bytes);retry.vm.supplyModule('leaf.dll',leaf.bytes);const info=retry.vm.load(main.bytes);while(retry.vm.ip!==retry.vm.entryPoints.callback)equal((await retry.vm.run(1n)).reason,Exit.Budget);const page=info.sp&~4095n;equal(core.wire_protect(retry.handle,page,4096n,2),0);const before=snapshot(retry);equal((await retry.vm.run(1n)).reason,Exit.Fault);equal(snapshot(retry),before);equal(core.wire_protect(retry.handle,page,4096n,4),0);equal((await retry.vm.run(1000n)).reason,Exit.Returned);equal(retry.vm.readInt(retry.vm.moduleBase('leaf.dll')+0x3030n),1n);retry.vm.dispose();
 const shadow=machine(mode);shadow.vm.supplyModule('kernel32.dll',[0]);shadow.vm.supplyModule('forward.dll',moduleFixture(mode,{dll:true,entry:false,forward:'kernel32.GetTickCount64'}).bytes);shadow.vm.load(moduleFixture(mode,{dependency:'forward.dll'}).bytes);equal((await shadow.vm.run(1000n)).reason,Exit.Returned);equal(shadow.vm.moduleBase('kernel32.dll'),0n,'built-in provider identity cannot be shadowed by supplied bytes');shadow.vm.dispose();
}
for(const mode of [32,64]){
 const test=machine(mode),{vm}=test,main=moduleFixture(mode,{dependency:'bad.dll'}),bad=moduleFixture(mode,{dll:true,attach:false});vm.supplyModule('bad.dll',bad.bytes);vm.load(main.bytes);await assert.rejects(vm.run(1000n),/attach returned FALSE/);equal(vm.ip,vm.entryPoints.callback);equal(vm.register(0),0n);const before=snapshot(test);await assert.rejects(vm.run(1n),/attach returned FALSE/);equal(snapshot(test),before,'rejected DllMain never advances to the application entry');vm.dispose();
}

const corruptions=[
 f=>f.put(f.dirs,0x4ff0),f=>f.rput(0x2514,65537),f=>f.rput(0x251c,0xfffffff0),f=>f.rput(0x2510,65536),f=>f.rput(0x2540,0x5000),f=>f.rput(0x2560,2,2),f=>f.rput(0x2554,0x2570),f=>f.rput(0x2540,0),f=>{f.rput(0x2540,0x25b0);f.rtext(0x25b0,'bad.#not-a-number\0');},
];
for(const mode of [32,64])for(const mutate of corruptions){
 const test=machine(mode),{vm}=test,main=moduleFixture(mode,{dependency:'leaf.dll'}),leaf=moduleFixture(mode,{dll:true});mutate(leaf);vm.supplyModule('leaf.dll',leaf.bytes);vm.register(0,0x1234n);const before=snapshot(test);assert.throws(()=>vm.load(main.bytes),Error);equal(snapshot(test),before);equal(vm.image,null);vm.map(main.preferred,0x5000n);vm.dispose();
}
for(const mode of [32,64]){
 const test=machine(mode),{vm}=test,main=moduleFixture(mode,{dependency:'missing.dll'});const before=snapshot(test);assert.throws(()=>vm.load(main.bytes),/not supplied/);equal(snapshot(test),before);const leaf=moduleFixture(mode,{dll:true});vm.supplyModule('missing.dll',leaf.bytes);equal(vm.load(main.bytes).base,main.preferred);equal((await vm.run(100n)).reason,Exit.Returned);vm.supplyModule('extra.dll',leaf.bytes);equal(vm.moduleBase('extra.dll'),0n);vm.dispose();
 const invalid=machine(mode);invalid.vm.supplyModule('leaf.dll',leaf.bytes);assert.throws(()=>invalid.vm.supplyModule('LEAF.DLL',leaf.bytes),/duplicate/);assert.throws(()=>invalid.vm.supplyModule('../leaf.dll',leaf.bytes),/invalid/);assert.throws(()=>invalid.vm.supplyModule('',leaf.bytes),/invalid/);invalid.vm.dispose();
 const cyclic=machine(mode),a=moduleFixture(mode,{dll:true,forward:'b.GetValue'}),b=moduleFixture(mode,{dll:true,forward:'a.GetValue'});cyclic.vm.supplyModule('a.dll',a.bytes);cyclic.vm.supplyModule('b.dll',b.bytes);assert.throws(()=>cyclic.vm.load(moduleFixture(mode,{dependency:'a.dll'}).bytes),/forwarder/);cyclic.vm.dispose();
 const wrong=machine(mode);wrong.vm.supplyModule('leaf.dll',moduleFixture(mode===32?64:32,{dll:true}).bytes);assert.throws(()=>wrong.vm.load(moduleFixture(mode,{dependency:'leaf.dll'}).bytes),/machine does not match/);wrong.vm.dispose();
 const copied=machine(mode),source=moduleFixture(mode,{dll:true,seed:29});copied.vm.supplyModule('leaf.dll',source.bytes);source.bytes.fill(0);copied.vm.load(moduleFixture(mode,{dependency:'leaf.dll'}).bytes);equal((await copied.vm.run(100n)).reason,Exit.Returned);equal(copied.vm.register(0),29n,'supplied bytes remain owned after caller buffer changes');copied.vm.dispose();
}

console.log(`${checks} PE dependency/export/DllMain/TLS checks passed`);
