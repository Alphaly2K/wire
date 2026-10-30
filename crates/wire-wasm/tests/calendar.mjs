import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const packed=fields=>fields.flatMap(n=>Array.from(encode(BigInt(n),2)));
const read=(vm,p)=>Array.from({length:8},(_,n)=>Number(vm.readInt(p+BigInt(n*2),2)));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const epoch=Date.UTC(1601,0,1);
const ticks=([y,m,,d,h=0,min=0,s=0,ms=0])=>BigInt(Date.UTC(y,m-1,d,h,min,s,ms)-epoch)*10000n;
const canonical=fields=>{const date=new Date(Number(ticks(fields)/10000n)+epoch);return [date.getUTCFullYear(),date.getUTCMonth()+1,date.getUTCDay(),date.getUTCDate(),date.getUTCHours(),date.getUTCMinutes(),date.getUTCSeconds(),date.getUTCMilliseconds()];};
const point=[2024,2,4,29,23,59,58,987];

// Independent UTC dates cover leap centuries, limits, ignored weekday and precision.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x23000n;
 for(const fields of [[1601,1,6,1,0,0,0,0],[1700,3,0,1,0,0,0,0],[1800,3,0,1,0,0,0,0],[1900,3,0,1,0,0,0,0],[2000,2,0,29,0,0,0,1],point,[2100,3,0,1,0,0,0,0],[30827,12,0,31,23,59,59,999]]){
  vm.write(input,packed(fields));vm.write(t.error,encode(123n,4));equal(api(t,'SystemTimeToFileTime',[input,out]),1n);equal(vm.readInt(out,8),ticks(fields));equal(vm.readInt(t.error),123n);
  vm.write(out,encode(ticks(fields)+9999n,8));equal(api(t,'FileTimeToSystemTime',[out,input]),1n);equal(read(vm,input),canonical(fields));
 }
 for(const fields of [[1600,12,0,31,0,0,0,0],[30828,1,0,1,0,0,0,0],[2024,0,0,1,0,0,0,0],[2024,13,0,1,0,0,0,0],[1900,2,0,29,0,0,0,0],[2024,2,0,30,0,0,0,0],[2024,2,0,0,0,0,0,0],[2024,1,0,1,24,0,0,0],[2024,1,0,1,0,60,0,0],[2024,1,0,1,0,0,60,0],[2024,1,0,1,0,0,0,1000]]){
  vm.write(input,packed(fields));equal(api(t,'SystemTimeToFileTime',[input,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);
 }
 for(const value of [1n<<63n,(1n<<64n)-1n]){vm.write(input,encode(value,8));equal(api(t,'FileTimeToSystemTime',[input,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);equal(api(t,'FileTimeToLocalFileTime',[input,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);}
 for(const [a,b,result] of [[0n,1n,0xffffffffn],[1n,1n,0n],[(1n<<64n)-1n,1n,1n],[0x100000000n,0xffffffffn,1n]]){vm.write(input,encode(a,8));vm.write(out,encode(b,8));equal(api(t,'CompareFileTime',[input,out]),result);}
 for(const value of [0n,ticks(point)+7654n,(1n<<63n)-1n]){vm.write(input,encode(value,8));equal(api(t,'FileTimeToLocalFileTime',[input,out]),1n);equal(vm.readInt(out,8),value);}
 vm.write(input,packed(point));equal(api(t,'SystemTimeToFileTime',[input,input]),1n);equal(vm.readInt(input,8),ticks(point));equal(api(t,'FileTimeToSystemTime',[input,input]),1n);equal(read(vm,input),point);
 vm.write(input,packed(point));equal(api(t,'SystemTimeToFileTime',[input,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,8),ticks(point));
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);vm.write(high,packed(point));equal(api(t,'SystemTimeToFileTime',[high,high+32n]),1n);equal(vm.readInt(high+32n,8),ticks(point));equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

function zone(vm,p,{bias=300,standard=[0,11,0,1,2,0,0,0],daylight=[0,3,0,2,2,0,0,0],standardBias=0,daylightBias=-60}={}){
 vm.write(p,new Uint8Array(172));for(const [at,value]of [[0,bias],[84,standardBias],[168,daylightBias]])vm.write(p+BigInt(at),encode(BigInt(value),4));vm.write(p+68n,packed(standard));vm.write(p+152n,packed(daylight));
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,z=0x22000n,input=0x23000n,out=0x24000n;
 vm.write(z,new Uint8Array(173).fill(0xa5));equal(api(t,'GetTimeZoneInformation',[z]),0n);equal(vm.readInt(z),0n);equal(read(vm,z+68n),[0,0,0,0,0,0,0,0]);equal(vm.readInt(z+4n,8),0x4300540055n);equal(vm.readInt(z+172n,1),0xa5n);
 for(const name of ['SystemTimeToTzSpecificLocalTime','TzSpecificLocalTimeToSystemTime']){vm.write(input,packed(point));equal(api(t,name,[0n,input,out]),1n);equal(read(vm,out),point);}
 zone(vm,z);
 for(const [source,expected]of [
  [[2024,3,0,10,6,59,59,999],[2024,3,0,10,1,59,59,999]],
  [[2024,3,0,10,7,0,0,0],[2024,3,0,10,3,0,0,0]],
  [[2024,11,0,3,5,59,59,999],[2024,11,0,3,1,59,59,999]],
  [[2024,11,0,3,6,0,0,0],[2024,11,0,3,1,0,0,0]],
  [[2024,1,0,1,2,0,0,0],[2023,12,0,31,21,0,0,0]],
 ]){vm.write(input,packed(source));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,out]),1n);equal(read(vm,out),canonical(expected));}
 vm.write(input,packed([2024,11,0,3,1,30,0,0]));equal(api(t,'TzSpecificLocalTimeToSystemTime',[z,input,out]),1n);equal(read(vm,out),canonical([2024,11,0,3,6,30,0,0]));
 vm.write(input,packed([2024,3,0,10,2,30,0,0]));vm.write(out,new Uint8Array(16).fill(0xa5));equal(api(t,'TzSpecificLocalTimeToSystemTime',[z,input,out]),0n);equal(vm.readInt(t.error),87n);equal(bytes(vm,out,16),new Array(16).fill(0xa5));
 zone(vm,z,{bias:-600,standard:[0,4,0,1,3,0,0,0],daylight:[0,10,0,1,2,0,0,0]});
 for(const [month,hour]of [[1,11],[7,10]]){vm.write(input,packed([2024,month,0,15,0,0,0,0]));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,out]),1n);equal(read(vm,out),canonical([2024,month,0,15,hour,0,0,0]));}
 zone(vm,z,{bias:-60,standard:[0,10,0,5,3,0,0,0],daylight:[0,3,0,5,2,0,0,0]});vm.write(input,packed([2024,10,0,27,1,0,0,0]));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,out]),1n);equal(read(vm,out),canonical([2024,10,0,27,2,0,0,0]));
 zone(vm,z,{bias:-345,standard:[0,0,0,0,0,0,0,0],daylight:[0,0,0,0,0,0,0,0],standardBias:90,daylightBias:-90});vm.write(input,packed([2024,1,0,1,0,0,0,0]));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,out]),1n);equal(read(vm,out),canonical([2024,1,0,1,5,45,0,0]));
 zone(vm,z,{standard:[2024,11,0,3,2,0,0,0],daylight:[2024,3,0,10,2,0,0,0]});vm.write(input,packed([2024,7,0,1,12,0,0,0]));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,out]),1n);equal(read(vm,out),canonical([2024,7,0,1,8,0,0,0]));
 for(const options of [{standard:[0,0,0,0,0,0,0,0]},{daylight:[0,3,7,2,2,0,0,0]},{daylight:[0,3,0,6,2,0,0,0]},{daylight:[2024,2,0,30,0,0,0,0]}]){zone(vm,z,options);equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,0xdead0000n]),0n);equal(vm.readInt(t.error),87n);}
 zone(vm,z);vm.write(input,packed(point));equal(api(t,'SystemTimeToTzSpecificLocalTime',[z,input,z]),1n);equal(read(vm,z),canonical([2024,2,0,29,18,59,58,987]));
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Whole output, input, error slot and original return frame faults retain CPU state.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x24ffcn;
 for(const [name,data,args]of [
  ['SystemTimeToFileTime',packed(point),[input,out]],
  ['FileTimeToSystemTime',Array.from(encode(ticks(point),8)),[input,out]],
  ['FileTimeToLocalFileTime',Array.from(encode(ticks(point)+321n,8)),[input,out]],
  ['SystemTimeToTzSpecificLocalTime',packed(point),[0n,input,out]],
  ['TzSpecificLocalTimeToSystemTime',packed(point),[0n,input,out]],
  ['GetTimeZoneInformation',[],[out]],
 ]){
  vm.write(input,data);vm.write(out,new Uint8Array(172).fill(0xa5));frame(t,name,args,'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out),0xa5a5a5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);
 }
 vm.write(input,packed([1900,2,0,29,0,0,0,0]));frame(t,'SystemTimeToFileTime',[input,0xdead0000n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),87n);
 vm.write(input,packed(point));frame(t,'SystemTimeToFileTime',[input,0x23000n],'kernel32.dll');equal(core.wire_protect(t.handle,input,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,input,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 frame(t,'SystemTimeToFileTime',[input,0x23000n],'kernel32.dll');const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 let value=ticks(point)+7654n,fail=false;
 const t=await machine(mode,{wallClock(){if(fail)throw new Error('calendar clock unavailable');return value;}}),{vm}=t,out=0x22000n;
 vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));
 for(const name of ['GetSystemTime','GetLocalTime','GetSystemTimePreciseAsFileTime']){
  const samples=t.time.wallCalls;vm.write(t.error,encode(123n,4));equal(api(t,name,[out]),0n);equal(t.time.wallCalls,samples+1);if(name==='GetSystemTimePreciseAsFileTime')equal(vm.readInt(out,8),value);else equal(read(vm,out),point);equal(vm.readInt(t.error),123n);
  frame(t,name,[out],'kernel32.dll');fail=true;const before=snapshot(t);assert.throws(()=>vm.service(),/calendar clock unavailable/);equal(snapshot(t),before);fail=false;equal(vm.service().reason,Exit.Dispatch);
 }
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(t.time.calls,0);
 value=1n<<63n;frame(t,'GetSystemTime',[out],'kernel32.dll');const before=snapshot(t);assert.throws(()=>vm.service(),/wall clock is outside/);equal(snapshot(t),before);
 value=ticks(point);frame(t,'GetLocalTime',[0x24ffcn],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const beforeFault=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),beforeFault);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 for(const name of ['GetSystemTime','GetLocalTime','SystemTimeToFileTime','FileTimeToSystemTime','CompareFileTime','FileTimeToLocalFileTime','GetTimeZoneInformation','SystemTimeToTzSpecificLocalTime','TzSpecificLocalTimeToSystemTime','GetSystemTimePreciseAsFileTime']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('ntdll.dll',name),/unknown/);}
 equal(vm.bind('kernel32.dll','GetSystemTimes'),vm.bind('kernelbase.dll','GetSystemTimes'));equal(api(t,'GetSystemTimes',[0n,0n,0n]),0n);equal(vm.readInt(t.error),50n);vm.dispose();
}
console.log(`${checks} calendar/date/UTC/DST/precision/ABI/clock/fault checks passed`);
