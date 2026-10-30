import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const writeText=(vm,p,s)=>vm.write(p,[...s+'\0'].flatMap(c=>Array.from(encode(BigInt(c.charCodeAt(0)),2))));
const encoded=(s,wide)=>wide?[...s+'\0'].flatMap(c=>Array.from(encode(BigInt(c.charCodeAt(0)),2))):Array.from(new TextEncoder().encode(s+'\0'));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x24000n;
 for(const base of ['GetSystemDirectory','GetWindowsDirectory','GetSystemWindowsDirectory','GetTempPath','GetTempPath2'])for(const suffix of ['A','W']){
  const name=base+suffix,wide=suffix==='W',temporary=base.startsWith('GetTemp'),expected=temporary?'C:\\Temp\\':base==='GetSystemDirectory'?'C:\\Windows\\System32':'C:\\Windows',data=encoded(expected,wide),required=BigInt(expected.length+1),args=(pointer,capacity)=>temporary?[capacity,pointer]:[pointer,capacity];
  vm.write(t.error,encode(123n,4));vm.write(out,new Uint8Array(data.length).fill(0xa5));equal(api(t,name,args(0xdead0000n,0n)),required);equal(api(t,name,args(out,required-1n)),required);equal(bytes(vm,out,data.length),new Array(data.length).fill(0xa5));equal(api(t,name,args(out,required)),required-1n);equal(bytes(vm,out,data.length),data);equal(vm.readInt(t.error),123n);
  if(mode===64){equal(api(t,name,args(0xdead0000n,0x100000000n)),required);const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(api(t,name,args(high,required)),required-1n);equal(bytes(vm,high,data.length),data);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
  const split=0x24ffen;vm.write(split,[0xa5,0xa5]);frame(t,name,args(split,required),'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,split,2),[0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(bytes(vm,split,data.length),data);
 }
 // Temp selection follows guest variables and lexical VFS resolution, without a directory access test.
 const variable=0x22000n,value=0x23000n;writeText(vm,variable,'TMP');writeText(vm,value,'relative\\cache');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);equal(api(t,'GetTempPathW',[100n,out]),18n);equal(bytes(vm,out,38),encoded('C:\\relative\\cache\\',true));
 writeText(vm,value,'');equal(api(t,'SetEnvironmentVariableW',[variable,value]),1n);equal(api(t,'GetTempPathA',[100n,out]),8n);equal(bytes(vm,out,9),encoded('C:\\Temp\\',false));
 writeText(vm,variable,'TEMP');equal(api(t,'SetEnvironmentVariableW',[variable,0n]),1n);equal(api(t,'GetTempPath2W',[100n,out]),14n);equal(bytes(vm,out,30),encoded('C:\\Users\\wire\\',true));writeText(vm,variable,'USERPROFILE');equal(api(t,'SetEnvironmentVariableW',[variable,0n]),1n);equal(api(t,'GetTempPath2A',[100n,out]),11n);equal(bytes(vm,out,12),encoded('C:\\Windows\\',false));
 equal(api(t,'GetWindowsDirectoryW',[t.sp,100n]),10n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,22),encoded('C:\\Windows',true));
 for(const base of ['GetSystemDirectory','GetWindowsDirectory','GetSystemWindowsDirectory','GetTempPath','GetTempPath2'])for(const suffix of ['A','W']){const name=base+suffix;equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} system/temp path ABI/capacity/selection/alias/fault checks passed`);
