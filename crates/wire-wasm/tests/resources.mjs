import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const kernel=['FindResourceA','FindResourceW','FindResourceExA','FindResourceExW','LoadResource','SizeofResource','LockResource','FreeResource'];
const user=['LoadStringA','LoadStringW'];
const dll=name=>user.includes(name)?'user32.dll':'kernel32.dll';
const api=(t,name,args)=>call(t,name,args,dll(name));
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
function resourceFixture(mode){
 const f=fixture(mode,{tls:false}),root=0x2300,descriptors=new Map();let cursor=0;
 f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0,f.offset(root),f.offset(0x2600));
 f.bytes[f.offset(0x1000)]=0xc3;f.put(f.sections+40+36,0xc0000040);
 const p=(offset,value,size=4)=>f.rput(root+offset,value,size),allocate=size=>{const at=cursor;cursor+=size;return at;};
 function node(entries,path=[]){
  const at=allocate(16+entries.length*8);p(at+12,entries.filter(([key])=>typeof key==='string').length,2);p(at+14,entries.filter(([key])=>typeof key!=='string').length,2);
  for(const[n,[key,value]]of entries.entries()){
   let name=key;if(typeof key==='string'){const units=Array.from({length:key.length},(_,i)=>key.charCodeAt(i)),offset=allocate(2+units.length*2);p(offset,units.length,2);units.forEach((unit,n)=>p(offset+2+n*2,unit,2));name=0x80000000+offset;}
   p(at+16+n*8,name);let target;
   if(Array.isArray(value))target=0x80000000+node(value,[...path,key]);
   else{target=allocate(16);p(target,value.rva);p(target+4,value.size);descriptors.set([...path,key].join('/'),root+target);}
   p(at+20+n*8,target);
  }return at;
 }
 const data=(rva,size)=>({rva,size}),langs=[[0x411,data(0x3100,4)],[0x409,data(0x3110,3)],[0x1000409,data(0x3120,2)]];
 node([['Type',[['日本',langs],[7,langs]]],[6,[[1,[[0x411,data(0x3000,46)],[0x409,data(0x3080,42)],[0x1000409,data(0x30c0,40)]]]]],[10,[[7,langs],[8,[[0x411,data(0x3130,0)]]]]]]);
 assert.ok(cursor<=0x300);f.put(f.dirs+16,root);f.put(f.dirs+20,cursor);
 const block=(rva,slots)=>{let at=rva;for(let i=0;i<16;i++){const units=slots[i]??[];f.rput(at,units.length,2);at+=2;for(const unit of units){f.rput(at,unit,2);at+=2;}}};
 block(0x3000,[[0x65e5,0x672c],[],[0x41,0,0x42],[0xd800,0x42]]);block(0x3080,[[79,116,104,101,114]]);block(0x30c0,[[70,117,108,108]]);
 f.bytes.set([1,2,3,4],f.offset(0x3100));f.bytes.set([5,6,7],f.offset(0x3110));f.bytes.set([8,9],f.offset(0x3120));
 return{...f,root,resourceSize:cursor,descriptors};
}
async function resources(mode,{base}={}){
 let handle;const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){throw new Error('unexpected clock');},wallClock(){throw new Error('unexpected wall clock');}}),f=resourceFixture(mode);
 const image=vm.load(f.bytes,{base});equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);
 return{vm,mode,handle,image,f,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68)};
}

