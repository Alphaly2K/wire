import assert from 'node:assert/strict';
import {WasmMachine} from '../web/runtime.mjs';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
import {moduleFixture} from './module-fixture.mjs';
import {fixture} from './pe-fixture.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const writeText=(vm,p,s,wide=true)=>vm.write(p,wide?[...s+'\0'].flatMap(c=>{const b=[];for(let n=0;n<c.length;n++)b.push(...encode(BigInt(c.charCodeAt(n)),2));return b;}):new TextEncoder().encode(s+'\0'));
function text(vm,p,wide=true){let s='';for(let n=0;n<32768;n++){const c=Number(vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1));if(!c)return s;s+=String.fromCharCode(c);}throw new Error('unterminated test string');}
function block(vm,p,wide=true){const result=[];for(let n=0;n<2048;n++){const s=text(vm,p,wide);if(!s)return result;p+=BigInt((s.length+1)*(wide?2:1));result.push(s);}throw new Error('unterminated test block');}
function mirror(t){const{vm,mode,image}=t,p=mode/8,parameter=vm.readInt(image.peb+BigInt(mode===32?0x10:0x20),p);return{parameter,pointer:parameter+BigInt(mode===32?0x48:0x80),size:parameter+BigInt(mode===32?0x290:0x3f0)};}

// Staged startup allocation must also avoid pages already owned by the caller.
for(const mode of [32,64]){
 const vm=new WasmMachine(core,{mode});try{vm.map(0x10000n,4096n);vm.write(0x10000n,encode(0x12345678n,4));const image=vm.load(fixture(mode,{tls:false}).bytes);equal(vm.readInt(0x10000n),0x12345678n);equal(block(vm,vm.readInt(mirror({vm,mode,image}).pointer,mode/8)).includes('SystemRoot=C:\\Windows'),true);}finally{vm.dispose();}
}

