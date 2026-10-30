import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const HKCU=0x80000001n, input=0x20000n, out=0x22000n, count=0x23000n, kind=0x23010n, size=0x23020n, data=0x26000n;
const names=['RegEnumKeyA','RegEnumKeyW','RegEnumKeyExA','RegEnumKeyExW','RegEnumValueA','RegEnumValueW','RegQueryInfoKeyA','RegQueryInfoKeyW','RegDeleteKeyA','RegDeleteKeyW','RegDeleteKeyExA','RegDeleteKeyExW','RegDeleteTreeA','RegDeleteTreeW','RegFlushKey'];
const api=(t,name,args=[])=>call(t,name,args,name.startsWith('SH')?'shlwapi.dll':'advapi32.dll');
function text(t,s,wide=true,p=input){t.vm.write(p,wide?Buffer.from(s+'\0','utf16le'):Uint8Array.from([...s+'\0'].flatMap(c=>c==='ソ'?[0x83,0x5c]:[c.charCodeAt(0)])));return p;}
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const string=(vm,p,wide=true)=>{let s='';for(let n=0;n<256;n++){const c=Number(vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1));if(!c)return s;s+=String.fromCharCode(c);}throw Error('unterminated registry name');};
function create(t,path,base=HKCU,access=0xf003fn){equal(api(t,'RegCreateKeyExW',[base,text(t,path),0n,0n,0n,access,0n,out,0n]),0n);return t.vm.readInt(out,t.mode/8);}
function open(t,path,base=HKCU,access=0xf003fn){equal(api(t,'RegOpenKeyExW',[base,text(t,path),0n,access,out]),0n);return t.vm.readInt(out,t.mode/8);}
function set(t,key,name,values,wide=true,type=3n){t.vm.write(data,[...values,0x7f,0x7f]);equal(api(t,'RegSetValueEx'+(wide?'W':'A'),[key,text(t,name,wide),0n,type,data,BigInt(values.length)]),0n);}
function query(t,key){return api(t,'RegQueryValueExW',[key,0n,0n,0n,0n,size]);}