// Numeric/named trees, full language carriers and opaque views use the loaded image.
for(const mode of [32,64]){
 const t=await resources(mode,{base:mode===64?0x123400400000n:undefined}),{vm,image}=t;
 for(const name of [...kernel,...user]){assert.ok(vm.bind(dll(name),name));for(const wrong of (user.includes(name)?['kernel32.dll','kernelbase.dll']:['user32.dll']))assert.throws(()=>vm.bind(wrong,name),/unknown/);if(kernel.includes(name))equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));}
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 const found=api(t,'FindResourceW',[0n,7n,10n]);assert.ok(found);equal(api(t,'FindResourceA',[image.base,7n,10n]),found);equal(api(t,'FindResourceExW',[0n,10n,7n,0x777n]),found);
 const loaded=api(t,'LoadResource',[0n,found]);assert.notEqual(loaded,found);equal(api(t,'LoadResource',[image.base,found]),loaded);equal(api(t,'SizeofResource',[0n,found]),4n);equal(api(t,'LockResource',[loaded]),image.base+0x3100n);equal(bytes(vm,api(t,'LockResource',[loaded]),4),Uint8Array.from([1,2,3,4]));
 for(const[language,rva,size]of [[0x409n,0x3110n,3n],[0x1000409n,0x3120n,2n]]){const resource=api(t,'FindResourceExA',[image.base,10n,7n,language]);assert.notEqual(resource,found);equal(api(t,'SizeofResource',[image.base,resource]),size);equal(api(t,'LockResource',[api(t,'LoadResource',[image.base,resource])]),image.base+rva);}
 const high=mode===64?0x123500000000n:0x22000n;if(mode===64)vm.map(high,4096n,3);
 text(vm,high,'type',true);text(vm,high+32n,'日本',true);const named=api(t,'FindResourceW',[image.base,high+32n,high]);assert.ok(named);
 text(vm,high,'TYPE');vm.write(high+32n,[0x93,0xfa,0x96,0x7b,0]);equal(api(t,'FindResourceA',[0n,high+32n,high]),named);equal(api(t,'SizeofResource',[0n,named]),4n);
 const numbered=api(t,'FindResourceA',[0n,7n,high]);assert.ok(numbered);text(vm,high,'type',true);equal(api(t,'FindResourceW',[0n,7n,high]),numbered);assert.notEqual(numbered,named);
 const empty=api(t,'FindResourceW',[0n,8n,10n]);equal(api(t,'SizeofResource',[0n,empty]),0n);equal(api(t,'LockResource',[api(t,'LoadResource',[0n,empty])]),image.base+0x3130n);
 equal(api(t,'FreeResource',[loaded]),0n);equal(api(t,'LockResource',[loaded]),image.base+0x3100n);equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);
 for(const[name,args,error]of [['FindResourceA',[0n,99n,10n],1814n],['FindResourceW',[123n,7n,10n],6n],['LoadResource',[0n,loaded],6n],['SizeofResource',[0n,loaded],6n],['LockResource',[found],6n],['LoadResource',[123n,found],6n]]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);}
 if(mode===64){for(const[name,args]of [['FindResourceW',[image.base&0xffffffffn,7n,10n]],['LoadResource',[0n,found&0xffffffffn]],['LockResource',[loaded&0xffffffffn]]]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),6n);}}
 // A resource view observes edits; LockResource itself does not read or copy its payload.
 vm.write(image.base+0x3100n,[9]);equal(vm.readInt(api(t,'LockResource',[loaded]),1),9n);
 equal(core.wire_protect(t.handle,image.base+0x3000n,4096n,1),0);equal(api(t,'LockResource',[loaded]),image.base+0x3100n);equal(core.wire_protect(t.handle,image.base+0x3000n,4096n,4),0);
 vm.dispose();
}

