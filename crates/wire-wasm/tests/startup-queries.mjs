import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[],dll='kernel32.dll')=>call(t,name,args,dll);
const utf16=s=>Array.from({length:s.length},(_,i)=>Array.from(encode(BigInt(s.charCodeAt(i)),2))).flat();
const string=(vm,p,s,wide=true)=>vm.write(p,wide?utf16(s+'\0'):new TextEncoder().encode(s+'\0'));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const encoded=(s,wide)=>wide?utf16(s+'\0'):Array.from(new TextEncoder().encode(s+'\0'));

// Identity length and error outputs are one checked publication.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x22000n,size=0x23000n;
 for(const wide of [false,true])for(const [base,format,text]of [['GetComputerName',null,'WIRE'],['GetUserName',null,'wire'],...[0,1,2,3,4,5,6,7].map(n=>['GetComputerNameEx',n,[0,4].includes(n)?'WIRE':[2,6].includes(n)?'':'wire'])]){
  const name=base+(wide?'W':'A'),user=base==='GetUserName',dll=user?'advapi32.dll':'kernel32.dll',args=pointer=>format===null?[pointer,size]:[BigInt(format),pointer,size],required=BigInt(text.length+1);
  vm.write(size,[...encode(0n,4),0xa5]);vm.write(t.error,encode(123n,4));equal(api(t,name,args(0xdead0000n),dll),0n);equal(vm.readInt(size),required);equal(vm.readInt(size+4n,1),0xa5n);equal(vm.readInt(t.error),user?122n:format===null?111n:234n);
  vm.write(size,encode(100n,4));vm.write(t.error,encode(123n,4));equal(api(t,name,args(out),dll),1n);equal(bytes(vm,out,encoded(text,wide).length),encoded(text,wide));equal(vm.readInt(size),user?required:required-1n);equal(vm.readInt(t.error),123n);
 }
 equal(api(t,'GetComputerNameExW',[8n,0xdead0000n,0xbeef0000n]),0n);equal(vm.readInt(t.error),87n);
 vm.write(size,encode(0n,4));frame(t,'GetComputerNameW',[0xdead0000n,size],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(size),0n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(size),5n);
 vm.write(size,encode(100n,4));vm.write(0x24ffen,[0xa5,0xa5]);frame(t,'GetComputerNameW',[0x24ffen,size],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(size),100n);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(size),4n);
 equal(api(t,'GetComputerNameA',[out,t.sp]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),4n);
 equal(t.time.calls,0);vm.dispose();
}