// Enumeration must preserve spelling and distinguish character and byte capacities.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,suffix=wide?'W':'A',width=wide?2:1,key=create(t,'Software\\Enumeration');
 for(const path of ['zLast','Alpha','ソ'])create(t,path,key);
 create(t,'Alpha\\Grandchild',key);create(t,'ALPHA',key);
 set(t,key,'MixedCase',[4,5,6]);set(t,key,'MIXEDCASE',[7,8]);
 set(t,key,'ソ',[65,0,66,0],true,1n);set(t,key,'',[],true,3n);
 const spelling=[];
 for(let i=0;i<3;i++){
  vm.write(count,encode(64n,4));vm.write(size,encode(1n,4));
  equal(api(t,'RegEnumKeyEx'+suffix,[key,BigInt(i),out,count,0n,data,size,0n]),0n);
  const raw=bytes(vm,out,Number(vm.readInt(count))*width);
  spelling.push(wide?Buffer.from(raw).toString('utf16le'):Buffer.from(raw).equals(Buffer.from([0x83,0x5c]))?'ソ':Buffer.from(raw).toString());
  equal(vm.readInt(data,width),0n);equal(vm.readInt(size),0n);
 }
 equal(spelling.sort(),['Alpha','zLast','ソ'].sort());
 vm.write(count,encode(123n,4));vm.write(out,new Uint8Array(32).fill(0xa5));
 equal(api(t,'RegEnumKeyEx'+suffix,[key,3n,out,count,0n,0n,0n,0n]),259n);equal(vm.readInt(count),123n);equal(bytes(vm,out,32),new Uint8Array(32).fill(0xa5));
 vm.write(count,encode(5n,4));equal(api(t,'RegEnumKeyEx'+suffix,[key,0n,out,count,0n,0n,0n,0n]),234n);equal(vm.readInt(count),5n);equal(bytes(vm,out,32),new Uint8Array(32).fill(0xa5));
 equal(api(t,'RegEnumKey'+suffix,[key,0n,out,6n]),0n);equal(string(vm,out,wide),'Alpha');
 equal(api(t,'RegEnumKey'+suffix,[key,0n,out,5n]),234n);
 vm.write(count,encode(1n,4));vm.write(size,encode(0n,4));
 equal(api(t,'RegEnumValue'+suffix,[key,0n,out,count,0n,kind,0n,size]),0n);equal(vm.readInt(count),0n);equal(vm.readInt(size),0n);equal(vm.readInt(out,width),0n);
 vm.write(count,encode(64n,4));vm.write(size,encode(1n,4));vm.write(data,new Uint8Array(8).fill(0xa5));
 equal(api(t,'RegEnumValue'+suffix,[key,1n,out,count,0n,kind,data,size]),234n);equal(string(vm,out,wide),'MixedCase');equal(vm.readInt(size),2n);equal(bytes(vm,data,8),new Uint8Array(8).fill(0xa5));
 vm.write(count,encode(64n,4));vm.write(size,encode(2n,4));equal(api(t,'RegEnumValue'+suffix,[key,1n,out,count,0n,kind,data,size]),0n);equal(bytes(vm,data,3),new Uint8Array([7,8,0xa5]));
 vm.write(count,encode(64n,4));vm.write(size,encode(64n,4));equal(api(t,'RegEnumValue'+suffix,[key,2n,out,count,0n,kind,data,size]),0n);equal(vm.readInt(count),wide?1n:2n);equal(vm.readInt(size),wide?4n:2n);equal(vm.readInt(kind),1n);equal(bytes(vm,data,wide?4:2),wide?new Uint8Array([65,0,66,0]):new Uint8Array([65,66]));
 // All counts describe immediate children and stored values, including the unnamed value.
 const fields=Array.from({length:6},(_,n)=>0x24000n+BigInt(n*4));
 equal(api(t,'RegQueryInfoKey'+suffix,[key,0n,0n,0n,fields[0],fields[1],0n,fields[2],fields[3],fields[4],0n,0n]),0n);
 equal(fields.slice(0,5).map(p=>vm.readInt(p)),[3n,5n,3n,9n,wide?4n:2n]);
 const encoded=create(t,'Encoded',key);create(t,'ソ',encoded);set(t,encoded,'ソ',[0,0xd8,0],true,1n);
 equal(api(t,'RegQueryInfoKey'+suffix,[encoded,0n,0n,0n,fields[0],fields[1],0n,fields[2],fields[3],fields[4],0n,0n]),0n);
 equal(fields.slice(0,5).map(p=>vm.readInt(p)),[1n,wide?1n:2n,1n,wide?1n:2n,wide?3n:1n]);
 vm.write(count,encode(1n,4));vm.write(size,encode(64n,4));vm.write(out,new Uint8Array(8).fill(0xa5));vm.write(data,new Uint8Array(8).fill(0xa5));
 equal(api(t,'RegEnumValue'+suffix,[encoded,0n,out,count,0n,kind,data,size]),234n);equal(vm.readInt(count),wide?1n:2n);equal(bytes(vm,out,8),new Uint8Array(8).fill(0xa5));equal(bytes(vm,data,8),new Uint8Array(8).fill(0xa5));
 vm.write(count,encode(0n,4));vm.write(data,[0xa5,0xa5]);equal(api(t,'RegQueryInfoKey'+suffix,[key,data,count,0n,0n,0n,0n,0n,0n,0n,0n,0n]),234n);equal(vm.readInt(count),0n);equal(vm.readInt(data,2),0xa5a5n);
 vm.write(count,encode(1n,4));equal(api(t,'RegQueryInfoKey'+suffix,[key,data,count,0n,0n,0n,0n,0n,0n,0n,0n,0n]),0n);equal(vm.readInt(data,width),0n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Every handle to a deleted node stays deleted when the same path is recreated.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,suffix=wide?'W':'A',parent=create(t,'Software\\Delete'),old=create(t,'Leaf',parent),alias=open(t,'leaf',parent);
 set(t,old,'',[9]);vm.write(t.error,encode(123n,4));
 const readonly=open(t,'Software\\Delete',HKCU,1n);
 equal(api(t,'RegDeleteKey'+suffix,[readonly,text(t,'LEAF',wide)]),0n);
 for(const h of [old,alias])equal(query(t,h),1018n);
 const replacement=create(t,'Leaf',parent);equal(query(t,replacement),2n);equal(query(t,old),1018n);equal(api(t,'RegCloseKey',[old]),0n);equal(api(t,'RegCloseKey',[old]),6n);
 const child=create(t,'Branch\\Child',parent);set(t,child,'',[1]);
 equal(api(t,'RegDeleteKey'+suffix,[parent,text(t,'Branch',wide)]),5n);
 for(const [view,reserved,error]of [[0x300n,0n,87n],[1n,0n,87n],[0x200n,0n,50n],[0n,1n,87n]])equal(api(t,'RegDeleteKeyEx'+suffix,[parent,text(t,'Leaf',wide),view,reserved]),error);
 equal(query(t,replacement),2n);equal(api(t,'RegDeleteKeyEx'+suffix,[parent,text(t,'Leaf',wide),0x100n,0n]),0n);equal(query(t,replacement),1018n);
 equal(api(t,'RegDeleteTree'+suffix,[readonly,text(t,'Branch',wide)]),5n);
 const noSet=open(t,'Software\\Delete',HKCU,0x10009n);equal(api(t,'RegDeleteTree'+suffix,[noSet,text(t,'Branch',wide)]),5n);
 equal(api(t,'RegDeleteTree'+suffix,[parent,text(t,'Branch',wide)]),0n);equal(query(t,child),1018n);
 const prefixSibling=create(t,'BranchOther',parent);equal(query(t,prefixSibling),2n);
 set(t,parent,'',[3]);const kept=create(t,'Kept\\Nested',parent);equal(api(t,'RegDeleteTree'+suffix,[parent,0n]),0n);equal(query(t,parent),2n);equal(query(t,kept),1018n);equal(query(t,prefixSibling),1018n);
 const shellChild=create(t,'Shell\\Nested',parent);equal(api(t,'SHDeleteKey'+suffix,[parent,text(t,'Shell',wide)]),0n);equal(query(t,shellChild),1018n);
 create(t,'ShellAgain',parent);equal(api(t,'SHDeleteKey'+suffix,[parent,0n]),0n);equal(query(t,parent),2n);
 equal(api(t,'RegDeleteKey'+suffix,[parent,0n]),87n);equal(api(t,'RegDeleteKey'+suffix,[parent,text(t,'',wide)]),87n);
 equal(api(t,'RegDeleteTree'+suffix,[0x80000002n,0n]),5n);equal(vm.readInt(t.error),123n);vm.dispose();
}

