import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const shlwapi=['PathFileExistsA','PathFileExistsW','PathIsDirectoryA','PathIsDirectoryW','PathRemoveFileSpecA','PathRemoveFileSpecW','PathRenameExtensionA','PathRenameExtensionW','PathAddExtensionA','PathAddExtensionW','PathUnquoteSpacesA','PathUnquoteSpacesW'];
const universal=['WNetGetUniversalNameA','WNetGetUniversalNameW'];
const dll=name=>universal.includes(name)?'mpr.dll':name==='MakeSureDirectoryPathExists'?'imagehlp.dll':'shlwapi.dll';
const api=(t,name,args=[])=>call(t,name,args,dll(name));
const encoded=(s,wide=false)=>wide?Buffer.from(s+'\0','utf16le'):Buffer.from([...s+'\0'].flatMap(c=>({'ソ':[0x83,0x5c],'フ':[0x83,0x74],'ト':[0x83,0x67],'日':[0x93,0xfa],'本':[0x96,0x7b]})[c]??[c.charCodeAt(0)]));
const text=(vm,p,s,wide=false)=>vm.write(p,encoded(s,wide));
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const contents=(vm,p,s,wide=false)=>equal(bytes(vm,p,encoded(s,wide).length),new Uint8Array(encoded(s,wide)));

// The shared Wine-derived root rules operate on original bytes/UTF-16, including DBCS trails.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=mode===64?0x123500000000n:0x22000n;if(mode===64)vm.map(p,65536n,3);
 for(const name of [...shlwapi,...universal,'MakeSureDirectoryPathExists']){assert.ok(vm.bind(dll(name),name));for(const wrong of ['user32.dll','kernel32.dll'])assert.throws(()=>vm.bind(wrong,name),/unknown/);}
 for(const wide of [false,true])for(const[input,expected,changed]of [
  ['C:\\program.exe','C:\\',1n],['C:\\dir\\file','C:\\dir',1n],['C:\\','C:\\',0n],['C:','C:',0n],['file','',1n],
  ['\\\\server\\share\\file','\\\\server\\share',1n],['C:\\dir\\\\file','C:\\dir',1n],['C:\\ソフト\\ソ','C:\\ソフト',1n],['C:\\dir/file','C:\\',1n],['','',0n],
  ...(wide?[[String.raw`\\?\C:\file`,String.raw`\\?\C:`+'\\',1n],[String.raw`\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\file`,String.raw`\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}`+'\\',1n]]:[])
 ]){text(vm,p,input,wide);vm.write(t.error,encode(77n,4));equal(api(t,wide?'PathRemoveFileSpecW':'PathRemoveFileSpecA',[p]),changed);contents(vm,p,expected,wide);equal(vm.readInt(t.error),77n);}
 vm.write(p,[67,58,92,0x83,0]);equal(api(t,'PathRemoveFileSpecA',[p]),1n);contents(vm,p,'C:\\');
 text(vm,p,'x',true);equal(api(t,'PathRemoveFileSpecW',[p]),1n);equal(vm.readInt(p,2),0n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Extensions, quotes, limits and NULL short-circuits preserve the existing contracts.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,p=0x22000n,ext=0x23000n,suffix=wide?'W':'A';vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 for(const[input,extension,add,expected,result]of [
  ['C:\\dir.name\\file','.x',true,'C:\\dir.name\\file.x',1n],['name.old','.x',true,'name.old',0n],['name.old','.x',false,'name.x',1n],['name','.x',false,'name.x',1n],
  ['name.old part','.x',true,'name.old part.x',1n],['name.old','',false,'name',1n],['ソフト','.x',true,'ソフト.x',1n],['','',true,'',1n]
 ]){text(vm,p,input,wide);text(vm,ext,extension,wide);equal(api(t,`Path${add?'Add':'Rename'}Extension${suffix}`,[p,ext]),result);contents(vm,p,expected,wide);}
 text(vm,p,'name.ext',wide);equal(api(t,`PathRenameExtension${suffix}`,[p,p+BigInt(4*(wide?2:1))]),1n);contents(vm,p,'name.ext',wide);
 for(const[input,expected]of [['"ソ path"','ソ path'],['""',''],['"one','"one'],['plain','plain']]){text(vm,p,input,wide);equal(api(t,'PathUnquoteSpaces'+suffix,[p]),0n);contents(vm,p,expected,wide);}
 for(const name of ['PathRemoveFileSpec','PathUnquoteSpaces','PathAddExtension','PathRenameExtension']){equal(api(t,name+suffix,[0n,0x60000n]),0n);equal(vm.readInt(t.error),77n);}
 for(const name of ['PathAddExtension','PathRenameExtension'])equal(api(t,name+suffix,[0x60000n,0n]),0n);
 const long=wide?'a'.repeat(258):'ソ'.repeat(129);text(vm,p,long,wide);text(vm,ext,'.x',wide);equal(core.wire_protect(t.handle,p,4096n,2),0);equal(api(t,'PathAddExtension'+suffix,[p,ext]),0n);equal(vm.readInt(t.error),206n);contents(vm,p,long,wide);equal(core.wire_protect(t.handle,p,4096n,4),0);
 equal(vm.flags,0x8d5n);vm.dispose();
}

