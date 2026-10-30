import assert from 'node:assert/strict';
import {program} from './host-fixture.mjs';
import {fixture} from './pe-fixture.mjs';
import {machine,window,call,frame,invoke,Exit,encode,equal,checks,color} from './gdi-harness.mjs';


for(const mode of[32,64])for(const childMode of[32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);await invoke(t,'ShowWindow',[h,1n]);await invoke(t,'UpdateWindow',[h]);
 vm.supplyFile('C:\\child.exe',program(childMode));vm.write(0x23000n,Uint8Array.from([...'C:\\child.exe\0'].flatMap(c=>[c.charCodeAt(0),0])));vm.write(0x23400n,new Uint8Array(104));vm.write(0x23400n,encode(BigInt(mode===32?68:104),4));
 equal(call(t,'CreateProcessW',[0x23000n,0n,0n,0n,0n,0n,0n,0n,0x23400n,0x23800n],'kernel32.dll'),1n);const child=Number(vm.readInt(0x23800n+BigInt(mode===32?8:16))),process=vm.readInt(0x23800n,mode/8);
 vm.writeInput([0x5a],{end:true},child);frame(t,'WaitForSingleObject',[process,0xffffffffn],'kernel32.dll');equal((await vm.run(5000n)).reason,Exit.Waiting);
 const own=vm.windows(),children=vm.windows(child);equal(own.length,1);equal(children.length,1);if(mode===childMode)equal(own[0].handle,children[0].handle);else assert.notEqual(own[0].handle,children[0].handle);const ch=children[0].handle;
 equal(vm.windowOrigin(h),[10,20]);equal(vm.windowOrigin(ch,child),[10,20]);equal(vm.windowOrigin(ch+(1n<<32n),child),null);equal(color(vm.windowPixels(ch,child),0,0),[255,255,255,255]);
 equal([...vm.readOutput(1,64,child)],[0x5a]);equal([...vm.readOutput()],[]);
 vm.queueWindowEvent({window:h,message:0x8001,wparam:11n,time:1});vm.queueWindowEvent({window:ch,message:0x8001,wparam:73n,time:1},child);
 const running=vm.run(5000n);for(const query of[()=>vm.windows(child),()=>vm.windowPixels(ch,child),()=>vm.windowOrigin(ch,child),()=>vm.readOutput(1,64,child),()=>vm.writeInput([],{},child),()=>vm.queueWindowEvents([],child)])assert.throws(query,/already running/);
 await running;equal(vm.processes().find(p=>p.id===child).exitCode,73);equal(vm.windows(child),[]);equal(vm.windowPixels(ch,child),null);equal(vm.windowOrigin(ch,child),null);
 for(const pid of[0,-1,1.5,0x100000000,999])for(const query of[()=>vm.windows(pid),()=>vm.windowPixels(h,pid),()=>vm.windowOrigin(h,pid),()=>vm.readOutput(1,64,pid),()=>vm.writeInput([],{},pid),()=>vm.queueWindowEvents([],pid)])assert.throws(query,/process identity/);
 vm.dispose();assert.throws(()=>vm.windows(child),/disposed/);
}
// A retired parent must not expose child host I/O while the family awaits an output factory.
{
 const t=await machine(32),{vm}=t;let resume,started;
 const gate=new Promise(resolve=>{resume=resolve;}),entered=new Promise(resolve=>{started=resolve;});
 await vm.enableAudio({async createSink(pid){if(pid===2){started();await gate;}return{write(){return true;},close(){}};}});
 const f=fixture(32,{tls:false});f.relocations.splice(0);f.rtext(0x20c0,'dsound.dll\0');f.rtext(0x20e2,'DirectSoundCreate\0');f.bytes.set([0x6a,0,0x68,0,0,0,0,0x6a,0,0xff,0x15,0,0,0,0,0xc3],f.offset(0x1000));f.absolute(0x1003,f.preferred+0x3000n);f.absolute(0x100b,f.preferred+0x20a0n);f.rebuildRelocations();vm.supplyFile('C:\\sound.exe',f.bytes);
 vm.write(0x23000n,Uint8Array.from([...'C:\\sound.exe\0'].flatMap(c=>[c.charCodeAt(0),0])));vm.write(0x23400n,new Uint8Array(68));vm.write(0x23400n,encode(68n,4));equal(call(t,'CreateProcessW',[0x23000n,0n,0n,0n,0n,0n,0n,0n,0x23400n,0x23800n],'kernel32.dll'),1n);
 frame(t,'TerminateProcess',[0xffffffffn,0n],'kernel32.dll');equal(vm.service().reason,Exit.ProcessExited);
 const running=vm.run(1000n);await entered;
 for(const query of[()=>vm.windows(2),()=>vm.windowPixels(1n,2),()=>vm.windowOrigin(1n,2),()=>vm.readOutput(1,64,2),()=>vm.writeInput([],{},2),()=>vm.queueWindowEvents([],2),()=>vm.service(),()=>vm.supplyDirectory('C:\\pending')])assert.throws(query,/already running/);
 resume();equal((await running).reason,Exit.ProcessExited);equal(vm.processes()[1].exitCode,0);vm.dispose();
}
console.log(`${checks} process-scoped windows/pixels/origins/input/stdio/isolation checks passed`);
