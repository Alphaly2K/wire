import assert from 'node:assert/strict';
import {Exit,WasmMachine} from '../web/runtime.mjs';
import {core,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=test=>new Uint8Array(core.memory.buffer,core.wire_state(test.handle),stateLayout[0]).slice();
const load=(test,address,size=8)=>BigInt.asUintN(64,core.wire_load(test.handle,address,size,1));
const names=['GetTickCount','GetTickCount64','timeGetTime','QueryPerformanceCounter','QueryPerformanceFrequency','GetSystemTimeAsFileTime'];
const owner=name=>name==='timeGetTime'?'winmm.dll':'kernel32.dll';
function frame(test,address,output=0x30000n){const {vm}=test,mode=vm.entryPoints.services<0x100000000n?32:64,sp=data+128n;vm.ip=address;vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));if(mode===32)vm.write(sp+4n,encode(output,4));else vm.register(1,output);return sp;}

for(const mode of [32,64])for(const name of names){
 let ticks=0xfedcba9876543210n,wall=0x123456789abcdef0n,reads=0,walls=0;
 const test=program([0xcc],mode,{clock(){reads++;return ticks;},wallClock(){walls++;return wall;}}),{vm}=test,address=vm.bind(owner(name),name),output=0x30000n;
 equal(vm.bind(owner(name).toUpperCase(),name),address);if(mode===64&&name!=='timeGetTime')equal(vm.bind('kernelbase.dll',name),address);
 vm.map(data,4096n);vm.map(output,8192n);vm.register(3,7n);vm.flags=0x203n;const saved=fp(test);saved.cw=0x27f;saved.sw=0x4520;saved.tags=0x93a5;saved.regs=saved.regs.map((_,n)=>(0x3fffn<<64n)|BigInt(n+1));fp(test,saved);vm.xmm(0,Uint8Array.from({length:16},(_,n)=>n*13));
 const pointer=name.startsWith('Query')||name==='GetSystemTimeAsFileTime';
 const code=[...(pointer?(mode===32?[0x68,...encode(output,4)]:[0x48,0xb9,...encode(output,8)]):[]),...(mode===32?[0xb8,...encode(address,4)]:[0x48,0xb8,...encode(address,8)]),0xff,0xd0,0x83,0xd3,0,0xcc];vm.write(base,code);vm.register(4,data+4096n);const initialFp=fp(test),initialXmm=vm.xmm(0),callWork=BigInt(pointer?3:2);
 equal((await vm.run(callWork)).reason,Exit.Budget);equal(vm.ip,address);equal((await vm.run(1n)).reason,Exit.Service);equal(vm.work,callWork);const retired=vm.retired,sp=vm.register(4);
 const result=vm.service();equal(result.reason,Exit.Dispatch);equal(result.work,1n);equal(vm.work,callWork+1n);equal(vm.retired,retired);equal(vm.register(4),sp+BigInt(mode/8+(mode===32&&pointer?4:0)));equal(vm.flags,0x203n);equal(fp(test),initialFp);equal(vm.xmm(0),initialXmm);
 const value=name==='QueryPerformanceCounter'?ticks:name==='QueryPerformanceFrequency'?10000000n:name==='GetSystemTimeAsFileTime'?wall:ticks/10000n;
 if(pointer)equal(load(test,output),value);else equal(vm.register(0),name==='GetTickCount64'&&mode===64?value:value&0xffffffffn);
 if(name==='GetTickCount64'&&mode===32)equal(vm.register(2),value>>32n);
 equal(reads,name==='QueryPerformanceFrequency'||name==='GetSystemTimeAsFileTime'?0:1);equal(walls,name==='GetSystemTimeAsFileTime'?1:0);equal((await vm.run(20n)).reason,Exit.Unsupported);equal(vm.register(3),8n,'arithmetic carry survives a service call');equal(vm.work,vm.retired+1n);vm.dispose();
}

