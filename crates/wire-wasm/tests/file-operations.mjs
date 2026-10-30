import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const text=(vm,p,s,wide=true)=>vm.write(p,wide?Buffer.from(s+'\0','utf16le'):Uint8Array.from([...s+'\0'].flatMap(c=>c==='ソ'?[0x83,0x5c]:[c.charCodeAt(0)])));
function string(vm,p,wide=true){let out='';for(let n=0;n<260;n++){const c=Number(vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1));if(!c)return out;out+=String.fromCharCode(c);}throw Error('unterminated path output');}
const open=(t,path,flags=0x80n)=>{text(t.vm,0x20000n,path);return api(t,'CreateFileW',[0x20000n,0x80000100n,7n,0n,3n,flags,0n]);};
function info(t,path,flags=0x80n){const h=open(t,path,flags);assert.notEqual(h,0xffffffffn);assert.notEqual(h,0xffffffffffffffffn);equal(api(t,'GetFileInformationByHandle',[h,0x28000n]),1n);const result={id:(t.vm.readInt(0x2802cn)<<32n)|t.vm.readInt(0x28030n),times:[4n,12n,20n].map(n=>t.vm.readInt(0x28000n+n,8)),size:(t.vm.readInt(0x28020n)<<32n)|t.vm.readInt(0x28024n)};equal(api(t,'CloseHandle',[h]),1n);return result;}
function content(t,path){const h=open(t,path);equal(api(t,'ReadFile',[h,0x29000n,64n,0x2a000n,0n]),1n);const bytes=Array.from({length:Number(t.vm.readInt(0x2a000n))},(_,n)=>Number(t.vm.readInt(0x29000n+BigInt(n),1)));equal(api(t,'CloseHandle',[h]),1n);return bytes;}
function setTimes(t,path,values){text(t.vm,0x20000n,path);const h=api(t,'CreateFileW',[0x20000n,0x100n,7n,0n,3n,0x80n,0n]);for(let n=0;n<3;n++)t.vm.write(0x27000n+BigInt(n*8),encode(BigInt(values[n]),8));equal(api(t,'SetFileTime',[h,0x27000n,0x27008n,0x27010n]),1n);equal(api(t,'CloseHandle',[h]),1n);}
function replace(t,original,replacement,backup=null,wide=true,flags=0n,reserved=0n){for(const[p,s]of [[0x22000n,original],[0x23000n,replacement],...(backup===null?[]:[[0x26000n,backup]])])text(t.vm,p,s,wide);return api(t,'ReplaceFile'+(wide?'W':'A'),[0x22000n,0x23000n,backup===null?0n:0x26000n,flags,0n,reserved]);}
function copy(t,source,target,wide=true,{flags=0n,cancel=0n,progress=0n}={}){text(t.vm,0x22000n,source,wide);text(t.vm,0x23000n,target,wide);return api(t,'CopyFileEx'+(wide?'W':'A'),[0x22000n,0x23000n,progress,0n,cancel,flags]);}
const error=(t,code)=>equal(t.vm.readInt(t.error),BigInt(code));

