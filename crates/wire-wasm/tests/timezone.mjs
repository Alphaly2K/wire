import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {timezoneFixture} from './timezone-fixture.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';

const epoch=Date.UTC(1601,0,1),ticks=fields=>BigInt(Date.UTC(fields[0],fields[1]-1,fields[3],...fields.slice(4))-epoch)*10000n;
const canonical=value=>{const d=new Date(Number(value/10000n)+epoch);return[d.getUTCFullYear(),d.getUTCMonth()+1,d.getUTCDay(),d.getUTCDate(),d.getUTCHours(),d.getUTCMinutes(),d.getUTCSeconds(),d.getUTCMilliseconds()];};
const fields=(vm,p)=>Array.from({length:8},(_,n)=>Number(vm.readInt(p+BigInt(n*2),2)));
const text=(vm,p)=>{let value='';for(let n=0;n<64;n++){const c=Number(vm.readInt(p+BigInt(n*2),2));if(!c)return value;value+=String.fromCharCode(c);}throw Error('unterminated output');};
const profile=(bias,seasonal=false)=>{
 const bytes=new Uint8Array(172),v=new DataView(bytes.buffer);v.setInt32(0,bias,true);
 for(const[n,c]of[...'Standard'].entries())v.setUint16(4+n*2,c.charCodeAt(0),true);
 if(seasonal){for(const[at,date]of[[68,[0,11,0,1,2,0,0,0]],[152,[0,3,0,2,2,0,0,0]]])date.forEach((value,n)=>v.setUint16(at+n*2,value,true));v.setInt32(168,-60,true);}
 return bytes;
};
const wide=value=>Uint8Array.from([...value+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8]));

// One configured profile drives every default-zone API without changing UTC calls.
for(const mode of[32,64])for(const[bias,seasonal,point,expectedBias,zoneMode]of[
 [-480,false,[2024,2,0,29,22,59,58,987],-480,0],
 [300,true,[2024,1,0,15,12,34,56,789],300,1],
 [300,true,[2024,7,0,15,12,34,56,789],240,2],
]){
 const bytes=profile(bias,seasonal),original=bytes.slice(),value=ticks(point)+7654n,local=value-BigInt(expectedBias)*600000000n;
 let samples=0;const vm=new WasmMachine(core,{mode,timeZone:bytes,wallClock:()=>{samples++;return value;}});bytes.fill(0);
 const image=vm.load(timezoneFixture(mode).bytes);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),BigInt(canonical(local)[4]));
 equal(fields(vm,image.base+0x3000n),canonical(value));equal(fields(vm,image.base+0x3010n),canonical(local));
 equal(fields(vm,image.base+0x3180n),canonical(local));equal(fields(vm,image.base+0x3190n),canonical(value));equal(vm.readInt(image.base+0x30d0n),BigInt(zoneMode));
 equal(Uint8Array.from({length:172},(_,n)=>Number(vm.readInt(image.base+0x3020n+BigInt(n),1))),original);
 equal(vm.readInt(image.base+0x30e0n,8),value);equal(vm.readInt(image.base+0x30e8n,8),local);
 const f=canonical(local);equal(text(vm,image.base+0x3100n),`${String(f[1]).padStart(2,'0')}/${String(f[3]).padStart(2,'0')}/${f[0]}`);equal(text(vm,image.base+0x3140n),f.slice(4,7).map(n=>String(n).padStart(2,'0')).join(':'));
 equal(samples,seasonal?7:5);
 const t={vm,mode,image,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68)};
 if(mode===32){equal(vm.readInt(call(t,'__p__timezone',[],'msvcrt.dll')),BigInt.asUintN(32,BigInt(bias*60)));equal(vm.readInt(call(t,'__p__daylight',[],'msvcrt.dll')),BigInt(seasonal));}
 vm.write(image.base+0x30e0n,encode(ticks([2024,1,0,15,12,0,0,0])+4321n,8));
 equal(call(t,'FileTimeToLocalFileTime',[image.base+0x30e0n,image.base+0x30e8n],'kernel32.dll'),1n);equal(vm.readInt(image.base+0x30e8n,8),ticks([2024,1,0,15,12,0,0,0])+4321n-BigInt(expectedBias)*600000000n);
 vm.dispose();
}

// Malformed rules reject construction and release the allocated core owner.
for(const mode of[32,64])for(const bad of['length','type','transition','bias']){
 const handles=[],wrapped={...core,wire_create(...args){const handle=core.wire_create(...args);handles.push(handle);return handle;}};
 let value=profile(300,true);
 if(bad==='length')value=value.slice(1);if(bad==='type')value=Array.from(value);
 if(bad==='transition')new DataView(value.buffer).setUint16(70,0,true);
 if(bad==='bias')new DataView(value.buffer).setInt32(0,0x7fffffff,true);
 assert.throws(()=>new WasmMachine(wrapped,{mode,timeZone:value}),/timezone/);
 equal(handles.length,bad==='length'||bad==='type'?0:1);for(const h of handles)equal(core.wire_state(h),0);
}

// A loaded image cannot change its profile through the raw export.
for(const mode of[32,64]){
 const t=await machine(mode,{timeZone:profile(-480),wallClock:()=>ticks([2024,1,0,1,0,0,0,0])}),{vm}=t;
 frame(t,'GetLocalTime',[0x22000n],'kernel32.dll');const before=snapshot(t),at=core.wire_input(t.handle,172);new Uint8Array(core.memory.buffer,at,172).set(profile(0));
 equal(core.wire_configure_timezone(t.handle),1);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(fields(vm,0x22000n),canonical(ticks([2024,1,0,1,8,0,0,0])));vm.dispose();
}

// Children inherit the immutable profile across all parent/child ABI pairings.
for(const mode of[32,64])for(const childMode of[32,64]){
 const now=ticks([2024,2,0,29,22,0,0,0]),t=await machine(mode,{timeZone:profile(-480),wallClock:()=>now}),{vm}=t;
 vm.supplyFile('C:\\child.exe',timezoneFixture(childMode).bytes);vm.write(0x20000n,wide('C:\\child.exe'));vm.write(0x24000n,new Uint8Array(104));vm.write(0x24000n,encode(BigInt(mode===32?68:104),4));
 equal(call(t,'CreateProcessW',[0x20000n,0n,0n,0n,0n,0n,0n,0n,0x24000n,0x25000n],'kernel32.dll'),1n);
 for(let n=0;n<100 && vm.processes()[1].exitCode===null;n++){const result=await vm.run(1000n);assert.ok([Exit.Budget,Exit.Dispatch,Exit.ProcessExited].includes(result.reason));}
 equal(vm.processes()[1].exitCode,6);assert.ok(vm.processes()[1].retired>0n);vm.dispose();
}
console.log(`${checks} configured timezone/default conversions/locale/CRT/inheritance/lifetime checks passed`);