// Full paths use the current VFS directory and preserve precise pointer widths.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x23000n,part=0x24000n,p=mode/8;
 vm.supplyFile('C:\\app\\marker',[]);string(vm,input,'C:\\app');equal(api(t,'SetCurrentDirectoryW',[input]),1n);
 for(const wide of [false,true]){
  const name='GetFullPathName'+(wide?'W':'A'),expected='C:\\app\\file.txt';string(vm,input,'file.txt',wide);vm.write(out,new Uint8Array(40).fill(0xa5));vm.write(part,encode(123n,p));
  equal(api(t,name,[input,0n,0xdead0000n,0xbeef0000n]),16n);equal(api(t,name,[input,15n,out,part]),16n);equal(bytes(vm,out,40),new Array(40).fill(0xa5));equal(vm.readInt(part,p),123n);equal(api(t,name,[input,16n,out,part]),15n);equal(bytes(vm,out,encoded(expected,wide).length),encoded(expected,wide));equal(vm.readInt(part,p),out+BigInt(7*(wide?2:1)));
  vm.write(out,new Uint8Array(40).fill(0xa5));frame(t,name,[input,100n,out,0x25000n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,out,40),new Array(40).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
  string(vm,input,'C:\\',wide);equal(api(t,name,[input,100n,out,part]),3n);equal(vm.readInt(part,p),0n);string(vm,input,'folder/',wide);equal(api(t,name,[input,100n,out,part]),14n);equal(vm.readInt(part,p),0n);
 }
 vm.write(input,[67,58,92,0x83,0x5c,92,...new TextEncoder().encode('file.bin\0')]);equal(api(t,'GetFullPathNameA',[input,100n,out,part]),14n);equal(vm.readInt(part,p),out+6n);
 string(vm,input,'file.txt');equal(api(t,'GetFullPathNameW',[input,100n,t.sp,part]),15n);equal(vm.ip,vm.entryPoints.returned);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(api(t,'GetFullPathNameW',[input,100n,high,part]),15n);equal(vm.readInt(part,8),high+14n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(t.time.calls,0);vm.dispose();
}

// Expansion is one pass, preserves unknown names, and bounds amplification.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x23000n,name=0x24000n,value=0x26000n;
 string(vm,name,'Outer');string(vm,value,'%Inner%');equal(api(t,'SetEnvironmentVariableW',[name,value]),1n);string(vm,name,'Inner');string(vm,value,'literal');equal(api(t,'SetEnvironmentVariableW',[name,value]),1n);
 for(const wide of [false,true])for(const [source,result]of [['%SystemRoot%\\x','C:\\Windows\\x'],['%oUtEr%','%Inner%'],['%%|%Missing%|%open','%%|%Missing%|%open'],['plain','plain'],['','']]){
  const fn='ExpandEnvironmentStrings'+(wide?'W':'A'),expected=encoded(result,wide),required=BigInt(result.length+1);string(vm,input,source,wide);vm.write(out,new Uint8Array(expected.length).fill(0xa5));vm.write(t.error,encode(123n,4));equal(api(t,fn,[input,0xdead0000n,0n]),required);equal(api(t,fn,[input,out,required-1n]),required);equal(bytes(vm,out,expected.length),new Array(expected.length).fill(0xa5));equal(api(t,fn,[input,out,required]),required);equal(bytes(vm,out,expected.length),expected);equal(vm.readInt(t.error),123n);
 }
 string(vm,input,'%SystemRoot%');vm.write(0x24ffen,[0xa5,0xa5]);frame(t,'ExpandEnvironmentStringsW',[input,0x24ffen,100n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),11n);
 equal(api(t,'ExpandEnvironmentStringsW',[input,t.sp,100n]),11n);equal(vm.ip,vm.entryPoints.returned);equal(api(t,'ExpandEnvironmentStringsW',[input,input,100n]),0n);equal(vm.readInt(t.error),87n);
 if(mode===64){string(vm,0x29000n,'B');string(vm,0x30000n,'x'.repeat(32767));equal(api(t,'SetEnvironmentVariableW',[0x29000n,0x30000n]),1n);string(vm,0x20000n,'%B%'.repeat(6000));equal(api(t,'ExpandEnvironmentStringsW',[0x20000n,0xdead0000n,0n]),0n);equal(vm.readInt(t.error),8n);}
 equal(t.time.calls,0);vm.dispose();
}

