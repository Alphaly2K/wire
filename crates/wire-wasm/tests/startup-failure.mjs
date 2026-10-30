import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout} from './x87-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {dynamicFixture,trace} from './dynamic-fixture.mjs';

let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
const count=(t,id,offset=0)=>t.vm.readInt(trace+BigInt(id*16+offset));
function machine(mode){
 let handle;const vm=new WasmMachine({...core,wire_create(...args){handle=core.wire_create(...args);return handle;}},{mode});vm.map(0x20000n,0x4000n);
 vm.supplyModule('leaf.dll',dynamicFixture(mode,{id:1,tls:true,seed:73}).bytes);vm.supplyModule('bad.dll',dynamicFixture(mode,{id:2,dependency:'leaf.dll',tls:true,attach:false}).bytes);
 const image=vm.load(moduleFixture(mode,{dependency:'bad.dll',tls:true}).bytes);return {vm,handle,mode,image,error:image.teb+BigInt(mode===32?0x34:0x68),bases:[image.base,vm.moduleBase('bad.dll'),vm.moduleBase('leaf.dll')]};
}
async function rejected(t,budget){
 for(let n=0;n<5000;n++){
  try{equal((await t.vm.run(budget)).reason,Exit.Budget);}
  catch(error){assert.match(error.message,/attach returned FALSE/);return;}
 }
 throw new Error('startup never reported its rejected DllMain');
}
function verify(t){
 const {vm,image,mode}=t;equal([count(t,1),count(t,2)],[1n,1n]);equal([count(t,1,4),count(t,2,4)],[1n,1n]);equal([count(t,1,8),count(t,2,8),count(t,1,12),count(t,2,12)],[2n,2n,2n,2n]);equal(vm.readInt(trace+0x400n),0x21n);equal(vm.readInt(t.error),1114n);
 for(const name of ['program.exe','bad.dll','leaf.dll'])equal(vm.moduleBase(name),0n);
 for(const base of t.bases)assert.throws(()=>vm.readInt(base),/memory access/);
 equal([0n,1n,2n].map(slot=>vm.readInt(image.tlsArray+slot*BigInt(mode/8),mode/8)),[0n,0n,0n]);equal(vm.ip,vm.entryPoints.callback);equal(vm.register(0),0n);equal(vm.work,vm.retired,'loader cleanup creates no synthetic guest retirement');equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});
}

for(const mode of [32,64])for(const budget of [1n,1000n]){
 const t=machine(mode),blocks=[0n,1n,2n].map(slot=>t.vm.readInt(t.image.tlsArray+slot*BigInt(mode/8),mode/8));await rejected(t,budget);verify(t);for(const block of blocks)assert.throws(()=>t.vm.readInt(block),/memory access/);
 const before=snapshot(t);await assert.rejects(t.vm.run(1n),/attach returned FALSE/);equal(snapshot(t),before);equal((await t.vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);t.vm.ip=0x20000n;const edited=snapshot(t);await assert.rejects(t.vm.run(1n),/attach returned FALSE/);equal(snapshot(t),edited,'editing the PC cannot resume a rejected process');t.vm.dispose();
}
for(const mode of [32,64])for(const failure of ['slots','error','frame']){
 const t=machine(mode),{vm,image}=t;
 if(failure==='frame'){
  for(let n=0;n<1000;n++){if(vm.ip===vm.entryPoints.callback&&vm.register(0)===0n&&count(t,2)===1n)break;equal((await vm.run(1n)).reason,Exit.Budget);assert.ok(n<999);}
 }
 const page=failure==='slots'?image.tlsArray:failure==='error'?image.teb:image.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);let fault;
 for(let n=0;n<5000;n++){fault=await vm.run(1n);if(fault.reason===Exit.Fault)break;equal(fault.reason,Exit.Budget);assert.ok(n<4999);}
 equal(fault.reason,Exit.Fault);equal(vm.moduleBase('bad.dll'),t.bases[1]);const before=snapshot(t);equal((await vm.run(0n)).reason,Exit.Budget);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);await rejected(t,1n);verify(t);equal(count(t,2),1n,'retry never repeats the failed attach');equal(count(t,1,4),1n,'retry never repeats completed detach');vm.dispose();
}
console.log(`${checks} rejected static startup/rollback/retry/resource checks passed`);