// VFS queries and recursive creation share ordinary file/directory identity and lifetime.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.supplyFile('C:\\inputs\\file.dat',[1,2,3]);
 for(const wide of [false,true])for(const[path,exists,directory,error]of [['C:\\inputs\\file.dat',1n,0n,77n],['C:\\inputs',1n,1n,77n],['C:\\missing',0n,0n,2n]]){
  text(vm,0x22000n,path,wide);for(const[name,want]of [['PathFileExists',exists],['PathIsDirectory',directory]]){vm.write(t.error,encode(77n,4));equal(api(t,name+(wide?'W':'A'),[0x22000n]),want);equal(vm.readInt(t.error),error);}
 }
 for(const[path,directory]of [['C:\\made\\child\\','C:\\made\\child'],['C:\\other\\parent\\file.bin','C:\\other\\parent']]){
  text(vm,0x22000n,path);equal(api(t,'MakeSureDirectoryPathExists',[0x22000n]),1n);text(vm,0x22000n,directory,true);equal(api(t,'PathIsDirectoryW',[0x22000n]),1n);
 }
 text(vm,0x22000n,'C:\\made\\child',true);equal(call(t,'RemoveDirectoryW',[0x22000n],'kernel32.dll'),1n);text(vm,0x22000n,'C:\\made',true);equal(api(t,'PathIsDirectoryW',[0x22000n]),1n);
 text(vm,0x22000n,'C:\\inputs\\file.dat\\child\\');equal(api(t,'MakeSureDirectoryPathExists',[0x22000n]),0n);equal(vm.readInt(t.error),3n);
 for(const name of ['PathFileExistsA','PathIsDirectoryW','MakeSureDirectoryPathExists']){equal(api(t,name,[0n]),0n);equal(vm.readInt(t.error),87n);}
 vm.write(t.error,encode(77n,4));for(const name of universal)equal(api(t,name,[0x60000n,0xffffffffn,0x60000n,0x60000n]),2250n);equal(vm.readInt(t.error),77n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Faulting writes leave all source bytes intact, with the original frame available to retry.
for(const mode of [32,64])for(const kind of ['extension','remove','source','error','return']){
 const t=await machine(mode),{vm}=t,p=kind==='remove'?0x24ffdn:0x24ff0n;vm.write(p,new Uint8Array(128).fill(0xa5));text(vm,p,kind==='remove'?'\\AB':'a',true);text(vm,0x23000n,'.long-extension',true);
 const name=kind==='remove'?'PathRemoveFileSpecW':kind==='error'?'PathFileExistsA':'PathRenameExtensionW',args=kind==='error'?[0n]:[kind==='source'?0x60000n:p,0x23000n];frame(t,name,args,dll(name));const before=snapshot(t),work=vm.work,original=bytes(vm,p,32);
 const page=kind==='error'?t.image.teb:kind==='return'?t.sp&~4095n:0x25000n;if(kind!=='source')equal(core.wire_protect(t.handle,page,4096n,kind==='return'?1:2),0);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(vm,p,32),original);
 if(kind==='source'){vm.map(0x60000n,4096n,3);text(vm,0x60000n,'a',true);}else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);vm.dispose();
}
// Source reads stop at NUL, and output aliasing does not replace the saved return address.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;text(vm,0x24ffen,'a');text(vm,0x23000n,'');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(api(t,'PathAddExtensionA',[0x24ffen,0x23000n]),1n);
 text(vm,0x23000n,'.x');frame(t,'PathAddExtensionA',[t.sp,0x23000n],'shlwapi.dll');vm.write(t.sp,encode(0x00636261n,mode/8));equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,0x00636261n);contents(vm,t.sp,'abc.x');vm.dispose();
}

// All fifteen exports resolve as static imports and an actual JIT call modifies the image.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),groups=[['shlwapi.dll',shlwapi,0x2080,0x2100],['imagehlp.dll',['MakeSureDirectoryPathExists'],0x2180,0x21a0],['mpr.dll',universal,0x21c0,0x2200]];
 f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,80);let at=0x2400;
 for(const[n,[provider,names,lookup,iat]]of groups.entries()){const descriptor=0x2000+n*20;f.rput(descriptor,lookup);f.rput(descriptor+12,at);f.rtext(at,provider+'\0');at+=provider.length+1;f.rput(descriptor+16,iat);for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}}
 assert.ok(at<0x2600);f.bytes.set(encoded('path',true),f.offset(0x3000));f.bytes.set(encoded('.x',true),f.offset(0x30a0));const iat=0x2100+9*f.ptr;
 const code=mode===32?[0x68,...encode(f.preferred+0x30a0n,4),0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+BigInt(iat),4),0xc3]:[0x48,0x83,0xec,0x28,0x48,0x8d,0x0d,...encode(0x3000n-0x100bn,4),0x48,0x8d,0x15,...encode(0x30a0n-0x1012n,4),0xff,0x15,...encode(BigInt(iat)-0x1018n,4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1001,0x1006,0x100c]:[]));f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);
 for(const[provider,names,,iat]of groups)for(const[i,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+i*f.ptr),f.ptr),vm.bind(provider,name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);contents(vm,image.base+0x3000n,'path.x',true);vm.dispose();
}
console.log(`${checks} path/root/encoding/VFS/ABI/JIT/fault checks passed`);