// Length-prefixed strings preserve raw UTF-16 and CP932 byte truncation.
for(const mode of [32,64]){
 const t=await resources(mode),{vm,image}=t,out=mode===64?0x123500000000n:0x22000n;if(mode===64)vm.map(out,4096n,3);
 vm.write(t.error,encode(77n,4));
 for(const[name,id,count,want,ret]of [
  ['LoadStringW',0x10000n,2n,[0xe5,0x65,0,0],1n],['LoadStringW',0n,4n,[0xe5,0x65,0x2c,0x67,0,0],2n],
  ['LoadStringA',0n,2n,[0x93,0],1n],['LoadStringA',0n,8n,[0x93,0xfa,0x96,0x7b,0],4n],
  ['LoadStringW',2n,8n,[65,0,0,0,66,0,0,0],3n],['LoadStringA',2n,8n,[65,0,66,0],3n],
  ['LoadStringW',3n,8n,[0,0xd8,66,0,0,0],2n],['LoadStringA',3n,8n,[63,66,0],2n],
  ['LoadStringA',1n,8n,[0],0n],['LoadStringW',16n,8n,[0,0],0n],['LoadStringW',0n,1n,[0,0],0n]
 ]){vm.write(out,new Uint8Array(32).fill(0xa5));equal(api(t,name,[0n,id,out,count]),ret);equal(bytes(vm,out,want.length),Uint8Array.from(want));equal(vm.readInt(out+BigInt(want.length),1),0xa5n);}
 for(const[id,pointer,length]of [[0n,image.base+0x3002n,2n],[1n,image.base+0x3008n,0n],[16n,0n,0n]]){equal(api(t,'LoadStringW',[0n,id,out,0n]),length);equal(vm.readInt(out,mode/8),pointer);}
 for(const[name,count]of [['LoadStringA',0n],['LoadStringA',0xffffffffn],['LoadStringW',0xffffffffn]])equal(api(t,name,[123n,0n,0x60000n,count]),0n);
 equal(api(t,'LoadStringW',[0n,0n,t.sp,0n]),2n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,mode/8),image.base+0x3002n);equal(vm.readInt(t.error),77n);
 // Source/output overlap uses the original string before publishing any bytes.
 equal(api(t,'LoadStringW',[0n,0n,image.base+0x3000n,3n]),2n);equal(bytes(vm,image.base+0x3000n,6),Uint8Array.from([0xe5,0x65,0x2c,0x67,0,0]));vm.dispose();
}

// Failed output, source, error and return access leaves the service retryable.
for(const mode of [32,64])for(const kind of ['output','source','error','return']){
 const t=await resources(mode),{vm,image}=t,out=0x24ffen;
 vm.write(out,new Uint8Array(12).fill(0xa5));const name=kind==='error'?'LockResource':kind==='return'?'FindResourceW':'LoadStringW',args=kind==='error'?[123n]:kind==='return'?[0n,7n,10n]:[0n,0n,out,3n];
 frame(t,name,args,dll(name));const before=snapshot(t),work=vm.work;
 const page=kind==='output'?0x25000n:kind==='source'?image.base+0x3000n:kind==='error'?image.teb:t.sp&~4095n;
 equal(core.wire_protect(t.handle,page,4096n,kind==='error'?2:1),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(out,2),0xa5a5n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);
 if(kind==='output'||kind==='source')equal(bytes(vm,out,6),Uint8Array.from([0xe5,0x65,0x2c,0x67,0,0]));
 if(kind==='return')equal(vm.register(0),mode===32?0x46000000n:0x7ffd06000000n);
 vm.dispose();
}

// Malformed metadata fails explicitly without publishing a resource identity or output.
for(const mode of [32,64])for(const kind of ['tree','descriptor','payload','length','name']){
 const t=await resources(mode),{vm,image,f}=t,root=image.base+BigInt(f.root),desc=image.base+BigInt(f.descriptors.get('10/7/1041'));
 const edits={tree:[root+14n,65535n,2],descriptor:[root+20n+16n,BigInt(f.resourceSize),4],payload:[desc,0x4fffn,4],length:[image.base+0x3000n,65535n,2],name:[root+16n,0x80000300n,4]};
 const[p,value,size]=edits[kind],old=vm.readInt(p,size);vm.write(p,encode(value,size));vm.write(0x22000n,new Uint8Array(16).fill(0xa5));
 const name=kind==='length'?'LoadStringW':kind==='name'?'FindResourceW':kind==='payload'?'LoadResource':'FindResourceW';
 let args=kind==='length'?[0n,0n,0x22000n,8n]:[0n,7n,10n];
 if(kind==='name'){text(vm,0x22080n,'Type',true);text(vm,0x220a0n,'日本',true);args=[0n,0x220a0n,0x22080n];}
 if(kind==='payload')args=[0n,api(t,'FindResourceW',[0n,7n,10n])];
 equal(api(t,name,args),0n);equal(vm.readInt(t.error),13n);equal(vm.readInt(0x22000n),0xa5a5a5a5n);vm.write(p,encode(old,size));assert.ok(api(t,'FindResourceW',[0n,7n,10n]));vm.dispose();
}

