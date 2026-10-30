import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish,window,invoke} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const groups=[
 ['advapi32.dll',['AllocateAndInitializeSid','FreeSid','BuildTrusteeWithSidW','LookupAccountSidW','DuplicateToken','ImpersonateLoggedOnUser','RevertToSelf','GetEffectiveRightsFromAclW','GetNamedSecurityInfoW','SetNamedSecurityInfoW','SetEntriesInAclW','CreateProcessWithLogonW','SystemFunction036']],
 ['kernel32.dll',['CancelSynchronousIo','FlushProcessWriteBuffers','FreeConsole','GetCurrentProcessorNumberEx','GetLogicalDriveStringsA','GetLogicalDriveStringsW','GetNumberOfConsoleInputEvents','GetPriorityClass','GetProcessTimes','GetProcessWorkingSetSize','GetVolumeInformationA','GetVolumeInformationW','K32EnumProcesses','LockFile','OpenFileMappingA','OpenFileMappingW','PeekConsoleInputA','PeekConsoleInputW','ReadConsoleInputA','ReadConsoleInputW','SetConsoleMode','SetPriorityClass','SetProcessWorkingSetSize','SignalObjectAndWait','UnlockFile','VerLanguageNameA','VerLanguageNameW']],
 ['ole32.dll',['CoTaskMemRealloc']],['shell32.dll',['ShellExecuteExA','ShellExecuteExW']],
 ['user32.dll',['GetWindow','WaitForInputIdle']],['winmm.dll',['timeKillEvent','timeSetEvent']],['psapi.dll',['EnumProcesses']],['version.dll',['VerLanguageNameA','VerLanguageNameW']],
];
const dllFor=n=>groups.find(([,names])=>names.includes(n))?.[0]??'kernel32.dll';
const api=(t,n,a=[])=>call(t,n,a,dllFor(n));
const sec=(t,n,a=[])=>call(t,n,a,'advapi32.dll');
const task=(t,n,a=[])=>call(t,n,a,'ole32.dll');
const current=t=>BigInt.asUintN(t.mode,-1n),thread=t=>BigInt.asUintN(t.mode,-2n);
const out=0x23000n,data=0x24000n,used=0x22000n;
const mark=t=>t.vm.write(t.error,encode(123n,4));
const error=(t,n)=>equal(t.vm.readInt(t.error),BigInt(n));
const bytes=(t,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(t.vm.readInt(p+BigInt(i),1)));
const fill=(t,p,n)=>t.vm.write(p,new Uint8Array(n).fill(0xa5));
const text=(t,p,s,wide=true)=>t.vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const token=(t,access=14n)=>{equal(sec(t,'OpenProcessToken',[current(t),access,out]),1n);return t.vm.readInt(out,t.mode/8);};
const info=(t,h,c)=>sec(t,'GetTokenInformation',[h,BigInt(c),data,64n,used]);

// Allocation growth, failure and freeing must preserve the original COM owner.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;mark(t);
 let p=api(t,'CoTaskMemRealloc',[0n,17n]);assert.ok(p);const original=Uint8Array.from({length:17},(_,n)=>n+1);vm.write(p,original);
 equal(api(t,'CoTaskMemRealloc',[p,0x10000001n]),0n);equal(bytes(t,p,17),original);
 p=api(t,'CoTaskMemRealloc',[p,65537n]);assert.ok(p);equal(bytes(t,p,17),original);vm.write(p+65536n,[0x73]);
 p=api(t,'CoTaskMemRealloc',[p,8n]);assert.ok(p);equal(bytes(t,p,8),original.slice(0,8));
 equal(api(t,'CoTaskMemRealloc',[p,0n]),0n);frame(t,'CoTaskMemFree',[p],'ole32.dll');assert.throws(()=>vm.service(),/invalid COM task/);
 const zero=api(t,'CoTaskMemRealloc',[0n,0n]);assert.ok(zero);task(t,'CoTaskMemFree',[zero]);
 const heap=api(t,'GetProcessHeap'),ordinary=api(t,'HeapAlloc',[heap,0n,16n]);frame(t,'CoTaskMemRealloc',[ordinary,32n],'ole32.dll');assert.throws(()=>vm.service(),/invalid COM task/);equal(api(t,'HeapSize',[heap,0n,ordinary]),16n);api(t,'HeapFree',[heap,0n,ordinary]);
 p=task(t,'CoTaskMemAlloc',[16n]);vm.write(p,[0x5a]);frame(t,'CoTaskMemRealloc',[p,8192n],'ole32.dll');const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(t,p,1),new Uint8Array([0x5a]));equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const grown=vm.register(0);equal(bytes(t,grown,1),new Uint8Array([0x5a]));task(t,'CoTaskMemFree',[grown]);error(t,123);vm.dispose();
}

