import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=(1n<<BigInt(mode))-1n,read=api(t,'OpenProcess',[0x10n,0n,1n]),write=api(t,'OpenProcess',[0x28n,0n,1n]),limited=api(t,'OpenProcess',[0x20n,0n,1n]);
 vm.write(0x22000n,[1,2,3,4]);vm.write(t.error,encode(123n,4));equal(api(t,'ReadProcessMemory',[read,0x22000n,0x23000n,4n,0x24000n]),1n);equal(vm.readInt(0x23000n),0x04030201n);equal(vm.readInt(0x24000n,mode/8),4n);equal(vm.readInt(t.error),123n);
 vm.write(0x23000n,[8,7,6,5]);equal(api(t,'WriteProcessMemory',[write,0x22000n,0x23000n,4n,0n]),1n);equal(vm.readInt(0x22000n),0x05060708n);equal(api(t,'WriteProcessMemory',[limited,0x22000n,0xdead0000n,4n,0x24000n]),0n);equal(vm.readInt(t.error),5n);equal(vm.readInt(0x24000n,mode/8),0n);
 for(const name of ['ReadProcessMemory','WriteProcessMemory']){equal(api(t,name,[0n,0n,0n,1n,0x24000n]),0n);equal(vm.readInt(t.error),6n);equal(api(t,name,[current,0xdead0000n,0xdead1000n,0n,0x24000n]),1n);equal(vm.readInt(0x24000n,mode/8),0n);}
 for(const name of ['ReadProcessMemory','WriteProcessMemory']){
  vm.write(0x24ffen,[1,2,3,4]);vm.write(0x23000n,[0xa5,0xa5,0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(api(t,name,[current,0x24ffen,0x23000n,4n,0x24000n]),0n);equal(vm.readInt(t.error),299n);equal(vm.readInt(0x24000n,mode/8),0n);equal(vm.readInt(0x23000n),0xa5a5a5a5n);equal(vm.readInt(0x24ffen,2),0x0201n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 }
 equal(core.wire_protect(t.handle,0x25000n,4096n,0x104),0);equal(api(t,'ReadProcessMemory',[current,0x25000n,0x23000n,1n,0n]),0n);equal(vm.readInt(t.error),299n);frame(t,'ReadProcessMemory',[current,0x22000n,0x25000n,1n,0n],'kernel32.dll');let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.service().reason,Exit.Dispatch);
 vm.write(0x22000n,[1,2,3,4]);vm.write(0x23000n,[0xa5,0xa5,0xa5,0xa5]);frame(t,'ReadProcessMemory',[current,0x22000n,0x23000n,4n,0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x23000n),0xa5a5a5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x23000n),0x04030201n);
 vm.write(0x24000n,encode(77n,mode/8));frame(t,'ReadProcessMemory',[current,0xdead0000n,0x23000n,4n,0x24000n],'kernel32.dll');equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24000n,mode/8),77n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24000n,mode/8),0n);
 for(const name of ['ReadProcessMemory','WriteProcessMemory'])for(const [source,destination] of [[0,3],[3,0],[0,0]]){
  const expected=Uint8Array.from({length:10032},(_,i)=>(i*29+17)&255);vm.write(0x30000n,expected);expected.copyWithin(destination,source,source+10003);
  const args=name==='ReadProcessMemory'?[current,0x30000n+BigInt(source),0x30000n+BigInt(destination),10003n,0n]:[current,0x30000n+BigInt(destination),0x30000n+BigInt(source),10003n,0n];equal(api(t,name,args),1n);equal(bytes(vm,0x30000n,expected.length),expected);
 }
 vm.write(0x22000n,[1,2,3,4,5,6,7,8]);equal(api(t,'ReadProcessMemory',[current,0x22000n,0x23000n,8n,0x22000n]),1n);equal(vm.readInt(0x23000n,8),0x0807060504030201n);equal(vm.readInt(0x22000n,mode/8),8n);
 equal(api(t,'ReadProcessMemory',[current,0x23000n,t.sp,8n,0n]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,8),0x0807060504030201n);
 if(mode===64){equal(api(t,'ReadProcessMemory',[read|(1n<<32n),0n,0n,0n,0n]),0n);equal(vm.readInt(t.error),6n);equal(api(t,'ReadProcessMemory',[current,0x22000n,0x23000n,(1n<<32n)+1n,0x24000n]),0n);equal(vm.readInt(t.error),299n);const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(api(t,'WriteProcessMemory',[write,high,0x23000n,8n,0x24000n]),1n);equal(vm.readInt(high,8),0x0807060504030201n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 for(const name of ['ReadProcessMemory','WriteProcessMemory'])equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));for(const h of [read,write,limited])equal(api(t,'CloseHandle',[h]),1n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,current=(1n<<BigInt(mode))-1n,code=0x30000n;
 vm.write(code,[0xb8,1,0,0,0,0xc3]);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(10n)).reason,Exit.Returned);equal(vm.register(0),1n);const ticket=core.wire_ticket(t.handle);vm.write(0x22000n,[2]);equal(api(t,'WriteProcessMemory',[current,code+1n,0x22000n,1n,0n]),1n);equal(core.wire_live(t.handle,ticket),0);vm.ip=code;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(10n)).reason,Exit.Returned);equal(vm.register(0),2n);vm.dispose();
}
console.log(`${checks} process-memory/rights/overlap/atomic-copy/source-revocation/ABI checks passed`);
