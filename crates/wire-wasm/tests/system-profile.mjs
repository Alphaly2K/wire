import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

const kernel=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const status=t=>{
 t.vm.write(0x21000n,encode(64n,4));equal(kernel(t,'GlobalMemoryStatusEx',[0x21000n]),1n);
 return [4,8,16,24,32,40,48,56].map((p,i)=>t.vm.readInt(0x21000n+BigInt(p),i?8:4));
};

// The guest has one fixed volume and an AC desktop profile, independent of the host.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));
 for(const [name,wide]of [['GetDriveTypeA',false],['GetDriveTypeW',true]]){
  equal(kernel(t,name,[0n]),3n);equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));
  for(const [path,result]of [['C:\\',3n],['c:/',3n],['\\',3n],['C:\\Windows\\',3n],['',1n],['C:',1n],['D:\\',1n],['\\\\server\\share\\',1n],['C:\\absent\\',1n],['C:\\program.exe\\',1n]]){
   vm.write(0x21000n,wide?Uint8Array.from([...path+'\0'].flatMap(c=>[c.charCodeAt(0),0])):new TextEncoder().encode(path+'\0'));equal(kernel(t,name,[0x21000n]),result);
  }
  frame(t,name,[0xdead0000n],'kernel32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 }
 const out=0x21001n;vm.write(out-1n,new Uint8Array(14).fill(0xa5));equal(kernel(t,'GetSystemPowerStatus',[out]),1n);
 equal(bytes(vm,out,12),Uint8Array.from([1,128,255,0,255,255,255,255,255,255,255,255]));equal(vm.readInt(out-1n,1),0xa5n);equal(vm.readInt(out+12n,1),0xa5n);
 equal(vm.bind('kernelbase.dll','GetSystemPowerStatus'),vm.bind('kernel32.dll','GetSystemPowerStatus'));equal(vm.readInt(t.error),123n);equal(t.time.calls,0);vm.dispose();
}

