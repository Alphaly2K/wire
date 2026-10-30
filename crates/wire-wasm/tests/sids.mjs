import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'advapi32.dll');
const source=0x21000n,dest=0x23000n,authority=0x20000n;
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const sid=[1,2,0,0,0,0,0,5,21,0,0,0,42,0,0,0];

// Field views are address calculations; validation reads only the header and preserves LastError.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 for(const [count,length] of [[0n,8n],[1n,12n],[15n,68n],[16n,72n],[255n,1028n],[0x100000101n,12n]])equal(api(t,'GetSidLengthRequired',[count]),length);
 for(const p of [0n,source,...(mode===64?[0x123400000000n]:[])]){
  equal(api(t,'GetSidIdentifierAuthority',[p]),BigInt.asUintN(mode,p+2n));equal(api(t,'GetSidSubAuthorityCount',[p]),BigInt.asUintN(mode,p+1n));
  equal(api(t,'GetSidSubAuthority',[p,0x100000003n]),BigInt.asUintN(mode,p+20n));
 }
 vm.write(0x24ffen,[1,15]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);
 equal(api(t,'IsValidSid',[0x24ffen]),1n);equal(api(t,'GetLengthSid',[0x24ffen]),68n);
 for(const p of [0n,0xdead0000n])equal(api(t,'IsValidSid',[p]),0n);
 for(const header of [[0,0],[2,1],[1,16],[1,255]]){vm.write(source,header);equal(api(t,'IsValidSid',[source]),0n);equal(api(t,'EqualSid',[source,0x25000n]),0n);}
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 vm.write(source,sid);vm.write(dest,sid);equal(api(t,'GetLengthSid',[source]),16n);equal(api(t,'EqualSid',[source,dest]),1n);
 vm.write(dest+12n,encode(43n,4));equal(api(t,'EqualSid',[source,dest]),0n);
 vm.write(authority,[0,0,0,0,0,16]);vm.write(dest,new Uint8Array(68).fill(0xa5));
 equal(api(t,'InitializeSid',[dest,authority,0x10000000fn]),1n);equal(bytes(vm,dest,68),Uint8Array.from([1,15,0,0,0,0,0,16,...new Uint8Array(60)]));
 equal(api(t,'InitializeSid',[0xdead0000n,0xdead0000n,16n]),0n);equal(vm.readInt(t.error),87n);vm.write(t.error,encode(123n,4));
 vm.write(dest,new Uint8Array(16).fill(0xa5));equal(api(t,'InitializeSid',[dest,0n,2n]),1n);equal(bytes(vm,dest,16),Uint8Array.from([1,2,...new Uint8Array(6).fill(0xa5),...new Uint8Array(8)]));
 vm.write(dest,new Uint8Array(16).fill(0xa5));equal(api(t,'CopySid',[15n,dest,source]),0n);equal(vm.readInt(t.error),122n);equal(bytes(vm,dest,16),new Uint8Array(16).fill(0xa5));vm.write(t.error,encode(123n,4));
 equal(api(t,'CopySid',[16n,dest,source]),1n);equal(bytes(vm,dest,16),Uint8Array.from(sid));
 equal(api(t,'CopySid',[16n,source+1n,source]),1n);equal(bytes(vm,source+1n,16),Uint8Array.from(sid));
 equal(vm.flags,0x8d5n);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);
 for(const name of ['GetSidSubAuthority','GetSidSubAuthorityCount','GetSidIdentifierAuthority','IsValidSid','GetLengthSid','CopySid','EqualSid','GetSidLengthRequired','InitializeSid']){
  assert.ok(vm.bind('advapi32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 }
 vm.dispose();
}

// Source, output, guard, error-slot and first-frame failures leave the complete call retryable.
for(const mode of [32,64])for(const kind of ['source','dest','guard','initialize','authority','error','return']){
 const t=await machine(mode),{vm}=t;vm.write(source,sid);vm.write(authority,[0,0,0,0,0,5]);vm.write(0x24ffcn,new Uint8Array(68).fill(0xa5));vm.write(t.error,encode(123n,4));
 let page=0x25000n,protection=2;
 if(kind==='source'){page=source;protection=1;}
 if(kind==='authority'){page=authority;protection=1;}
 if(kind==='guard'){page=source;protection=0x104;}
 if(kind==='error')page=t.image.teb;
 if(kind==='return'){page=t.sp&~4095n;protection=1;}
 const init=kind==='initialize'||kind==='authority';
 frame(t,init?'InitializeSid':'CopySid',init?[0x24ffcn,authority,2n]:[kind==='error'?15n:16n,0x24ffcn,source],'advapi32.dll');
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),prefix=bytes(vm,0x24ffcn,4),work=vm.work;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(vm,0x24ffcn,4),prefix);equal(vm.readInt(t.error),123n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='error'?0n:1n);
 if(init)equal(bytes(vm,0x24ffcn,16),Uint8Array.from([1,2,0,0,0,0,0,5,...new Uint8Array(8)]));else if(kind!=='error')equal(bytes(vm,0x24ffcn,16),Uint8Array.from(sid));
 vm.dispose();
}

// A false first operand short-circuits; a real guard is delivered once rather than swallowed.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(source,sid);vm.write(dest,[0,1]);
 vm.write(0x24fffn,[0]);equal(core.wire_protect(t.handle,0x25000n,4096n,0x104),0);equal(api(t,'IsValidSid',[0x24fffn]),0n);
 frame(t,'IsValidSid',[0x25000n],'advapi32.dll');equal(vm.service().reason,Exit.Fault);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(core.wire_protect(t.handle,source,4096n,0x104),0);equal(api(t,'EqualSid',[dest,source]),0n);
 frame(t,'IsValidSid',[source],'advapi32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 vm.write(t.sp,new Uint8Array(16).fill(0xa5));frame(t,'CopySid',[16n,t.sp,source],'advapi32.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,16),Uint8Array.from(sid));
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);vm.write(high,sid);equal(api(t,'GetSidSubAuthority',[high,1n]),high+12n);equal(api(t,'CopySid',[16n,dest,high]),1n);equal(bytes(vm,dest,16),Uint8Array.from(sid));}
 vm.dispose();
}
console.log(`${checks} SID ABI/layout/field/initialization/copy/alias/fault checks passed`);
