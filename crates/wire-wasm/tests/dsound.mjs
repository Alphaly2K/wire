import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,call,window} from './gdi-harness.mjs';
import {createAudioSink} from '../web/audio.mjs';
const invalid=0x80070057n,notimpl=0x80004001n;
const api=(t,n,a=[])=>call(t,n,a,'dsound.dll'),kernel=(t,n,a=[])=>call(t,n,a,'kernel32.dll');
const notifyIID=[0x83,7,0x21,0xb0,0xcd,0x89,0xd0,0x11,0xaf,8,0,0xa0,0xc9,0x25,0xcd,0x16];
const bufferIID=[0x85,0xfa,0x9a,0x27,0x81,0x49,0xce,0x11,0xa5,0x21,0,0x20,0xaf,0xb,0xe5,0x60];
class Capture {samples=[];closes=0;blocked=false;failure=false;write(samples){if(this.failure){this.failure=false;throw new Error('sink failure');}if(this.blocked)return false;this.samples.push(samples.slice());return true;}close(){this.closes++;}}
async function setup(attach=true){let now=0n;const t=await machine(32,{clock:()=>now}),sink=new Capture();if(attach)await t.vm.enableAudio({sink});return{...t,sink,now:value=>{now=value;}};}
function frame(t,pc,args=[]){t.vm.ip=pc;t.vm.register(4,t.sp);t.vm.write(t.sp,encode(t.vm.entryPoints.returned,4));for(const[n,a]of args.entries())t.vm.write(t.sp+4n+BigInt(n*4),encode(a,4));}
const method=(t,p,slot)=>t.vm.readInt(t.vm.readInt(p)+BigInt(slot*4));
function invoke(t,p,slot,args=[]){const{vm}=t;frame(t,method(t,p,slot),[p,...args]);const work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(vm.retired,retired);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(4),t.sp+8n+BigInt(args.length*4));return vm.register(0);}
function device(t,name='DirectSoundCreate8'){equal(api(t,name,[0n,0x20100n,0n]),0n);return t.vm.readInt(0x20100n);}
function buffer(t,d,{flags=0x1e0,bytes=8,channels=1,rate=44100,bits=16,primary=false}={}){
 const{vm}=t;vm.write(0x20200n,[20,flags|(primary?1:0),primary?0:bytes,0,primary?0:0x20300].flatMap(n=>Array.from(encode(BigInt(n),4))));
 const format=new Uint8Array(18),v=new DataView(format.buffer),block=channels*bits/8;v.setUint16(0,1,true);v.setUint16(2,channels,true);v.setUint32(4,rate,true);v.setUint32(8,rate*block,true);v.setUint16(12,block,true);v.setUint16(14,bits,true);vm.write(0x20300n,format);
 equal(invoke(t,d,3,[0x20200n,0x20104n,0n]),0n);return vm.readInt(0x20104n);
}
function lock(t,b,offset=0n,length=8n,flags=0n){equal(invoke(t,b,11,[offset,length,0x20400n,0x20404n,0x20408n,0x2040cn,flags]),0n);return[0n,4n,8n,12n].map(o=>t.vm.readInt(0x20400n+o));}
const unlock=(t,b,a)=>invoke(t,b,19,a);
function fill(t,b,values){const a=lock(t,b,0n,BigInt(values.length*2));t.vm.write(a[0],values.flatMap(n=>Array.from(encode(BigInt.asUintN(16,BigInt(n)),2))));equal(unlock(t,b,a),0n);return a[0];}
const play=(t,b,loop=true)=>invoke(t,b,12,[0n,0n,loop?1n:0n]);