// Hardware layouts and live accounting must agree across old and Ex queries.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,wide=mode===64,p=mode/8;
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.xmm(5,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 for(const name of ['GetSystemInfo','GetNativeSystemInfo']){
  const length=wide?48:36,at=0x21000n;vm.write(at-1n,new Uint8Array(length+2).fill(0xa5));equal(kernel(t,name,[at]),0n);
  equal(vm.readInt(at-1n,1),0xa5n);equal(vm.readInt(at+BigInt(length),1),0xa5n);
  const read=(n,size=4)=>vm.readInt(at+BigInt(n),size),s=8+3*p;
  equal(read(0,2),wide?9n:0n);equal(read(2,2),0n);equal(read(4),4096n);equal(read(8,p),65536n);equal(read(8+p,p),wide?0x7ffffffeffffn:0x7ffeffffn);equal(read(8+2*p,p),1n);equal(read(s),1n);equal(read(s+4),wide?8664n:586n);equal(read(s+8),65536n);equal(read(s+12,2),6n);equal(read(s+14,2),0n);
 }
 const before=status(t);equal(before[1],2147483648n);equal(before[3],before[1]);equal(before[4],before[2]);equal(before[5],wide?0x7ffffffe0000n:0x7ffe0000n);equal(before[7],0n);
 const base=kernel(t,'VirtualAlloc',[0x60000n,12288n,0x2000n,4n]);equal(base,0x60000n);const reserve=status(t);equal(reserve[2],before[2]);equal(reserve[6],before[6]-12288n);
 equal(kernel(t,'VirtualAlloc',[base+4096n,4096n,0x1000n,4n]),base+4096n);const commit=status(t);equal(commit[2],before[2]-4096n);equal(commit[4],before[4]-4096n);equal(commit[6],reserve[6]);
 if(!wide){equal(kernel(t,'GlobalMemoryStatus',[0x21200n]),0n);equal(vm.readInt(0x21200n),32n);equal(vm.readInt(0x21204n),commit[0]);for(let n=0;n<6;n++)equal(vm.readInt(0x21208n+BigInt(n*4)),commit[n+1]);}
 equal(kernel(t,'VirtualFree',[base+4096n,4096n,0x4000n]),1n);equal(status(t),reserve);equal(kernel(t,'VirtualFree',[base,0n,0x8000n]),1n);equal(status(t),before);
 if(wide){const size=0x100002000n,high=kernel(t,'VirtualAlloc',[0n,size,0x102000n,4n]);assert.ok(high>0xffffffffn);const large=status(t);equal(large[2],before[2]);equal(large[6],before[6]-size);equal(kernel(t,'VirtualFree',[high,0n,0x8000n]),1n);equal(status(t),before);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(5),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(t.time.calls,0);vm.dispose();
}

// Relation headers, masks and reserved bytes follow the actual pointer ABI.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,wide=mode===64,size=wide?48:44,total=wide?224:208,len=0x21200n,out=0x22000n;
 for(const relation of [0,1,2,3,4,6,0xffff]){
  const expected=relation===2?0:relation===4?(wide?80:76):relation===0xffff?total:size;
  vm.write(len,encode(0n,4));equal(kernel(t,'GetLogicalProcessorInformationEx',[BigInt(relation),0xdead0000n,len]),expected?0n:1n);equal(vm.readInt(len),BigInt(expected));if(expected)equal(vm.readInt(t.error),122n);
  vm.write(out-1n,new Uint8Array(total+2).fill(0xa5));vm.write(len,encode(BigInt(total),4));equal(kernel(t,'GetLogicalProcessorInformationEx',[BigInt(relation),out,len]),1n);equal(vm.readInt(len),BigInt(expected));equal(vm.readInt(out-1n,1),0xa5n);equal(vm.readInt(out+BigInt(expected),1),0xa5n);
  const kinds=[];let cursor=0;
  while(cursor<expected){const at=out+BigInt(cursor),kind=Number(vm.readInt(at)),length=Number(vm.readInt(at+4n));kinds.push(kind);equal(length,kind===4?(wide?80:76):size);const body=bytes(vm,at+8n,length-8);
   if(kind===4){equal(vm.readInt(at+8n,2),1n);equal(vm.readInt(at+10n,2),1n);equal(vm.readInt(at+32n,1),1n);equal(vm.readInt(at+33n,1),1n);equal(vm.readInt(at+72n,mode/8),1n);for(const n of [0,2,24,25,64])body[n]=0;}
   else{if(kind!==1){equal(vm.readInt(at+30n,2),1n);body[22]=0;}equal(vm.readInt(at+32n,mode/8),1n);body[24]=0;}
   equal(body,new Uint8Array(length-8));cursor+=length;
  }
  equal(kinds,relation===2?[]:relation===0xffff?[0,1,3,4]:[relation]);
 }
 for(const invalid of [5,7,0x10000]){vm.write(len,encode(99n,4));equal(kernel(t,'GetLogicalProcessorInformationEx',[BigInt(invalid),0xdead0000n,len]),0n);equal(vm.readInt(t.error),87n);equal(vm.readInt(len),99n);}
 equal(t.time.calls,0);vm.dispose();
}