function versionCall(t,name,args,dll=name.startsWith('Rtl')?'ntdll.dll':'kernel32.dll'){
 const {vm,mode,sp}=t;
 if(mode===64)return api(t,name,args,dll);
 frame(t,name,[],dll);
 if(name==='VerSetConditionMask'){vm.write(sp+4n,encode(args[0],8));vm.write(sp+12n,encode(args[1],4));vm.write(sp+16n,encode(args[2],4));}
 else {vm.write(sp+4n,encode(args[0],4));vm.write(sp+8n,encode(args[1],4));vm.write(sp+12n,encode(args[2],8));}
 const work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(vm.retired,retired);equal(vm.register(4),sp+20n);
 return name==='VerSetConditionMask'?vm.register(0)|(vm.register(2)<<32n):vm.register(0);
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x22000n,major=mode===32?6:10,minor=mode===32?1:0,build=mode===32?7601:19045,sp=mode===32?1:0;
 equal(api(t,'GetVersion'),BigInt(build)*65536n+BigInt(minor)*256n+BigInt(major));
 for(const [name,sizes,dll]of [['GetVersionExA',[148,156],'kernel32.dll'],['GetVersionExW',[276,284],'kernel32.dll'],['RtlGetVersion',[276,284],'ntdll.dll']])for(const size of sizes){
  vm.write(out,new Uint8Array(size+1).fill(0xa5));vm.write(out,encode(BigInt(size),4));vm.write(t.error,encode(123n,4));equal(api(t,name,[out],dll),name.startsWith('Rtl')?0n:1n);equal([4n,8n,12n,16n].map(n=>vm.readInt(out+n)),[BigInt(major),BigInt(minor),BigInt(build),2n]);equal(vm.readInt(out+BigInt(size),1),0xa5n);equal(vm.readInt(t.error),123n);if(size===156||size===284){equal(vm.readInt(out+BigInt(size-8),2),BigInt(sp));equal(vm.readInt(out+BigInt(size-4),2),mode===32?0n:0x100n);equal(vm.readInt(out+BigInt(size-2),1),1n);}
 }
 const request=(ansi,m=major,n=minor,service=sp)=>{const size=ansi?156:284;vm.write(out,new Uint8Array(size));for(const [offset,value,width]of [[0,size,4],[4,m,4],[8,n,4],[size-8,service,2],[size-2,1,1]])vm.write(out+BigInt(offset),encode(BigInt(value),width));};
 for(const ansi of [false,true]){const name=ansi?'VerifyVersionInfoA':'VerifyVersionInfoW';request(ansi,major-1,0xffffffff);equal(versionCall(t,name,[out,3n,27n]),1n);request(ansi,major+1,0);equal(versionCall(t,name,[out,3n,27n]),0n);equal(vm.readInt(t.error),1150n);request(ansi,major,minor,sp+1);equal(versionCall(t,name,[out,0x23n,27n|(3n<<15n)]),0n);equal(vm.readInt(t.error),1150n);request(ansi);equal(versionCall(t,name,[out,0n,27n]),0n);equal(vm.readInt(t.error),160n);}
 equal(versionCall(t,'VerSetConditionMask',[0x8000000000000000n,2n,0x103n]),0x8000000000000018n);
 request(false,major+1,0);vm.write(t.error,encode(123n,4));equal(versionCall(t,'RtlVerifyVersionInfo',[out,3n,27n]),0xc0000059n);equal(vm.readInt(t.error),123n);
 const pointer=0x25000n-156n;vm.write(pointer,new Uint8Array(156));vm.write(pointer,encode(156n,4));vm.write(pointer+4n,encode(BigInt(major),4));equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(versionCall(t,'VerifyVersionInfoA',[pointer,2n,8n]),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
 equal(t.time.calls,0);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['GetComputerNameA','GetComputerNameW','GetComputerNameExA','GetComputerNameExW','GetFullPathNameA','GetFullPathNameW','ExpandEnvironmentStringsA','ExpandEnvironmentStringsW','GetVersion','GetVersionExA','GetVersionExW','VerifyVersionInfoA','VerifyVersionInfoW','VerSetConditionMask']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 for(const name of ['GetUserNameA','GetUserNameW']){const address=vm.bind('advapi32.dll',name);assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);if(mode===32)equal(vm.bind('kernel32.dll',name),address);else assert.throws(()=>vm.bind('kernel32.dll',name),/unknown/);}
 for(const name of ['RtlGetVersion','RtlVerifyVersionInfo']){const address=vm.bind('ntdll.dll',name);if(mode===64){equal(vm.bind('kernel32.dll',name),address);equal(vm.bind('kernelbase.dll',name),address);}else{assert.throws(()=>vm.bind('kernel32.dll',name),/unknown/);assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);}}
 vm.dispose();
}
console.log(`${checks} identity/full-path/expansion/version ABI/profile/fault checks passed`);