// API values and the PEB share one model; issued blocks remain separate snapshots.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,m=mirror(t),initial=vm.readInt(m.pointer,p),variable=0x22000n,value=0x23000n,out=0x24000n;
 equal(block(vm,initial).includes('SystemRoot=C:\\Windows'),true);equal(vm.readInt(m.size,p),BigInt(block(vm,initial).reduce((n,s)=>n+s.length+1,1)*2));
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0xa5));const fpu=fp(t);vm.write(t.error,encode(123n,4));
 writeText(vm,variable,'SystemRoot');equal(api(t,'GetEnvironmentVariableW',[variable,0xdead0000n,0n]),11n);equal(api(t,'GetEnvironmentVariableW',[variable,out,11n]),10n);equal(text(vm,out),'C:\\Windows');equal(vm.readInt(t.error),123n);
 writeText(vm,variable,'WireTest');writeText(vm,value,'あいう');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);const current=vm.readInt(m.pointer,p);assert.notEqual(current,initial);assert.throws(()=>vm.readInt(initial),/memory access/);equal(block(vm,current).includes('WireTest=あいう'),true);
 const wide=api(t,'GetEnvironmentStringsW'),ansi=api(t,'GetEnvironmentStringsA'),plain=api(t,'GetEnvironmentStrings');equal(block(vm,wide).includes('WireTest=あいう'),true);
 writeText(vm,variable,'wIREtEST',false);equal(api(t,'GetEnvironmentVariableA',[variable,out,6n]),7n);equal(api(t,'GetEnvironmentVariableA',[variable,out,7n]),6n);equal(Array.from({length:7},(_,i)=>Number(vm.readInt(out+BigInt(i),1))),[0x82,0xa0,0x82,0xa2,0x82,0xa4,0]);
 writeText(vm,variable,'wiretest');writeText(vm,value,'changed');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);equal(block(vm,wide).includes('WireTest=あいう'),true);equal(block(vm,vm.readInt(m.pointer,p)).includes('wiretest=changed'),true);
 equal(api(t,'FreeEnvironmentStringsA',[wide]),0n);equal(vm.readInt(t.error),87n);equal(block(vm,wide).includes('WireTest=あいう'),true);
 const heap=api(t,'GetProcessHeap');equal(api(t,'HeapFree',[heap,0n,wide]),0n);equal(api(t,'VirtualFree',[wide,0n,0x8000n]),0n);equal(api(t,'FreeEnvironmentStringsW',[wide]),1n);assert.throws(()=>vm.readInt(wide),/memory access/);equal(api(t,'FreeEnvironmentStringsA',[ansi]),1n);equal(api(t,'FreeEnvironmentStringsA',[plain]),1n);equal(api(t,'FreeEnvironmentStringsW',[wide]),0n);
 writeText(vm,value,'');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);vm.write(out,[0xa5,0xa5]);equal(api(t,'GetEnvironmentVariableW',[variable,out,1n]),0n);equal(vm.readInt(out,2),0n);
 equal(api(t,'SetEnvironmentVariableW',[variable,0n]),1n);vm.write(out,[0xa5,0xa5]);equal(api(t,'GetEnvironmentVariableW',[variable,out,100n]),0n);equal(vm.readInt(t.error),203n);equal(vm.readInt(out,2),0xa5a5n);equal(api(t,'SetEnvironmentVariableW',[variable,0n]),0n);equal(vm.readInt(t.error),203n);
 equal(api(t,'GetEnvironmentVariableW',[0n,out,100n]),0n);equal(vm.readInt(t.error),87n);writeText(vm,variable,'a=b');equal(api(t,'SetEnvironmentVariableW',[variable,value]),0n);equal(vm.readInt(t.error),87n);
 equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Mirror/output faults publish no model or allocation; retry preserves return aliases.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode/8,m=mirror(t),variable=0x22000n,value=0x23000n,out=0x24ffen;
 writeText(vm,variable,'FaultTest');writeText(vm,value,'old');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);const current=vm.readInt(m.pointer,p);
 vm.write(0x26000n,encode(64n,4));api(t,'GlobalMemoryStatusEx',[0x26000n]);const available=vm.readInt(0x26010n,8);
 writeText(vm,value,'new');frame(t,'SetEnvironmentVariableW',[variable,value],'kernel32.dll');equal(core.wire_protect(t.handle,m.parameter,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(m.pointer,p),current);equal(block(vm,current).includes('FaultTest=old'),true);
 equal(core.wire_protect(t.handle,m.parameter,4096n,4),0);equal(api(t,'GlobalMemoryStatusEx',[0x26000n]),1n);equal(vm.readInt(0x26010n,8),available);equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);equal(block(vm,vm.readInt(m.pointer,p)).includes('FaultTest=new'),true);
 vm.write(out,[0xa5,0xa5]);frame(t,'GetEnvironmentVariableW',[variable,out,100n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(text(vm,out),'new');
 equal(api(t,'GetEnvironmentVariableW',[variable,t.sp,100n]),3n);equal(vm.ip,vm.entryPoints.returned);equal(text(vm,t.sp),'new');
 const issued=api(t,'GetEnvironmentStringsW');frame(t,'FreeEnvironmentStringsA',[issued],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(block(vm,issued).includes('FaultTest=new'),true);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(api(t,'FreeEnvironmentStringsW',[issued]),1n);equal(t.time.calls,0);vm.dispose();
}

// Static startup rejection retires both the PEB block and an issued DLL snapshot.
for(const mode of [32,64]){
 const f=moduleFixture(mode,{dll:true,dependency:'kernel32.dll',tls:false});f.rtext(0x20e2,'GetEnvironmentStringsW\0');f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r<0x1000||r>=0x1100));
 const code=mode===32?[0x83,0x7c,0x24,8,1,0x75,0]:[0x83,0xfa,1,0x75,0],jump=code.length-1;
 if(mode===32){code.push(0xff,0x15);f.relocations.push(0x1000+code.length);code.push(...encode(f.preferred+0x20a0n,4),0xa3,...encode(0x20000n,4));}
 else{code.push(0x48,0x83,0xec,40,0xff,0x15);code.push(...encode(BigInt(0x20a0-0x1000-code.length-4),4),0x48,0xa3,...encode(0x20000n,8),0x48,0x83,0xc4,40);}
 code.push(0x31,0xc0,...(mode===32?[0xc2,12,0]:[0xc3]));code[jump]=code.length-jump-1;code.push(0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3]));f.bytes.set(code,f.offset(0x1000));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode});vm.map(0x20000n,4096n);vm.supplyModule('bad.dll',f.bytes);const image=vm.load(moduleFixture(mode,{dependency:'bad.dll',tls:false}).bytes),initial=vm.readInt(mirror({vm,mode,image}).pointer,mode/8);
 await assert.rejects(vm.run(10000n),/attach returned FALSE/);const issued=vm.readInt(0x20000n,mode/8);assert.ok(issued);assert.throws(()=>vm.readInt(issued),/memory access/);assert.throws(()=>vm.readInt(initial),/memory access/);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;for(const name of ['GetEnvironmentStrings','GetEnvironmentStringsA','GetEnvironmentStringsW','FreeEnvironmentStringsA','FreeEnvironmentStringsW','GetEnvironmentVariableA','GetEnvironmentVariableW','SetEnvironmentVariableA','SetEnvironmentVariableW']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}vm.dispose();
}
console.log(`${checks} environment model/PEB/snapshot/encoding/fault/cleanup checks passed`);