// Failed output and error-slot accesses publish no partial data or CPU work.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,len=0x21200n,size=mode===64?48:44;
 for(const [name,n]of [['GetSystemPowerStatus',12],['GetSystemInfo',mode===64?48:36],['GetNativeSystemInfo',mode===64?48:36],['GlobalMemoryStatusEx',64],['GetStartupInfoA',mode===64?104:68],['GetStartupInfoW',mode===64?104:68],...(mode===32?[['GlobalMemoryStatus',32]]:[])]){
  const out=name==='GetSystemPowerStatus'?0x24ff8n:0x24ff0n;vm.write(out,new Uint8Array(n).fill(0xa5));if(name==='GlobalMemoryStatusEx')vm.write(out,encode(64n,4));frame(t,name,[out],'kernel32.dll');const initial=bytes(vm,out,16);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,out,16),initial);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);
 }
 vm.write(0x21000n,new Uint8Array(64).fill(0xa5));vm.write(0x21000n,encode(63n,4));const invalid=bytes(vm,0x21000n,64);equal(kernel(t,'GlobalMemoryStatusEx',[0x21000n]),0n);equal(vm.readInt(t.error),87n);equal(bytes(vm,0x21000n,64),invalid);
 vm.write(len,encode(0n,4));frame(t,'GetLogicalProcessorInformationEx',[0n,0xdead0000n,len],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);const initial=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),initial);equal(vm.readInt(len),0n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(len),BigInt(size));equal(vm.readInt(t.error),122n);
 vm.write(len,encode(1024n,4));vm.write(0x24ff0n,new Uint8Array(size).fill(0xa5));frame(t,'GetLogicalProcessorInformationEx',[0n,0x24ff0n,len],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(len),1024n);equal(bytes(vm,0x24ff0n,16),new Uint8Array(16).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(len),BigInt(size));
 // Length can alias the saved return; successful completion uses the captured PC.
 frame(t,'GetLogicalProcessorInformationEx',[0n,0x22000n,t.sp],'kernel32.dll');equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),BigInt(size));
 for(const name of ['GetSystemInfo','GetStartupInfoW']){equal(kernel(t,name,[t.sp]),0n);equal(vm.ip,vm.entryPoints.returned);}
 // A missing first return frame prevents even a pointer-only query from completing.
 frame(t,'GetCommandLineW',[],'kernel32.dll');const stack=t.sp&~4095n;equal(core.wire_protect(t.handle,stack,4096n,1),0);const first=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),first);equal(core.wire_protect(t.handle,stack,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),t.image.commandW);
 equal(t.time.calls,0);vm.dispose();
}

// Startup pointers retain process ownership; feature replies match the virtual ISA.
for(const mode of [32,64])for(const dep of [false,true]){
 const t=await machine(mode,{dep}),{vm}=t;vm.write(t.error,encode(0xabcdef01n,4));
 for(const [name,key,encoding]of [['GetCommandLineA','commandA','utf-8'],['GetCommandLineW','commandW','utf-16le']]){const address=kernel(t,name);equal(address,t.image[key]);equal(kernel(t,name),address);const n=encoding==='utf-8'?16:32;equal(new TextDecoder(encoding).decode(bytes(vm,address,n)).replace(/\0.*$/s,''),'"C:\\program.exe"');}
 for(const name of ['GetStartupInfoA','GetStartupInfoW']){const n=mode===64?104:68;vm.write(0x21000n-1n,new Uint8Array(n+2).fill(0xa5));equal(kernel(t,name,[0x21000n]),0n);equal(vm.readInt(0x21000n),BigInt(n));equal(bytes(vm,0x21004n,n-4),new Uint8Array(n-4));equal(vm.readInt(0x20fffn,1),0xa5n);equal(vm.readInt(0x21000n+BigInt(n),1),0xa5n);}
 const supported=new Set([2,3,6,8,10,11,13,14,15,32,36,37,38]);if(dep||mode===64)supported.add(12);
 for(let feature=0;feature<=64;feature++)equal(kernel(t,'IsProcessorFeaturePresent',[BigInt(feature)]),supported.has(feature)?1n:0n);
 if(mode===64)equal(kernel(t,'IsProcessorFeaturePresent',[0xabcdef0000000025n]),1n);
 equal(vm.readInt(t.error),0xabcdef01n);equal(t.time.calls,0);vm.dispose();
}

// Admission remains provider-specific, including the intentionally absent legacy Win64 call.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['GetSystemInfo','GetNativeSystemInfo','GlobalMemoryStatusEx','GetLogicalProcessorInformationEx','GetCommandLineA','GetCommandLineW','GetStartupInfoA','GetStartupInfoW','IsProcessorFeaturePresent']){const address=vm.bind('kernel32.dll',name);assert.throws(()=>vm.bind('user32.dll',name),/unknown/);if(mode===64)equal(vm.bind('kernelbase.dll',name),address);else assert.throws(()=>vm.bind('kernelbase.dll',name),/unknown/);}
 if(mode===64)assert.throws(()=>vm.bind('kernel32.dll','GlobalMemoryStatus'),/unknown/);vm.dispose();
}

console.log(`${checks} system-profile/startup ABI/accounting/topology/fault/feature checks passed`);