// DLL retirement invalidates both kinds of opaque view before same-base replacement.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,f=resourceFixture(mode);f.put(0x96,0x2102,2);f.bytes.set([0xb8,1,0,0,0,...(mode===32?[0xc2,12,0]:[0xc3])],f.offset(0x1000));
 const load=async name=>{vm.supplyModule(name,f.bytes);text(vm,0x20000n,name);frame(t,'LoadLibraryA',[0x20000n],'kernel32.dll');return finish(t);};
 const base=await load('resources.dll');assert.ok(base);const found=api(t,'FindResourceW',[base,7n,10n]),loaded=api(t,'LoadResource',[base,found]);
 equal(api(t,'LoadStringW',[base,0n,0x22000n,0n]),2n);equal(vm.readInt(0x22000n,mode/8),base+0x3002n);
 equal(api(t,'LoadResource',[0n,found]),0n);equal(vm.readInt(t.error),6n);
 frame(t,'FreeLibrary',[base],'kernel32.dll');equal(await finish(t),1n);
 for(const[name,args]of [['LoadResource',[base,found]],['SizeofResource',[base,found]],['LockResource',[loaded]]]){equal(api(t,name,args),0n);equal(vm.readInt(t.error),6n);}
 f.rput(0x3100,0x44332211);equal(await load('resources-replacement.dll'),base);
 equal(api(t,'LoadResource',[base,found]),0n);equal(api(t,'LockResource',[loaded]),0n);const fresh=api(t,'FindResourceW',[base,7n,10n]);assert.notEqual(fresh,found);const data=api(t,'LoadResource',[base,fresh]);assert.notEqual(data,loaded);equal(vm.readInt(api(t,'LockResource',[data])),0x44332211n);vm.dispose();
}

// All ten exports bind as static imports; a generated entry calls through the JIT.
for(const mode of [32,64]){
 const f=resourceFixture(mode);f.put(f.dirs+8,0x2000);f.put(f.dirs+12,60);f.bytes.fill(0,f.offset(0x2000),f.offset(0x2300));let string=0x21d0;
 for(const[n,[provider,names,lookup,iat]]of [['kernel32.dll',kernel,0x2080,0x2100],['user32.dll',user,0x2180,0x21a0]].entries()){
  const descriptor=0x2000+n*20;f.rput(descriptor,lookup);f.rput(descriptor+12,string);f.rtext(string,provider+'\0');string+=provider.length+1;f.rput(descriptor+16,iat);
  for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,string,f.ptr);f.rput(iat+i*f.ptr,string,f.ptr);f.rtext(string+2,name+'\0');string+=name.length+3;}
 }
 assert.ok(string<0x2300);
 const code=mode===32?[0x6a,8,0x68,...encode(f.preferred+0x3180n,4),0x6a,0,0x6a,0,0xff,0x15,...encode(f.preferred+0x21a0n+BigInt(f.ptr),4),0xc3]:[0x48,0x83,0xec,0x28,0x31,0xc9,0x31,0xd2,0x4c,0x8d,0x05,...encode(0x3180n-0x100fn,4),0x41,0xb9,8,0,0,0,0xff,0x15,...encode(0x21a0n+BigInt(f.ptr)-0x101bn,4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1003,0x100d]:[]));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);
 for(const[names,iat]of [[kernel,0x2100],[user,0x21a0]])for(const[i,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+i*f.ptr),f.ptr),vm.bind(dll(name),name));
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),2n);equal(bytes(vm,image.base+0x3180n,6),Uint8Array.from([0xe5,0x65,0x2c,0x67,0,0]));assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} resource tree/string/view/ABI/lifetime/JIT/fault checks passed`);