// Permissions, unsupported metadata, full pointers and parameter errors preserve outputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,key=create(t,'Software\\Rights');create(t,'Child',key);set(t,key,'Value',[1]);
 const none=open(t,'Software\\Rights',HKCU,0n),queryOnly=open(t,'Software\\Rights',HKCU,1n),enumOnly=open(t,'Software\\Rights',HKCU,8n);
 vm.write(count,encode(64n,4));
 equal(api(t,'RegEnumKeyExW',[queryOnly,0n,out,count,0n,0n,0n,0n]),5n);equal(api(t,'RegEnumValueW',[enumOnly,0n,out,count,0n,0n,0n,0n]),5n);
 equal(api(t,'RegQueryInfoKeyW',[enumOnly]),5n);equal(api(t,'RegFlushKey',[none]),5n);equal(api(t,'RegFlushKey',[key]),50n);
 for(const [name,args]of [['RegEnumKeyExW',[key,0n,out,count,1n]],['RegEnumKeyExW',[key,0n,out,count,0n,data,0n]],['RegEnumValueW',[key,0n,out,count,1n]],['RegEnumValueW',[key,0n,out,count,0n,0n,data,0n]],['RegQueryInfoKeyW',[key,data,0n]],['RegQueryInfoKeyW',[key,0n,0n,1n]]])equal(api(t,name,args),87n);
 for(const [name,args]of [['RegEnumKeyExW',[key,0n,out,count,0n,0n,0n,data]],['RegQueryInfoKeyW',[key,0n,0n,0n,count,0n,0n,0n,0n,0n,data,0n]],['RegQueryInfoKeyW',[key,0n,0n,0n,count,0n,0n,0n,0n,0n,0n,data]]]){
  vm.write(count,encode(64n,4));vm.write(data,new Uint8Array(8).fill(0xa5));equal(api(t,name,args),50n);equal(vm.readInt(count),64n);equal(bytes(vm,data,8),new Uint8Array(8).fill(0xa5));
 }
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,7);vm.write(high,encode(64n,4));equal(api(t,'RegEnumKeyExW',[key,1n<<32n,high+16n,high,0n,0n,0n,0n]),0n);equal(string(vm,high+16n),'Child');equal(api(t,'RegEnumKeyExW',[key&0xffffffffn,0n,out,count]),6n);}
 for(const name of [...names,'SHDeleteKeyA','SHDeleteKeyW']){assert.ok(vm.bind(name.startsWith('SH')?'shlwapi.dll':'advapi32.dll',name));assert.throws(()=>vm.bind(name.startsWith('SH')?'advapi32.dll':'shlwapi.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// A later copyout fault must not publish earlier metadata or a partial name.
for(const mode of [32,64])for(const operation of ['key-name','key-class','value-data','info-count']){
 const t=await machine(mode),{vm}=t,key=create(t,'Software\\Atomic');create(t,'Child',key);set(t,key,'Value',[1,2,3,4]);
 vm.write(count,encode(64n,4));vm.write(kind,encode(123n,4));vm.write(size,encode(64n,4));vm.write(out,new Uint8Array(32).fill(0xa5));vm.write(0x24ffen,[0xa5,0xa5]);
 if(operation==='key-name')frame(t,'RegEnumKeyExW',[key,0n,0x24ffen,count,0n,0n,0n,0n],'advapi32.dll');
 if(operation==='key-class')frame(t,'RegEnumKeyExW',[key,0n,out,count,0n,0x25000n,size,0n],'advapi32.dll');
 if(operation==='value-data')frame(t,'RegEnumValueW',[key,0n,out,count,0n,kind,0x24ffen,size],'advapi32.dll');
 if(operation==='info-count')frame(t,'RegQueryInfoKeyW',[key,0n,0n,0n,count,0x25000n,0n,kind,0n,0n,0n,0n],'advapi32.dll');
 equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t),work=vm.work,retired=vm.retired;
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);equal(vm.readInt(count),64n);equal(vm.readInt(kind),123n);equal(vm.readInt(size),64n);equal(bytes(vm,out,32),new Uint8Array(32).fill(0xa5));equal(vm.readInt(0x24ffen,2),0xa5a5n);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);vm.dispose();
}