// SID storage is ordinary process-heap storage; TRUSTEE snapshots aliased inputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(data,[0,0,0,0,0,5]);mark(t);
 const args=[data,8n,21n,1n,2n,3n,4n,5n,6n,0xdeadbeefn,out];equal(api(t,'AllocateAndInitializeSid',args),1n);const sid=vm.readInt(out,mode/8);
 equal(sec(t,'IsValidSid',[sid]),1n);equal(sec(t,'GetLengthSid',[sid]),40n);equal(vm.readInt(sid+36n),0xdeadbeefn);
 const heap=api(t,'GetProcessHeap');equal(api(t,'HeapSize',[heap,0n,sid]),40n);
 const trustee=0x24200n;fill(t,trustee,32);equal(api(t,'BuildTrusteeWithSidW',[trustee,sid]),0n);
 equal(bytes(t,trustee,mode===32?16:20),new Uint8Array(mode===32?16:20));equal(vm.readInt(trustee+BigInt(mode===32?16:24),mode/8),sid);
 if(mode===64)equal(vm.readInt(trustee+20n),0xa5a5a5a5n);
 equal(api(t,'BuildTrusteeWithSidW',[trustee,trustee]),0n);equal(vm.readInt(trustee+BigInt(mode===32?16:24),mode/8),trustee);
 equal(api(t,'FreeSid',[sid]),0n);equal(api(t,'FreeSid',[0n]),0n);error(t,123);
 vm.write(out,encode(0xfeedn,mode/8));equal(api(t,'AllocateAndInitializeSid',[1n,9n,...Array(8).fill(0n),out]),0n);error(t,1337);equal(vm.readInt(out,mode/8),0xfeedn);vm.dispose();
}

// Token type/level, rights and impersonation survive handle closure, but stay per-thread.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=token(t);equal(info(t,h,8),1n);equal(vm.readInt(data),1n);equal(info(t,h,9),0n);error(t,87);
 for(const level of [0n,1n,2n,3n]){
  equal(api(t,'DuplicateToken',[h,level,out]),1n);const duplicate=vm.readInt(out,mode/8);equal(info(t,duplicate,8),1n);equal(vm.readInt(data),2n);equal(info(t,duplicate,9),1n);equal(vm.readInt(data),level);
  equal(api(t,'ImpersonateLoggedOnUser',[duplicate]),level<2n?0n:1n);
  if(level<2n)error(t,1346);else{
   equal(api(t,'CloseHandle',[duplicate]),1n);equal(sec(t,'OpenThreadToken',[thread(t),8n,0n,out]),1n);const active=vm.readInt(out,mode/8);equal(info(t,active,9),1n);equal(vm.readInt(data),level);api(t,'CloseHandle',[active]);equal(api(t,'RevertToSelf'),1n);continue;
  }
  api(t,'CloseHandle',[duplicate]);
 }
 const query=token(t,8n);equal(api(t,'DuplicateToken',[query,2n,out]),0n);error(t,5);equal(api(t,'ImpersonateLoggedOnUser',[query]),0n);error(t,5);
 equal(api(t,'DuplicateToken',[h,4n,out]),0n);error(t,87);equal(api(t,'ImpersonateLoggedOnUser',[h]),1n);
 // A fresh worker must have no impersonation token, despite the main thread's active token.
 const code=[],emit=(...b)=>code.push(...b),imm=(v,n)=>emit(...encode(v,n));
 if(mode===32){for(const v of [out,0n,8n,thread(t)]){emit(0x68);imm(v,4);}emit(0xb8);imm(vm.bind('advapi32.dll','OpenThreadToken'),4);emit(0xff,0xd0,0x64,0xa1,0x34,0,0,0,0xc2,4,0);}
 else{emit(0x48,0x83,0xec,40,0x48,0xb9);imm(thread(t),8);emit(0xba,8,0,0,0,0x45,0x31,0xc0,0x49,0xb9);imm(out,8);emit(0x48,0xb8);imm(vm.bind('advapi32.dll','OpenThreadToken'),8);emit(0xff,0xd0,0x65,0x8b,0x04,0x25,0x68,0,0,0,0x48,0x83,0xc4,40,0xc3);}
 vm.write(0x30000n,code);const worker=api(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n]);frame(t,'WaitForSingleObject',[worker,0xffffffffn],'kernel32.dll');equal(await finish(t),0n);equal(api(t,'GetExitCodeThread',[worker,used]),1n);equal(vm.readInt(used),1008n);
 equal(sec(t,'OpenThreadToken',[thread(t),8n,1n,out]),1n);api(t,'CloseHandle',[vm.readInt(out,mode/8)]);equal(api(t,'RevertToSelf'),1n);equal(sec(t,'OpenThreadToken',[thread(t),8n,0n,out]),0n);error(t,1008);for(const handle of [worker,h,query])api(t,'CloseHandle',[handle]);vm.dispose();
}

