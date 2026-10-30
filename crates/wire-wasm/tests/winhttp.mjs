import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const names=['WinHttpGetIEProxyConfigForCurrentUser','WinHttpGetDefaultProxyConfiguration','WinHttpGetProxyForUrl','WinHttpOpen','WinHttpCloseHandle'];
const api=(t,name,args=[])=>call(t,name,args,'winhttp.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x23000n,size=mode===32?12:24;
 for(const name of names){assert.ok(vm.bind('winhttp.dll',name));assert.throws(()=>vm.bind('kernel32.dll',name),/unknown/);}
 vm.flags=0x8d5n;vm.write(t.error,encode(123n,4));vm.write(out,new Uint8Array(40).fill(0xa5));equal(api(t,names[1],[out]),1n);equal(vm.readInt(out),1n);equal(bytes(vm,out+4n,size-4),new Uint8Array(size-4));equal(vm.readInt(out+BigInt(size),1),0xa5n);equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);
 vm.write(out,new Uint8Array(40).fill(0xa5));equal(api(t,names[0],[out]),0n);equal(vm.readInt(t.error),2n);equal(bytes(vm,out,40),new Uint8Array(40).fill(0xa5));
 for(const [name,args,error] of [[names[0],[0n],87n],[names[1],[0n],87n],[names[2],[1n,1n,1n,1n],6n],[names[3],[1n,0n,1n,1n,0n],50n],[names[4],[1n],6n]]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);}
 equal(api(t,names[1],[t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),1n);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);equal(api(t,names[1],[high]),1n);equal(vm.readInt(high),1n);}
 for(const name of names.slice(0,2)){
  const p=0x24ffen;vm.write(p,new Uint8Array(40).fill(0xa5));frame(t,name,[p],'winhttp.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),name===names[1]?1n:0n);
 }
 frame(t,names[3],[1n,0n,1n,1n,0n],'winhttp.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),50n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} WinHTTP proxy-profile/absent-transport/ABI/fault checks passed`);