// A failed call frame must not delete a tree; aliased outputs use captured returns.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,parent=create(t,'Software\\Frames'),child=create(t,'Child',parent),path=text(t,'Child');
 frame(t,'RegDeleteTreeW',[parent,path],'advapi32.dll');const stack=t.sp&~4095n;equal(core.wire_protect(t.handle,stack,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,stack,4096n,4),0);equal(query(t,child),2n);
 vm.write(count,encode(64n,4));equal(api(t,'RegEnumKeyExW',[parent,0n,t.sp,count,0n,0n,0n,0n]),0n);equal(vm.ip,vm.entryPoints.returned);equal(string(vm,t.sp),'Child');
 set(t,parent,'',[4,3,2,1]);vm.write(count,encode(64n,4));vm.write(size,encode(4n,4));equal(api(t,'RegEnumValueW',[parent,0n,out,count,0n,size,size,size]),0n);equal(vm.readInt(size),0x01020304n);
 vm.dispose();
}

// Bind every new declaration through PE imports, then execute a real guest call.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),shell=['SHDeleteKeyA','SHDeleteKeyW'];f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);
 for(const[desc,lookup,iat,dllAt,dll]of [[0x2000,0x2040,0x20c0,0x21d0,'advapi32.dll'],[0x2014,0x2140,0x2180,0x21e0,'shlwapi.dll']]){f.rput(desc,lookup);f.rput(desc+12,dllAt);f.rput(desc+16,iat);f.rtext(dllAt,dll+'\0');}
 let at=0x2200;for(const[lookup,iat,list]of [[0x2040,0x20c0,names],[0x2140,0x2180,shell]])for(const[i,name]of list.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 const flush=0x20c0+14*f.ptr;
 if(mode===32){f.bytes.set([0x68,...encode(HKCU,4),0xff,0x15,...encode(f.preferred+BigInt(flush),4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1007);}
 else{f.bytes.set([0x48,0x83,0xec,40,0xb9,...encode(HKCU,4),0xff,0x15,...encode(BigInt(flush)-0x100fn,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);
 for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x20c0n+BigInt(i*f.ptr),f.ptr),vm.bind('advapi32.dll',name));
 for(const[i,name]of shell.entries())equal(vm.readInt(image.base+0x2180n+BigInt(i*f.ptr),f.ptr),vm.bind('shlwapi.dll',name));
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),50n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} registry enumeration/deletion/metadata/ABI/fault/JIT checks passed`);