for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t;for(const[p,b]of [['C:\\old',[1,2,3]],['C:\\staging\\new',[4,5]],['C:\\backup',[9,9,9,9]]])vm.supplyFile(p,b);
 setTimes(t,'C:\\old',[11,12,13]);setTimes(t,'C:\\staging\\new',[21,22,23]);const old=info(t,'C:\\old'),replacement=info(t,'C:\\staging\\new'),saved=info(t,'C:\\backup');assert.notEqual(old.id,replacement.id);assert.notEqual(old.id,saved.id);
 vm.write(t.error,encode(123n,4));equal(replace(t,'C:\\old','C:\\staging\\new','C:\\backup',wide,6n),1n);equal(vm.readInt(t.error),123n);equal(info(t,'C:\\old'),{id:replacement.id,times:[11n,22n,23n],size:2n});equal(info(t,'C:\\backup'),old);equal(content(t,'C:\\old'),[4,5]);equal(content(t,'C:\\backup'),[1,2,3]);
 text(vm,0x22000n,'C:\\staging\\new');equal(api(t,'GetFileAttributesW',[0x22000n]),0xffffffffn);error(t,2);text(vm,0x22000n,'C:\\staging');equal(api(t,'GetFileAttributesW',[0x22000n]),0x10n);
 vm.supplyFile('C:\\next',[7,8,9,10]);const next=info(t,'C:\\next');equal(replace(t,'C:\\old','C:\\next',null,wide),1n);equal(info(t,'C:\\old').id,next.id);equal(content(t,'C:\\old'),[7,8,9,10]);equal(content(t,'C:\\backup'),[1,2,3]);
 vm.supplyFile('C:\\new',[42]);const before=info(t,'C:\\old'),newBefore=info(t,'C:\\new');
 for(const [backup,flags,reserved,code]of [['C:\\absent\\backup',0n,0n,3],['C:\\old',0n,0n,87],['C:\\new',0n,0n,87],[null,1n,0n,87],[null,0n,1n,87],...(mode===64?[[null,0n,1n<<32n,87]]:[])]){
  equal(replace(t,'C:\\old','C:\\new',backup,wide,flags,reserved),0n);error(t,code);equal(info(t,'C:\\old'),before);equal(info(t,'C:\\new'),newBefore);
 }
 const lock=open(t,'C:\\new');equal(replace(t,'C:\\old','C:\\new',null,wide),0n);error(t,32);equal(api(t,'CloseHandle',[lock]),1n);
 const sectionFile=open(t,'C:\\old'),section=api(t,'CreateFileMappingW',[sectionFile,0n,2n,0n,0n,0n]);equal(api(t,'CloseHandle',[sectionFile]),1n);equal(replace(t,'C:\\old','C:\\new',null,wide),0n);error(t,1224);equal(api(t,'CloseHandle',[section]),1n);
 equal(copy(t,'C:\\old','C:\\copy',wide,{flags:3n}),1n);equal(content(t,'C:\\copy'),[7,8,9,10]);assert.notEqual(info(t,'C:\\copy').id,before.id);equal(copy(t,'C:\\old','C:\\copy',wide,{flags:1n}),0n);error(t,80);equal(copy(t,'C:\\new','C:\\copy',wide),1n);equal(content(t,'C:\\copy'),[42]);
 vm.write(0x24000n,encode(1n,4));equal(copy(t,'C:\\old','C:\\cancelled',wide,{cancel:0x24000n}),0n);error(t,1235);text(vm,0x22000n,'C:\\cancelled');equal(api(t,'GetFileAttributesW',[0x22000n]),0xffffffffn);error(t,2);
 for(const options of [{flags:4n},{progress:1n},...(mode===64?[{progress:1n<<32n}]:[])]){equal(copy(t,'C:\\old','C:\\new',wide,options),0n);error(t,50);equal(content(t,'C:\\new'),[42]);}
 if(mode===64){equal(copy(t,'C:\\old','C:\\high-flags',wide,{flags:(1n<<32n)|1n}),1n);equal(content(t,'C:\\high-flags'),[7,8,9,10]);}
 for(const name of ['ReplaceFileA','ReplaceFileW','CopyFileExA','CopyFileExW']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.supplyFile('C:\\tree\\leaf',[1]);const dir=info(t,'C:\\tree',0x02000000n).id,file=info(t,'C:\\tree\\leaf').id;
 text(vm,0x22000n,'C:\\tree');text(vm,0x23000n,'C:\\renamed');equal(api(t,'MoveFileW',[0x22000n,0x23000n]),1n);equal(info(t,'C:\\renamed',0x02000000n).id,dir);equal(info(t,'C:\\renamed\\leaf').id,file);
 text(vm,0x22000n,'C:\\renamed\\leaf');equal(api(t,'DeleteFileW',[0x22000n]),1n);vm.supplyFile('C:\\renamed\\leaf',[2]);assert.notEqual(info(t,'C:\\renamed\\leaf').id,file);
 vm.supplyFile('C:\\source',[3]);text(vm,0x22000n,'C:\\source');text(vm,0x23000n,'C:\\copied');vm.write(0x24ffen,encode(0n,4));frame(t,'CopyFileExW',[0x22000n,0x23000n,0n,0n,0x24ffen,1n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(content(t,'C:\\copied'),[3]);
 vm.supplyFile('C:\\replace',[4]);text(vm,0x22000n,'C:\\source');text(vm,0x23000n,'C:\\replace');text(vm,0x24ffen,'C:\\saved');frame(t,'ReplaceFileW',[0x22000n,0x23000n,0x24ffen,0n,0n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(content(t,'C:\\source'),[4]);equal(content(t,'C:\\saved'),[3]);vm.dispose();
}

// Fixed lexical cases include the public Wine SHLWAPI path tests; no VFS lookup is involved.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,name='PathCanonicalize'+(wide?'W':'A'),path=(args)=>call(t,name,args,'shlwapi.dll');vm.write(t.error,encode(123n,4));
 for(const [input,output]of [['','\\'],[' ',' '],['one\\two','one\\two'],['C:','C:\\'],['C:\\one\\.\\..\\two\\three\\..','C:\\two'],['C:\\one/two/three\\four/five\\six','C:\\one/two/three\\four/five\\six'],['C:\\one/.\\two','C:\\one/.\\two'],['../../one/two/','../../one/two/'],['C:\\one/.\\..\\two/three\\..\\four/.five','C:\\four/.five'],['D:\\absent\\.\\file','D:\\absent\\file'],['\\\\server\\share\\.\\file','\\\\server\\share\\file']]){
  text(vm,0x22000n,input,wide);vm.write(0x24000n,new Uint8Array(520).fill(0xa5));equal(path([0x24000n,0x22000n]),1n);equal(string(vm,0x24000n,wide),output);equal(vm.readInt(0x24000n+BigInt(Buffer.byteLength(output,wide?'utf16le':'ascii')+(wide?2:1)),1),0xa5n);equal(vm.readInt(t.error),123n);
 }
 text(vm,0x22000n,'C:\\ソ\\.\\file',wide);equal(path([0x22000n,0x22000n]),1n);const expected=wide?Buffer.from('C:\\ソ\\file\0','utf16le'):Uint8Array.from([67,58,92,0x83,0x5c,92,102,105,108,101,0]);equal(Uint8Array.from({length:expected.length},(_,n)=>Number(vm.readInt(0x22000n+BigInt(n),1))),Uint8Array.from(expected));
 if(wide){text(vm,0x22000n,'\ud800\\.\\x');equal(path([0x24000n,0x22000n]),1n);equal(string(vm,0x24000n),'\ud800\\x');}
 text(vm,0x22000n,'a'.repeat(259),wide);equal(path([0x24000n,0x22000n]),1n);equal(string(vm,0x24000n,wide),'a'.repeat(259));text(vm,0x22000n,'a'.repeat(260),wide);equal(path([0x24000n,0x22000n]),0n);error(t,206);equal(vm.readInt(0x24000n,wide?2:1),0n);
 text(vm,0x22000n,'C:\\a\\..\\x',wide);equal(path([t.sp,0x22000n]),1n);equal(vm.ip,vm.entryPoints.returned);
 frame(t,name,[0x24ffen,0x22000n],'shlwapi.dll');vm.write(0x24ffen,[0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);
 vm.write(0x24000n,[0xa5,0xa5]);frame(t,name,[0x24000n,0n],'shlwapi.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24000n,2),0xa5a5n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);error(t,87);equal(vm.readInt(0x24000n,wide?2:1),0n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const fileNames=['CopyFileExA','CopyFileExW','ReplaceFileA','ReplaceFileW'],pathNames=['PathCanonicalizeA','PathCanonicalizeW'],f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);
 for(const[desc,lookup,iat,dllAt,dll,names]of [[0x2000,0x2080,0x2180,0x2260,'kernel32.dll',fileNames],[0x2014,0x20c0,0x21c0,0x2270,'shlwapi.dll',pathNames]]){f.rput(desc,lookup);f.rput(desc+12,dllAt);f.rput(desc+16,iat);f.rtext(dllAt,dll+'\0');}
 let at=0x2300;for(const[lookup,iat,names]of [[0x2080,0x2180,fileNames],[0x20c0,0x21c0,pathNames]])for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 f.bytes.set(Buffer.from('C:\\a\\..\\done\0','utf16le'),f.offset(0x3040));
 if(mode===32){f.bytes.set([0x68,...encode(f.preferred+0x3040n,4),0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+0x21c4n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1001,0x1006,0x100c);}
 else{f.bytes.set([0x48,0x83,0xec,40,0x48,0xb9,...encode(f.preferred+0x3000n,8),0x48,0xba,...encode(f.preferred+0x3040n,8),0xff,0x15,...encode(0x11aan,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1006,0x1010);}
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of fileNames.entries())equal(vm.readInt(image.base+0x2180n+BigInt(i*f.ptr),f.ptr),vm.bind('kernel32.dll',name));for(const[i,name]of pathNames.entries())equal(vm.readInt(image.base+0x21c0n+BigInt(i*f.ptr),f.ptr),vm.bind('shlwapi.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);equal(string(vm,image.base+0x3000n),'C:\\done');assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} file replacement/copy/identity/lexical-path/ABI/fault/JIT checks passed`);