for(const mode of [32,64]){
 const test=program([0xcc],mode,{clock:()=>0x1122334455667788n}),{vm}=test,address=vm.bind('kernel32.dll','QueryPerformanceCounter');vm.map(data,8192n);vm.map(0x30000n,8192n);
 const sp=frame(test,address,data+128n);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned,'output may overwrite the saved return address');equal(load(test,sp),0x1122334455667788n);equal((await vm.run(1n)).reason,Exit.Returned);
 const output=0x30000n+4093n;frame(test,address,output);vm.write(output,new Uint8Array(8).fill(0xa5));equal(core.wire_protect(test.handle,0x31000n,4096n,2),0);const before=snapshot(test);const failed=vm.service();equal(failed.reason,Exit.Fault);equal(failed.work,0n);equal(failed.fault.access,2);equal(snapshot(test),before,'failed service output preserves the complete CPU');equal(core.wire_protect(test.handle,0x31000n,4096n,4),0);equal(load(test,output),0xa5a5a5a5a5a5a5a5n);equal(vm.service().reason,Exit.Dispatch);equal(load(test,output),0x1122334455667788n);equal(vm.work,2n);
 frame(test,address);equal(core.wire_protect(test.handle,data,4096n,1),0);const missing=snapshot(test);equal(vm.service().reason,Exit.Fault);equal(snapshot(test),missing,'failed return-frame read preserves the CPU');equal(core.wire_protect(test.handle,data,4096n,4),0);
 vm.ip=address+1n;const untouched=snapshot(test);assert.throws(()=>vm.service(),/unbound/);equal(snapshot(test),untouched);assert.throws(()=>vm.bind('user32.dll','GetTickCount'),/unknown/);assert.throws(()=>vm.bind('kernel32.dll','gettickcount'),/unknown/);assert.throws(()=>vm.bind('kernel32.dll\0x','GetTickCount'),/invalid/);
 // A service writing observed source bytes must revoke the installed code.
 vm.ip=base;equal((await vm.run(1n)).reason,Exit.Unsupported);equal(vm.stats().modules,1);const ticket=core.wire_ticket(test.handle);frame(test,address,base);equal(vm.service().reason,Exit.Dispatch);equal(core.wire_live(test.handle,ticket),0);equal(vm.stats().modules,0);
 vm.dispose();
}

for(const mode of [32,64]){
 let sample=0n;const error=new Error('clock unavailable'),test=program([0xcc],mode,{clock(){if(sample===0n)throw error;return sample;}}),{vm}=test;vm.map(data,4096n);const address=vm.bind('kernel32.dll','GetTickCount64');frame(test,address);const before=snapshot(test);assert.throws(()=>vm.service(),e=>e===error);equal(snapshot(test),before);sample=12345678n;equal(vm.service().reason,Exit.Dispatch);frame(test,address);sample=1n;const back=snapshot(test);assert.throws(()=>vm.service(),/backwards/);equal(snapshot(test),back);vm.dispose();
}
for(const value of [1,-1n,1n<<64n]){
 const test=program([0xcc],64,{wallClock:()=>value}),{vm}=test;vm.map(data,4096n);frame(test,vm.bind('kernel32.dll','GetSystemTimeAsFileTime'));const before=snapshot(test);assert.throws(()=>vm.service(),value===1?TypeError:RangeError);equal(snapshot(test),before);vm.dispose();
}
{
 let calls=0;const test=program([0xcc],64,{clock(){calls++;throw new Error('must not be sampled');},wallClock:()=>100n}),{vm}=test;vm.map(data,4096n);frame(test,vm.bind('kernel32.dll','QueryPerformanceFrequency'),data+512n);equal(vm.service().reason,Exit.Dispatch);equal(calls,0);equal(load(test,data+512n),10000000n);const other=new WasmMachine(core,{mode:64,ip:vm.entryPoints.services});assert.throws(()=>other.service(),/unbound/);equal(other.work,0n);other.dispose();vm.dispose();
}
{
 const original=Date.now;Date.now=()=>1700000000123;
 try{const test=program([0xcc]),{vm}=test;vm.map(data,4096n);frame(test,vm.bind('kernel32.dll','GetSystemTimeAsFileTime'),data+512n);equal(vm.service().reason,Exit.Dispatch);equal(load(test,data+512n),133444736001230000n);vm.dispose();}finally{Date.now=original;}
}
{
 const test=program([0xcc]),{vm}=test;vm.map(data,4096n);const address=vm.bind('kernel32.dll','GetTickCount');equal((await vm.run(1n)).reason,Exit.Unsupported);equal(core.wire_begin(test.handle,core.wire_ticket(test.handle)),1);frame(test,address);const before=snapshot(test);equal(core.wire_service(test.handle,10000n),2,'active execution leases forbid service dispatch');equal(snapshot(test),before);core.wire_end(test.handle);vm.dispose();
}

console.log(`${checks} Windows clock service/call-frame checks passed`);