// A real PCM consumer is required; known export/ordinal and private method visibility stay distinct.
{
 const t=await setup(false),{vm}=t;vm.write(0x20100n,encode(0xa5n,4));equal(api(t,'DirectSoundCreate',[0n,0x20100n,0n]),0x88780078n);equal(vm.readInt(0x20100n),0n);equal(api(t,'DirectSoundCreate',[0n,0x20100n,1n]),0x80040110n);equal(api(t,'DirectSoundCreate',[0n,0n,0n]),invalid);vm.write(0x20000n,new Uint8Array(16).fill(1));equal(api(t,'DirectSoundCreate',[0x20000n,0x20100n,0n]),0x88780078n);
 await vm.enableAudio({sink:t.sink});equal(vm.bind('dsound.dll','#1'),vm.bind('DSOUND','DirectSoundCreate'));equal(vm.bind('dsound.dll','#11'),vm.bind('dsound.dll','DirectSoundCreate8'));const d=device(t);assert.throws(()=>vm.bind('dsound.dll','IDirectSound8.AddRef'),/unknown/);assert.throws(()=>vm.bind('kernel32.dll','DirectSoundCreate'),/unknown/);assert.throws(()=>vm.bind('dsound.dll','#4'),/unknown/);
 for(const[name,n]of[['DirectSoundEnumerateA',2],['DirectSoundCaptureCreate8',3],['DirectSoundFullDuplexCreate',10],['DllCanUnloadNow',0]])equal(api(t,name,new Array(n).fill(0n)),notimpl);equal(invoke(t,d,2),0n);vm.dispose();equal(t.sink.closes,1);
 const q=await machine(64);assert.throws(()=>q.vm.bind('dsound.dll','DirectSoundCreate'),/unknown/);await assert.rejects(q.vm.enableAudio({sink:new Capture()}),/PE32/);q.vm.dispose();
}
// The complete legacy COM tables retain their unsupported slots and exact stack cleanup.
{
 const t=await setup(),d=device(t),b=buffer(t,d),{vm}=t;vm.write(0x20500n,notifyIID);equal(invoke(t,b,0,[0x20500n,0x20108n]),0n);const notify=vm.readInt(0x20108n);
 for(const[p,slot,args]of[[d,5,[b,0x20110n]],[d,10,[0n]],[d,11,[0x20110n]],[b,10,[d,0x20200n]]])equal(invoke(t,p,slot,args),notimpl);
 equal(invoke(t,d,7),0n);equal(invoke(t,d,8,[0x20110n]),0n);equal(vm.readInt(0x20110n),4n);equal(invoke(t,d,9,[4n]),0n);equal(invoke(t,d,9,[1n]),notimpl);equal(invoke(t,d,6,[0n,2n]),invalid);const hwnd=await window(t);equal(invoke(t,d,6,[hwnd,2n]),0n);
 vm.write(0x20500n,bufferIID);equal(invoke(t,notify,0,[0x20500n,0x20110n]),0n);equal(vm.readInt(0x20110n),b);equal(invoke(t,b,2),2n);equal(invoke(t,b,0,[0x20500n,0x20110n]),0n);equal(vm.readInt(0x20110n),b);equal(invoke(t,b,2),2n);equal(invoke(t,b,1),3n);equal(invoke(t,b,2),2n);
 equal(invoke(t,b,2),1n);equal(invoke(t,notify,2),0n);equal(invoke(t,b,1),invalid);equal(invoke(t,d,2),0n);vm.dispose();
}
// PCM mixing, clipping, resampling and cursor notification are driven by the injected monotonic clock.
{
 const t=await setup(),d=device(t),b=buffer(t,d),other=buffer(t,d,{bytes:2}),{vm,sink}=t;fill(t,b,[0,32767,-32768,16384]);fill(t,other,[16384]);
 vm.write(0x20500n,notifyIID);equal(invoke(t,b,0,[0x20500n,0x20108n]),0n);const notify=vm.readInt(0x20108n),event=kernel(t,'CreateEventW',[0n,0n,0n,0n]);vm.write(0x20600n,[2n,event].flatMap(n=>Array.from(encode(n,4))));equal(invoke(t,notify,3,[1n,0x20600n]),0n);equal(play(t,b),0n);equal(play(t,other),0n);
 t.now(50_000n);equal(invoke(t,b,4,[0x20700n,0x20704n]),0n);equal(sink.samples.length,1);equal(sink.samples[0],Int16Array.from({length:440},(_,i)=>[16384,32767,-16384,32767][Math.floor(i/2)%4]));equal(vm.readInt(0x20700n),0n);equal(kernel(t,'WaitForSingleObject',[event,0n]),0n);equal(kernel(t,'WaitForSingleObject',[event,0n]),258n);equal(vm.audioStats().mixedFrames,220n);
 equal(invoke(t,other,18),0n);equal(invoke(t,b,18),0n);equal(invoke(t,b,17,[22050n]),0n);equal(invoke(t,b,16,[10000n]),0n);equal(invoke(t,b,13,[0n]),0n);equal(play(t,b),0n);t.now(100_000n);equal(invoke(t,b,9,[0x20700n]),0n);equal(Array.from(sink.samples[1].subarray(0,16)),[0,0,0,0,0,32767,0,32767,0,-32768,0,-32768,0,16384,0,16384]);
 equal(invoke(t,b,18),0n);equal(invoke(t,b,17,[44100n]),0n);equal(invoke(t,b,16,[0n]),0n);equal(invoke(t,b,13,[0n]),0n);equal(play(t,b,false),0n);t.now(150_000n);invoke(t,b,9,[0x20700n]);equal(vm.readInt(0x20700n),0n);equal(Array.from(sink.samples[2].subarray(0,12)),[0,0,32767,32767,-32768,-32768,16384,16384,0,0,0,0]);
 for(const p of[notify,b,other,d])invoke(t,p,2);vm.dispose();
}
// An already waiting guest resumes through its original wait frame after a PCM event.
{
 const t=await setup(),{vm}=t,d=device(t),b=buffer(t,d);fill(t,b,[1,2,3,4]);vm.write(0x20500n,notifyIID);invoke(t,b,0,[0x20500n,0x20108n]);const notify=vm.readInt(0x20108n),event=kernel(t,'CreateEventW',[0n,0n,0n,0n]);vm.write(0x20600n,[2n,event].flatMap(n=>Array.from(encode(n,4))));invoke(t,notify,3,[1n,0x20600n]);play(t,b);frame(t,vm.bind('kernel32.dll','WaitForSingleObject'),[event,0xffffffffn]);equal(vm.service().reason,Exit.Waiting);t.now(50_000n);equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.register(0),0n);equal(vm.ip,vm.entryPoints.returned);
 invoke(t,b,18);vm.write(0x20600n,[0xffffffffn,event].flatMap(n=>Array.from(encode(n,4))));invoke(t,notify,3,[1n,0x20600n]);play(t,b);invoke(t,b,18);equal(kernel(t,'WaitForSingleObject',[event,0n]),0n);vm.dispose();
}
// Wrapped locks, controls and primary format changes use one guest backing and owned allocations.
{
 const t=await setup(),{vm}=t,d=device(t,'DirectSoundCreate'),b=buffer(t,d),heap=kernel(t,'GetProcessHeap');const a=lock(t,b,6n,4n);equal(a[0],a[2]+6n);equal(a[1],2n);equal(a[3],2n);equal(unlock(t,b,[a[0],3n,a[2],2n]),invalid);equal(unlock(t,b,a),0n);
 for(const[slot,value]of[[15,(-600n)&0xffffffffn],[16,500n],[17,22050n]])equal(invoke(t,b,slot,[value]),0n);for(const[slot,value]of[[6,(-600n)&0xffffffffn],[7,500n],[8,22050n]]){equal(invoke(t,b,slot,[0x20700n]),0n);equal(vm.readInt(0x20700n),value);}equal(invoke(t,b,15,[1n]),invalid);equal(invoke(t,b,20),0n);
 vm.write(0x20700n,encode(20n,4));equal(invoke(t,b,3,[0x20700n]),0n);equal(vm.readInt(0x20708n),8n);equal(invoke(t,b,5,[0x20800n,18n,0x20700n]),0n);equal(vm.readInt(0x20700n),18n);equal(vm.readInt(0x2080en,2),16n);equal(invoke(t,b,14,[0x20300n]),0x88780032n);
 const primary=buffer(t,d,{primary:true,flags:0});equal(invoke(t,primary,14,[0x20300n]),0n);equal(invoke(t,primary,2),0n);const eight=buffer(t,d,{bits:8,bytes:4,flags:0});const raw=lock(t,eight,0n,4n);equal([0n,1n,2n,3n].map(n=>vm.readInt(raw[0]+n,1)),[128n,128n,128n,128n]);unlock(t,eight,raw);equal(invoke(t,eight,6,[0x20700n]),0x8878001en);invoke(t,eight,2);
 invoke(t,b,2);equal(kernel(t,'HeapSize',[heap,0n,a[2]]),0xffffffffn);invoke(t,d,2);vm.dispose();
}
// A blocked/failed consumer retains exactly the same PCM packet without advancing twice.
{
 const t=await setup(),{vm,sink}=t,d=device(t),b=buffer(t,d);fill(t,b,[1,2,3,4]);play(t,b);sink.blocked=true;t.now(50_000n);invoke(t,b,9,[0x20700n]);equal(vm.audioStats().pendingFrames,220);equal(vm.audioStats().mixedFrames,220n);equal(sink.samples.length,0);invoke(t,b,9,[0x20700n]);equal(vm.audioStats().mixedFrames,220n);
 sink.blocked=false;sink.failure=true;frame(t,method(t,b,9),[b,0x20700n]);const before=snapshot(t);assert.throws(()=>vm.service(),/sink failure/);equal(snapshot(t),before);equal(vm.audioStats().pendingFrames,220);equal(vm.service().reason,Exit.Dispatch);equal(sink.samples.length,1);equal(vm.audioStats().mixedFrames,220n);equal(vm.audioStats().pendingFrames,0);vm.dispose();equal(sink.closes,1);
}
// A PCM read fault stops before any cursor or event changes and retries the whole mix once.
{
 const t=await setup(),{vm,sink}=t,d=device(t),b=buffer(t,d,{bytes:12}),data=fill(t,b,[1,2,3,4,5,6]),other=buffer(t,d,{bytes:8192});const area=lock(t,other,0n,8192n);unlock(t,other,area);play(t,b);play(t,other);t.now(50_000n);frame(t,method(t,b,9),[b,0x20700n]);const page=(area[0]+4096n)&~4095n;assert.notEqual(page,data&~4095n);equal(core.wire_protect(t.handle,page,4096n,1),0);const before=snapshot(t);core.wire_load(t.handle,0x600000n,4,1);equal(core.wire_fault_address(t.handle),0x600000n);const result=vm.service();equal(result.reason,Exit.Fault);equal(result.audio,true);equal(result.fault.address,area[0]);equal(core.wire_fault_address(t.handle),0x600000n);equal(snapshot(t),before);equal(sink.samples.length,0);equal(vm.audioStats().mixedFrames,0n);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(sink.samples.length,1);equal(Array.from(sink.samples[0].subarray(0,12)),[1,1,2,2,3,3,4,4,5,5,6,6]);equal(vm.audioStats().mixedFrames,220n);vm.dispose();
}
// Method source/output/return faults preserve references, locks, all format outputs and the saved return.
for(const kind of['query','lock','format','notify','frame']){
 const t=await setup(),{vm}=t,d=device(t),b=buffer(t,d);vm.write(0x24ffen,new Uint8Array(32).fill(0xa5));vm.write(0x20500n,bufferIID);let p=b,slot,args,page=0x25000n,protect=2;
 if(kind==='query'){slot=0;args=[0x20500n,0x24ffen];}
 if(kind==='lock'){slot=11;args=[0n,8n,0x20700n,0x24ffen,0n,0n,0n];}
 if(kind==='format'){slot=5;args=[0x20700n,18n,0x24ffen];}
 if(kind==='notify'){vm.write(0x20500n,notifyIID);invoke(t,b,0,[0x20500n,0x20108n]);p=vm.readInt(0x20108n);slot=3;args=[1n,0x25000n];protect=1;}
 if(kind==='frame'){slot=1;args=[];page=t.sp&~4095n;protect=1;}
 frame(t,method(t,p,slot),[p,...args]);equal(core.wire_protect(t.handle,page,4096n,protect),0);const before=snapshot(t),out=vm.readInt(0x20700n);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x20700n),out);equal(vm.readInt(0x24ffen,1),0xa5n);equal(core.wire_protect(t.handle,page,4096n,4),0);
 if(kind==='notify'){const event=kernel(t,'CreateEventW',[0n,0n,0n,0n]);vm.write(0x25000n,[0xffffffffn,event].flatMap(n=>Array.from(encode(n,4))));frame(t,method(t,p,slot),[p,...args]);}
 equal(vm.service().reason,Exit.Dispatch);if(kind==='query'||kind==='frame')equal(invoke(t,b,2),1n);if(kind==='lock')equal(unlock(t,b,[vm.readInt(0x20700n),8n,0n,0n]),0n);if(kind==='notify')invoke(t,p,2);invoke(t,b,2);invoke(t,d,2);vm.dispose();
}
// Static PE imports and generated x86 COM calls use the actual gateway, not the test service path.
{
 const f=fixture(32,{tls:false});f.rtext(0x20c0,'dsound.dll\0');f.rtext(0x20e2,'DirectSoundCreate\0');f.relocations.splice(0);f.bytes.set([0x6a,0,0x68,0,0,0,0,0x6a,0,0xff,0x15,0,0,0,0,0xa1,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1003,f.preferred+0x3000n);f.absolute(0x100b,f.preferred+0x20a0n);f.absolute(0x1010,f.preferred+0x3000n);f.rebuildRelocations();const vm=new WasmMachine(core,{mode:32,clock:()=>0n}),sink=new Capture(),image=vm.load(f.bytes);await vm.enableAudio({sink});equal((await vm.run(100n)).reason,Exit.Returned);const p=vm.register(0),t={vm,sp:image.sp-2048n};assert.ok(p);vm.map(0x30000n,4096n,7);vm.write(0x30000n,[0x68,...encode(p,4),0xb8,...encode(method(t,p,1),4),0xff,0xd0,0xc3]);frame(t,0x30000n);equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),2n);invoke(t,p,2);invoke(t,p,2);vm.dispose();equal(sink.closes,1);
}
// Real Web Audio conversion/scheduling gets a bounded, immutable stereo packet and closes its nodes.
{
 const nodes=[],buffers=[];const context={currentTime:0,state:'running',destination:{},createBuffer(c,n,rate){equal(c,2);equal(rate,44100);const channels=[new Float32Array(n),new Float32Array(n)],b={getChannelData:i=>channels[i]};buffers.push(channels);return b;},createBufferSource(){const node={connect(){},disconnect(){this.disconnected=true;},start(t){this.startTime=t;},stop(){this.stopped=true;}};nodes.push(node);return node;}};
 const sink=await createAudioSink({context});equal(sink.write(Int16Array.from([32767,-32768,16384,-16384])),true);equal(Array.from(buffers[0][0]),[32767/32768,0.5]);equal(Array.from(buffers[0][1]),[-1,-0.5]);equal(nodes[0].startTime,0.01);context.currentTime=1;equal(sink.write(new Int16Array(88200)),true);equal(nodes[0].disconnected,true);equal(sink.write(new Int16Array(88200)),false);sink.close();equal(nodes[1].stopped,true);assert.throws(()=>sink.write(new Int16Array(2)),/closed/);sink.close();
}
// Child outputs are independent, lazy and retryable; parent retirement keeps their configuration.
for(const name of[null,'DllCanUnloadNow','DirectSoundCreate','DirectSoundCreate8']){
 const t=await setup(false),{vm}=t,sinks=new Map(),requests=[];let fail=false;
 await vm.enableAudio({async createSink(process){requests.push(process);if(fail){fail=false;throw Error('child audio setup failed');}const sink=new Capture();sinks.set(process,sink);return sink;}});
 const f=fixture(32,{tls:false});f.relocations.splice(0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));
 if(name){
  f.rtext(0x20c0,'dsound.dll\0');f.rtext(0x20e2,name+'\0');
  if(name.startsWith('DirectSound')){f.bytes.set([0x6a,0,0x68,0,0,0,0,0x6a,0,0xff,0x15,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1003,f.preferred+0x3000n);f.absolute(0x100b,f.preferred+0x20a0n);}
  else{f.bytes.set([0xff,0x15,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1002,f.preferred+0x20a0n);}
 }else{f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.set([0x31,0xc0,0xc3],f.offset(0x1000));}
 f.rebuildRelocations();vm.supplyFile('C:\\child.exe',f.bytes);vm.write(0x23000n,Uint8Array.from([...'C:\\child.exe\0'].flatMap(c=>[c.charCodeAt(0),0])));vm.write(0x23400n,new Uint8Array(68));vm.write(0x23400n,encode(68n,4));
 equal(kernel(t,'CreateProcessW',[0x23000n,0n,0n,0n,0n,0n,0n,0n,0x23400n,0x23800n]),1n);equal(requests,[1]);
 frame(t,vm.bind('kernel32.dll','TerminateProcess'),[0xffffffffn,0n]);equal(vm.service().reason,Exit.ProcessExited);equal(sinks.get(1).closes,1);
 const sound=!!name?.startsWith('DirectSound');
 if(sound){fail=true;await assert.rejects(vm.run(1000n),e=>e.processId===2&&/child audio setup failed/.test(e.message));equal(requests,[1,2]);equal(vm.processes()[1].exitCode,null);}
 equal((await vm.run(1000n)).reason,Exit.ProcessExited);equal(vm.processes()[1].exitCode,name==='DllCanUnloadNow'?0x80004001:0);equal(requests,sound?[1,2,2]:[1]);
 if(sound)equal(sinks.get(2).closes,1);vm.dispose();equal(sinks.get(1).closes,1);if(sound)equal(sinks.get(2).closes,1);
}
// Exit and startup rejection retire the Rust owner before closing the host output.
{
 const t=await setup(),d=device(t),b=buffer(t,d),data=fill(t,b,[1,2,3,4]);play(t,b);frame(t,t.vm.bind('kernel32.dll','ExitProcess'),[0n]);const exit=t.vm.service();if(exit.reason!==Exit.ProcessExited)equal((await t.vm.run(100n)).reason,Exit.ProcessExited);equal(t.sink.closes,1);equal(t.vm.audioStats(),null);t.vm.dispose();equal(t.sink.closes,1);
 const v=new WasmMachine(core,{mode:32,clock:()=>0n}),sink=new Capture(),bad=moduleFixture(32,{dll:true,attach:false,tls:false});v.supplyModule('bad.dll',bad.bytes);const image=v.load(moduleFixture(32,{dependency:'bad.dll',tls:false}).bytes);v.map(0x20000n,0x10000n,7);await v.enableAudio({sink});const u={vm:v,mode:32,sp:image.sp-2048n},ip=v.ip,regs=Array.from({length:8},(_,i)=>v.register(i)),dev=device(u),buf=buffer(u,dev),owned=fill(u,buf,[1,2,3,4]);v.ip=ip;regs.forEach((n,i)=>v.register(i,n));await assert.rejects(v.run(10000n),/attach returned FALSE/);assert.throws(()=>v.readInt(owned),/memory access/);equal(sink.closes,1);v.dispose();
}
console.log(`${checks} DirectSound COM/PCM/audio consumer/JIT/notification/fault/lifetime checks passed`);
