import assert from 'node:assert/strict';
import {Exit} from '../web/runtime.mjs';
import {core,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const rel=(target,after)=>encode(BigInt.asUintN(32,target-after),4);
const state=test=>new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();

for(const mode of [32,64]){
 const test=program([0xcc],mode),{vm}=test,p=vm.entryPoints;
 equal(p,mode===32?{services:0x6fff0000n,serviceBytes:0x10000n,returned:0x6fff0000n,callback:0x70000000n,threadReturned:0x6fff0010n}:{services:0x7ffe00000000n,serviceBytes:0x100000n,returned:0x7ffe10000000n,callback:0x7ffe10000010n,threadReturned:0x7ffe10000020n});
 const points=[[p.services+32n,Exit.Service],[p.services+33n,Exit.Service],[p.services+p.serviceBytes-1n,Exit.Service],[p.returned,Exit.Returned],[p.callback,Exit.Callback],[p.threadReturned,Exit.ThreadReturned]];
 const pages=new Set(points.map(([pc])=>pc&~4095n));for(const page of pages){vm.map(page,4096n,7);vm.write(page,new Uint8Array(4096).fill(0x90));}
 for(const [pc,reason] of points){
  vm.ip=pc;vm.register(0,123n);vm.flags=0xed7n;const before=state(test);equal(await vm.run(0n),{reason:Exit.Budget,work:0n,ip:pc});equal(await vm.run(10n),{reason,work:0n,ip:pc});equal(state(test),before,'logical entries never fetch or execute mapped bytes');equal(vm.stats(),{modules:0,chargedBytes:0,instances:0});
 }
 const page=(p.services+32n)&~4095n;equal(core.wire_protect(test.handle,page,4096n,0x120),0);vm.ip=p.services+32n;equal((await vm.run(10n)).reason,Exit.Service);core.wire_load(test.handle,page,1,1);equal(core.wire_fault_kind(test.handle),4,'entry classification does not consume a guard');vm.dispose();

 for(const [pc,reason] of points){
  const item=program([0x00,0xd8,0xff,0xe2],mode),machine=item.vm;machine.register(0,255n);machine.register(3,1n);machine.register(2,pc);equal((await machine.run(20n)).reason,reason);equal(machine.ip,pc);equal(machine.work,2n);equal(machine.flags,0x257n);equal(machine.register(0),0n);const stats=machine.stats();machine.ip=base;machine.register(0,255n);equal((await machine.run(2n)).reason,Exit.Budget);equal(machine.ip,pc);equal((await machine.run(1n)).reason,reason);equal(machine.stats(),stats,'reaching logical entries does not install synthetic code modules');machine.dispose();
 }

 for(const call of [false,true]){
  const item=program([0xcc],mode),machine=item.vm,entry=p.services-4096n,target=p.services+32n;
  machine.map(entry,4096n,7);machine.map(data,4096n);machine.register(4,data+4096n);machine.register(0,255n);machine.register(3,1n);machine.write(entry,[0x00,0xd8,call?0xe8:0xe9,...rel(target,entry+7n),0xcc]);machine.ip=entry;
  equal((await machine.run(20n)).reason,Exit.Service);equal(machine.ip,target);equal(machine.work,2n);equal(machine.flags,0x257n);equal(machine.register(4),data+4096n-BigInt(call?mode/8:0));if(call)equal(BigInt.asUintN(mode,core.wire_load(item.handle,machine.register(4),mode/8,1)),entry+7n);
  const module=core.wire_ticket(item.handle);equal(core.wire_ready(item.handle,module),0);machine.ip=entry;machine.register(4,data+4096n);equal((await machine.run(2n)).reason,Exit.Budget);equal((await machine.run(1n)).reason,Exit.Service);machine.dispose();
 }

 for(const pc of [p.services-1n,p.returned-1n,p.callback-1n,p.threadReturned-1n]){
  if(pc>=p.services&&pc<p.services+p.serviceBytes)continue;
  const item=program([0xcc],mode),machine=item.vm,page=pc&~4095n;machine.map(page,4096n,7);if(((pc+1n)&4095n)===0n)machine.map(page+4096n,4096n,7);
  machine.write(pc,[0xb0,99]);machine.ip=pc;machine.register(0,0x1234n);const before=state(item);equal((await machine.run(10n)).reason,Exit.Fault);equal(state(item),before,'an instruction spanning a logical entry cannot retire');equal(core.wire_fault_kind(item.handle),3);equal(BigInt.asUintN(64,core.wire_fault_address(item.handle)),pc+1n);equal(core.wire_fault_access(item.handle),4);equal(machine.stats().modules,0);machine.dispose();
 }

 // A branch skips a spanning instruction in the same mapped page. Returning
 // to that instruction later must still produce a precise fetch failure.
 const item=program([0xcc],mode),machine=item.vm,pc=(mode===32?p.returned:p.callback)-1n,nearPage=pc&~4095n;
 machine.map(nearPage,8192n,7);machine.write(pc-2n,[0xeb,1,0xb0,99,0xcc]);machine.ip=pc-2n;equal((await machine.run(10n)).reason,mode===32?Exit.Returned:Exit.Callback);equal(machine.work,1n);machine.ip=pc;equal((await machine.run(10n)).reason,Exit.Fault);equal(machine.ip,pc);equal(machine.work,1n);machine.dispose();
}

console.log(`${checks} Windows gateway/return-boundary checks passed`);
