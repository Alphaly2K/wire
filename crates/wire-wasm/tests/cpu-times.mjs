import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {cpuFixture} from './cpu-fixture.mjs';
import {readCPUTimeSample} from '../web/platform.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,args)=>call(t,'GetSystemTimes',args,'kernel32.dll');
for(const mode of[32,64]){
 let samples=0,values=[0x123456789abcdef0n,0x23456789abcdef01n,0xf123456789abcdefn];
 const t=await machine(mode,{cpuTimes(){samples++;return values;}}),{vm}=t,ps=[0x22000n,0x23000n,0x24000n];
 vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));vm.write(t.error,encode(123n,4));
 equal(api(t,ps),1n);equal(samples,1);equal(ps.map(p=>vm.readInt(p,8)),values);equal(vm.readInt(t.error),123n);
 equal(api(t,[ps[0],ps[0],ps[0]]),1n);equal(vm.readInt(ps[0],8),values[2]);equal(api(t,[0n,ps[1],0n]),1n);equal(vm.readInt(ps[1],8),values[1]);equal(api(t,[0n,0n,0n]),1n);
 equal(api(t,[t.sp,t.sp+2n,t.sp+4n]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp+4n,8),values[2]);
 if(mode===64){const p=call(t,'VirtualAlloc',[0n,4096n,0x103000n,4n],'kernel32.dll');assert.ok(p>0xffffffffn);equal(api(t,[p,p+8n,p+16n]),1n);equal([0n,8n,16n].map(o=>vm.readInt(p+o,8)),values);call(t,'VirtualFree',[p,0n,0x8000n],'kernel32.dll');}
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Every output and the full saved call frame must be usable before asking for a sample.
for(const mode of[32,64])for(const failed of[0,1,2,'frame','arguments']){
 let samples=0;const values=[11n,22n,33n],t=await machine(mode,{cpuTimes(){samples++;return values;}}),{vm}=t,ps=[0x22000n,0x23000n,0x24000n];
 if(failed==='arguments'&&mode===64){vm.dispose();continue;}
 if(failed==='arguments')t.sp=0x24ffcn;
 let page;if(Number.isInteger(failed)){ps[failed]=0x24ffcn;page=0x25000n;}else page=failed==='arguments'?0x25000n:t.sp&~4095n;
 for(const p of ps)vm.write(p,new Uint8Array(8).fill(0xa5));frame(t,'GetSystemTimes',ps,'kernel32.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,page,4096n,Number.isInteger(failed)?2:1),0);equal(core.wire_service_source(t.handle)&127,1);equal(vm.service().reason,Exit.Fault);equal(samples,0);equal(snapshot(t),before);for(const p of ps)equal(vm.readInt(p,4),0xa5a5a5a5n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(samples,1);equal(ps.map(p=>vm.readInt(p,8)),values);equal(vm.register(0),1n);vm.dispose();
}
// Unavailable, throwing and malformed providers keep their distinct failure protocols.
for(const mode of[32,64]){
 let result=null,throws=false,samples=0;const t=await machine(mode,{cpuTimes(){samples++;if(throws)throw new Error('CPU source failed');return result;}}),{vm}=t,ps=[0x22000n,0x23000n,0x24000n];
 for(const p of ps)vm.write(p,new Uint8Array(8).fill(0xa5));equal(api(t,ps),0n);equal(vm.readInt(t.error),50n);equal(ps.map(p=>vm.readInt(p,8)),new Array(3).fill(0xa5a5a5a5a5a5a5a5n));
 for(const value of[Promise.resolve([1n,2n,3n]),[],[1,2n,3n],[1n,2n],new Array(3),[-1n,1n,1n],[0n,1n,1n<<64n],[3n,2n,1n]]){
  result=value;frame(t,'GetSystemTimes',ps,'kernel32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/CPU times|unsigned|range|64|include|BigInt/);equal(snapshot(t),before);equal(ps.map(p=>vm.readInt(p,8)),new Array(3).fill(0xa5a5a5a5a5a5a5a5n));
 }
 throws=true;frame(t,'GetSystemTimes',ps,'kernel32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/CPU source failed/);equal(snapshot(t),before);throws=false;result=[1n,3n,2n];equal(vm.service().reason,Exit.Dispatch);equal(ps.map(p=>vm.readInt(p,8)),result);
 // A raw host preserves its platform's Windows error value.
 frame(t,'GetSystemTimes',ps,'kernel32.dll');equal(core.wire_service_cpu(t.handle,0n,0n,0n,31),0);equal(vm.register(0),0n);equal(vm.readInt(t.error),31n);
 result=null;frame(t,'GetSystemTimes',ps,'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const errorBefore=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),errorBefore);equal(ps.map(p=>vm.readInt(p,8)),[1n,3n,2n]);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),50n);vm.dispose();
}
// Static imports execute the real JIT gateway in both guest ABIs.
for(const mode of[32,64]){
 const vm=new WasmMachine(core,{mode,clock:()=>0n,cpuTimes:()=>[11n,22n,33n]}),image=vm.load(cpuFixture(mode).bytes);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);equal([0x3000n,0x3008n,0x3010n].map(o=>vm.readInt(image.base+o,8)),[11n,22n,33n]);vm.dispose();
}

// An asynchronous native sample suspends at the preflighted gateway with no guest progress.
for(const mode of[32,64]){
 let release,enter,samples=0,value=[11n,22n,33n];
 const gate=new Promise(resolve=>{release=resolve;}),entered=new Promise(resolve=>{enter=resolve;});
 const t=await machine(mode,{async cpuTimes(){samples++;enter();await gate;return value;}}),{vm}=t,ps=[0x22000n,0x23000n,0x24000n];
 for(const p of ps)vm.write(p,new Uint8Array(8).fill(0xa5));
 frame(t,'GetSystemTimes',ps,'kernel32.dll');equal(core.wire_protect(t.handle,ps[1],4096n,2),0);
 const beforeFault=snapshot(t);equal((await vm.run(1n)).reason,Exit.Fault);equal(samples,0);equal(snapshot(t),beforeFault);
 equal(core.wire_protect(t.handle,ps[1],4096n,4),0);
 const before=snapshot(t),work=vm.work,retired=vm.retired,running=vm.run(1n);await entered;
 equal(snapshot(t),before);equal(vm.work,work);equal(ps.map(p=>vm.readInt(p,8)),new Array(3).fill(0xa5a5a5a5a5a5a5a5n));
 for(const action of[()=>vm.readOutput(),()=>vm.supplyDirectory('C:\\pending'),()=>vm.service()])assert.throws(action,/running/);
 for(const action of[()=>{vm.ip=vm.entryPoints.returned;},()=>vm.register(0,5n),()=>{vm.flags=0n;},()=>vm.xmm(0,new Uint8Array(16))])assert.throws(action,/pending CPU/);
 release();equal((await running).reason,Exit.Budget);equal(samples,1);equal(vm.work,work+1n);equal(vm.retired,retired);equal(ps.map(p=>vm.readInt(p,8)),value);
 for(const error of[31,0xfedcba98]){value={error};frame(t,'GetSystemTimes',ps,'kernel32.dll');equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),0n);equal(vm.readInt(t.error),BigInt(error));equal(ps.map(p=>vm.readInt(p,8)),[11n,22n,33n]);}
 value=undefined;frame(t,'GetSystemTimes',ps,'kernel32.dll');const rejected=snapshot(t),count=samples;await assert.rejects(vm.run(1n),/CPU times/);equal(samples,count+1);equal(snapshot(t),rejected);
 value=[1n,3n,2n];equal((await vm.run(1n)).reason,Exit.Budget);equal(ps.map(p=>vm.readInt(p,8)),value);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Provider rejection and disposal during sampling never commit a stale result.
for(const mode of[32,64]){
 let reject=true;
 const t=await machine(mode,{async cpuTimes(){if(reject)throw Error('native sample failed');return[1n,3n,2n];}}),{vm}=t;
 frame(t,'GetSystemTimes',[0x22000n,0x23000n,0x24000n],'kernel32.dll');const before=snapshot(t);await assert.rejects(vm.run(1n),/native sample failed/);equal(snapshot(t),before);reject=false;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.readInt(0x23000n,8),3n);vm.dispose();
 let release,enter,commits=0;
 const gate=new Promise(resolve=>{release=resolve;}),entered=new Promise(resolve=>{enter=resolve;});
 const v=new WasmMachine({...core,wire_service_cpu(...args){commits++;return core.wire_service_cpu(...args);}},{mode,async cpuTimes(){enter();await gate;return[1n,3n,2n];}});v.load(cpuFixture(mode).bytes);
 const running=v.run(100n);await entered;v.dispose();release();await assert.rejects(running,/disposed/);equal(commits,0);
}

// JSON durations stay decimal strings until conversion; an HTTP failure is not a guest success.
{
 const original=globalThis.fetch;let response={error:0,times:['9223372036854775808','18446744073709551615','12345678901234567890']},ok=true;
 globalThis.fetch=async()=>({ok,json:async()=>response});
 try{
  equal(await readCPUTimeSample(),[9223372036854775808n,18446744073709551615n,12345678901234567890n]);
  response={error:0xfedcba98};equal(await readCPUTimeSample(),response);
  for(const sample of[{error:0,times:[9007199254740993,2,3]},{error:0,times:['1','2']},{error:0,times:['1','2','0x3']},{error:-1},{error:'31'},{}]){response=sample;await assert.rejects(readCPUTimeSample(),/invalid host CPU/);}
  ok=false;await assert.rejects(readCPUTimeSample(),/sampling failed/);
 }finally{globalThis.fetch=original;}
}
console.log(`${checks} cumulative CPU time/provider/ABI/JIT/preflight/fault checks passed`);