// Copyout failures cannot consume token identities or publish partial SID/TRUSTEE records.
for(const mode of [32,64])for(const kind of ['token','sid','trustee']){
 const t=await machine(mode),{vm}=t,h=token(t),p=0x24ffen;fill(t,p,40);vm.write(data,[0,0,0,0,0,5]);
 const n=kind==='token'?'DuplicateToken':kind==='sid'?'AllocateAndInitializeSid':'BuildTrusteeWithSidW';
 const a=kind==='token'?[h,2n,p]:kind==='sid'?[data,1n,21n,...Array(7).fill(0n),p]:[p,h];frame(t,n,a,'advapi32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t),work=vm.work;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(t,p,2),new Uint8Array(2).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 if(kind==='token'){equal(vm.readInt(p,mode/8),h+1n);api(t,'CloseHandle',[h+1n]);}else if(kind==='sid')api(t,'FreeSid',[vm.readInt(p,mode/8)]);api(t,'CloseHandle',[h]);vm.dispose();
}

// Account length queries and full output transactions share the virtual TokenUser identity.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=token(t);equal(info(t,h,1),1n);const sid=vm.readInt(data,mode/8),a=[0n,sid,0x24200n,used,0x24300n,used+4n,out];
 vm.write(used,new Uint8Array(8));fill(t,0x24200n,16);fill(t,0x24300n,16);mark(t);equal(api(t,'LookupAccountSidW',a),0n);error(t,122);equal(vm.readInt(used),5n);equal(vm.readInt(used+4n),5n);equal(bytes(t,0x24200n,16),new Uint8Array(16).fill(0xa5));
 mark(t);equal(api(t,'LookupAccountSidW',a),1n);error(t,123);equal(vm.readInt(used),4n);equal(vm.readInt(used+4n),4n);equal(vm.readInt(out),1n);equal(bytes(t,0x24200n,10),new Uint8Array(Buffer.from('wire\0','utf16le')));equal(bytes(t,0x24300n,10),new Uint8Array(Buffer.from('WIRE\0','utf16le')));
 text(t,0x24100n,'elsewhere');equal(api(t,'LookupAccountSidW',[0x24100n,...a.slice(1)]),0n);error(t,50);
 vm.write(sid+24n,encode(1001n,4));equal(api(t,'LookupAccountSidW',a),0n);error(t,1332);vm.write(sid+24n,encode(1000n,4));
 for(const kind of ['payload','length-error']){
  vm.write(used,encode(kind==='payload'?5n:0n,4));vm.write(used+4n,encode(5n,4));fill(t,0x24200n,16);const page=kind==='payload'?0x25000n:t.image.teb,args=[...a];if(kind==='payload')args[4]=0x24ffen;
  frame(t,'LookupAccountSidW',args,'advapi32.dll');equal(core.wire_protect(t.handle,page,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(used),kind==='payload'?5n:0n);equal(bytes(t,0x24200n,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 }
 api(t,'CloseHandle',[h]);vm.dispose();
}

// System/profile APIs report virtual state, while unimplemented operations preserve outputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;mark(t);equal(api(t,'FlushProcessWriteBuffers'),0n);fill(t,data,8);equal(api(t,'GetCurrentProcessorNumberEx',[data]),0n);equal(vm.readInt(data),0n);equal(vm.readInt(data+4n),0xa5a5a5a5n);
 for(const suffix of ['A','W']){
  const wide=suffix==='W',scale=wide?2:1;fill(t,data,64);equal(api(t,'GetLogicalDriveStrings'+suffix,[4n,1n]),5n);equal(api(t,'GetLogicalDriveStrings'+suffix,[5n,data]),4n);equal(bytes(t,data,scale*5),new Uint8Array(Buffer.from('C:\\\0\0',wide?'utf16le':'ascii')));
  equal(api(t,'VerLanguageName'+suffix,[0x409n,data,4n]),3n);equal(bytes(t,data,scale*4),new Uint8Array(Buffer.from('Eng\0',wide?'utf16le':'ascii')));equal(api(t,'VerLanguageName'+suffix,[0xffffn,1n,0n]),0n);
  const a=[0n,data,16n,used,used+4n,used+8n,0x24200n,16n];equal(api(t,'GetVolumeInformation'+suffix,a),1n);equal(vm.readInt(used),1n);equal(vm.readInt(used+4n),32765n);equal(vm.readInt(used+8n),6n);equal(bytes(t,data,scale*5),new Uint8Array(Buffer.from('Wire\0',wide?'utf16le':'ascii')));
  for(const [root,code]of [['C:',123],['D:\\',15],['C:\\subdir\\',144]]){text(t,0x24100n,root,wide);equal(api(t,'GetVolumeInformation'+suffix,[0x24100n,...a.slice(1)]),0n);error(t,code);}
  fill(t,data,32);equal(api(t,'GetVolumeInformation'+suffix,[0n,data,1n,...a.slice(3)]),0n);error(t,234);equal(bytes(t,data,32),new Uint8Array(32).fill(0xa5));
  text(t,0x24100n,'section',wide);equal(api(t,'OpenFileMapping'+suffix,[4n,0n,0x24100n]),0n);error(t,2);equal(api(t,'OpenFileMapping'+suffix,[4n,0n,0n]),0n);error(t,87);
 }
 const self=current(t);equal(api(t,'GetPriorityClass',[self]),0x20n);for(const priority of [0x40n,0x4000n,0x8000n,0x80n,0x100n,0x20n]){equal(api(t,'SetPriorityClass',[self,priority]),1n);equal(api(t,'GetPriorityClass',[self]),priority);}
 for(const [value,code]of [[1n,87],[0x100000n,50],[0x200000n,50]]){equal(api(t,'SetPriorityClass',[self,value]),0n);error(t,code);equal(api(t,'GetPriorityClass',[self]),0x20n);}
 const query=api(t,'OpenProcess',[0x1000n,0n,1n]);equal(api(t,'SetPriorityClass',[query,0x80n]),0n);error(t,5);api(t,'CloseHandle',[query]);
 for(const name of ['EnumProcesses','K32EnumProcesses']){fill(t,data,8);equal(api(t,name,[data,3n,used]),1n);equal(vm.readInt(used),0n);equal(vm.readInt(data),0xa5a5a5a5n);equal(api(t,name,[data,8n,used]),1n);equal(vm.readInt(used),4n);equal(vm.readInt(data),1n);equal(vm.readInt(data+4n),0xa5a5a5a5n);}
 for(const [name,args,result,code]of [
  ['GetProcessTimes',[self,data,data+8n,data+16n,data+24n],0n,50],['GetProcessWorkingSetSize',[self,data,data+8n],0n,50],['SetProcessWorkingSetSize',[self,0n,0n],0n,50],['WaitForInputIdle',[self,0n],0xffffffffn,50],['CancelSynchronousIo',[thread(t)],0n,50],['LockFile',[0n,0n,0n,1n,0n],0n,6],['UnlockFile',[0n,0n,0n,1n,0n],0n,6],
  ['GetNumberOfConsoleInputEvents',[0n,data],0n,6],['SetConsoleMode',[0n,0n],0n,6],...['PeekConsoleInputA','PeekConsoleInputW','ReadConsoleInputA','ReadConsoleInputW'].map(n=>[n,[0n,data,1n,used],0n,6]),...['ShellExecuteExA','ShellExecuteExW'].map(n=>[n,[data],0n,1155]),['CreateProcessWithLogonW',Array(11).fill(data),0n,50],
 ]){fill(t,data,32);equal(api(t,name,args),result);error(t,code);equal(bytes(t,data,32),new Uint8Array(32).fill(0xa5));}
 for(const name of ['GetEffectiveRightsFromAclW','GetNamedSecurityInfoW','SetNamedSecurityInfoW','SetEntriesInAclW']){mark(t);fill(t,data,32);equal(api(t,name,Array(8).fill(data)),50n);error(t,123);equal(bytes(t,data,32),new Uint8Array(32).fill(0xa5));}
 mark(t);equal(api(t,'FreeConsole'),1n);equal(api(t,'timeSetEvent',[1n,0n,1n,2n,1n]),0n);equal(api(t,'timeKillEvent',[1n]),97n);error(t,123);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,first=await window(t);const atom=mode===32?call(t,'GetClassLongW',[first,0xffffffe0n]):0x20100n;
 const second=await invoke(t,'CreateWindowExW',[0n,atom,0n,0n,0n,0n,1n,1n,0n,0n,t.image.base,0n]);assert.ok(second);
 const child=await invoke(t,'CreateWindowExW',[0n,atom,0n,0x40000000n,0n,0n,1n,1n,first,0n,t.image.base,0n]);assert.ok(child);
 for(const [handle,command,expected,label]of [[first,0n,second,'first'],[second,1n,first,'last'],[second,2n,first,'next'],[first,3n,second,'previous'],[first,5n,child,'child'],[child,2n,0n,'child next'],[child,4n,0n,'child owner']])equal(api(t,'GetWindow',[handle,command]),expected,label);
 const desktop=call(t,'GetDesktopWindow');equal(api(t,'GetWindow',[desktop,5n]),second);equal(api(t,'GetWindow',[first,6n]),0n);error(t,50);equal(api(t,'GetWindow',[0n,0n]),0n);error(t,1400);vm.dispose();
}

// Formal imports cover the complete group. Guest calls exercise stack cleanup and both pointer ABIs.
for(const mode of [32,64])for(const [dll,names]of groups){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,40);f.rput(0x200c,0x2030);f.rtext(0x2030,dll+'\0');let at=0x2050,lookup=at;at+=(names.length+1)*f.ptr;const iat=at;at+=(names.length+1)*f.ptr;f.rput(0x2000,lookup);f.rput(0x2010,iat);
 for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}assert.ok(at<0x2600);f.relocations.length=0;const code=[];
 const chosen=dll==='kernel32.dll'?'GetCurrentProcessorNumberEx':dll==='ole32.dll'?'CoTaskMemRealloc':dll==='advapi32.dll'?'RevertToSelf':dll==='winmm.dll'?'timeKillEvent':dll==='shell32.dll'?'ShellExecuteExW':dll==='user32.dll'?'GetWindow':dll==='version.dll'?'VerLanguageNameW':'EnumProcesses';
 const args=chosen==='GetCurrentProcessorNumberEx'?[f.preferred+0x3000n]:chosen==='CoTaskMemRealloc'?[0n,16n]:chosen==='RevertToSelf'?[]:chosen==='timeKillEvent'?[0n]:chosen==='ShellExecuteExW'?[0n]:chosen==='GetWindow'?[0n,0n]:chosen==='VerLanguageNameW'?[0x409n,f.preferred+0x3000n,4n]:[f.preferred+0x3000n,4n,f.preferred+0x3010n];
 const absolute=(opcode,value)=>{code.push(...opcode);f.relocations.push(0x1000+code.length);code.push(...encode(value,f.ptr));};
 const argument=(opcode,value)=>{if(value>=f.preferred)absolute(opcode,value);else code.push(...opcode,...encode(value,f.ptr));};
 if(mode===32){for(const a of args.toReversed())argument([0x68],a);absolute([0xff,0x15],f.preferred+BigInt(iat+names.indexOf(chosen)*f.ptr));code.push(0xc3);}
 else{code.push(0x48,0x83,0xec,40);for(const[i,a]of args.entries())argument([[0x48,0xb9],[0x48,0xba],[0x49,0xb8]][i],a);code.push(0xff,0x15,...encode(BigInt(iat+names.indexOf(chosen)*f.ptr)-BigInt(0x1000+code.length+6),4),0x48,0x83,0xc4,40,0xc3);}
 f.bytes.set(code,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw Error('unexpected clock');}}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+i*f.ptr),f.ptr),vm.bind(dll,name));equal((await vm.run(100n)).reason,Exit.Returned);
 if(chosen==='CoTaskMemRealloc')assert.ok(vm.register(0));else equal(vm.register(0),['RevertToSelf','EnumProcesses'].includes(chosen)?1n:chosen==='timeKillEvent'?97n:chosen==='VerLanguageNameW'?3n:0n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} runtime security, task ownership, profile, process, window, failure and formal-import checks passed`);
