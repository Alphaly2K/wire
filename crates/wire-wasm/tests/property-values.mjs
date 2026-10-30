import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';

const api=(t,name,args=[])=>call(t,name,args,'ole32.dll');
const automation=(t,name,args=[])=>call(t,name,args,'oleaut32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const size=t=>t.mode===32?16:24,ptr=t=>t.mode/8,cp=t=>t.mode===32?12:16;
function value(t,p,vt,payload=0n,count){const b=new Uint8Array(size(t)).fill(0x55);b.set(encode(BigInt(vt),2));b.set(encode(payload,ptr(t)),8);if(count!==undefined){b.set(encode(BigInt(count),4),8);b.set(encode(payload,ptr(t)),cp(t));}t.vm.write(p,b);return b;}
function task(t,b){const p=api(t,'CoTaskMemAlloc',[BigInt(b.length)]);assert.ok(p);t.vm.write(p,b);return p;}
function bstr(t){t.vm.write(0x27000n,[65,0,0,0,66,0]);return automation(t,'SysAllocStringLen',[0x27000n,3n]);}
function freed(t,p){frame(t,'CoTaskMemFree',[p],'ole32.dll');assert.throws(()=>t.vm.service(),/invalid COM task/);}
function array(t,vt=17,count=3n){return automation(t,'SafeArrayCreateVector',[BigInt(vt),0n,count]);}
function arrayData(t,p){equal(automation(t,'SafeArrayAccessData',[p,0x28000n]),0n);const result=t.vm.readInt(0x28000n,ptr(t));equal(automation(t,'SafeArrayUnaccessData',[p]),0n);return result;}
function memoryStatus(t){t.vm.write(0x29000n,encode(64n,4));equal(call(t,'GlobalMemoryStatusEx',[0x29000n],'kernel32.dll'),1n);return bytes(t.vm,0x29000n,64);}

// Output records may be uninitialized; copying never releases their previous payload, while clear zeroes every byte.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,source=mode===32?0x22000n:0x123400000000n,out=source+0x100n;
 if(mode===64)vm.map(source,4096n,3);vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 equal(api(t,'PropVariantClear',[0n]),0n);
 for(const vt of [0,1,2,3,4,5,6,7,10,11,14,16,17,18,19,20,21,22,23,64]){
  const expected=value(t,source,vt,0xfedcba9876543210n);vm.write(out,new Uint8Array(size(t)).fill(0xa5));
  equal(api(t,'PropVariantCopy',[out,source]),0n);equal(bytes(vm,out,size(t)),expected);equal(bytes(vm,source,size(t)),expected);
  equal(api(t,'PropVariantClear',[out]),0n);equal(bytes(vm,out,size(t)),new Uint8Array(size(t)));
 }
 const previous=task(t,[65,0]);value(t,out,30,previous);value(t,source,3,73n);equal(api(t,'PropVariantCopy',[out,source]),0n);equal(api(t,'CoTaskMemFree',[previous]),0n);
 const text=bstr(t);value(t,source,8,text);equal(api(t,'PropVariantCopy',[source,source]),0n);equal(vm.readInt(source+8n,ptr(t)),text);
 equal(api(t,'PropVariantCopy',[out,source]),0n);const next=vm.readInt(out+8n,ptr(t));assert.notEqual(next,text);equal(automation(t,'SysStringByteLen',[next]),6n);equal(bytes(vm,next,6),Uint8Array.from([65,0,0,0,66,0]));
 equal(api(t,'PropVariantClear',[out]),0n);equal(api(t,'PropVariantClear',[source]),0n);
 const borrowed=bstr(t);value(t,source,0x4008,borrowed);equal(api(t,'PropVariantClear',[source]),0x80030057n);equal(bytes(vm,source,size(t)),new Uint8Array(size(t)));automation(t,'SysFreeString',[borrowed]);
 for(const vt of [9,13,66,67,68,69,73]){
  const original=value(t,source,vt,0x60000n);vm.write(out,new Uint8Array(size(t)).fill(0xa5));
  equal(api(t,'PropVariantCopy',[out,source]),0x80004001n);equal(bytes(vm,out,size(t)),new Uint8Array(size(t)).fill(0xa5));equal(api(t,'PropVariantClear',[source]),0x80004001n);equal(bytes(vm,source,size(t)),original);
  value(t,source,vt,0n);equal(api(t,'PropVariantCopy',[out,source]),0n);equal(api(t,'PropVariantClear',[out]),0n);
 }
 value(t,source,0xffff);vm.write(out,new Uint8Array(size(t)).fill(0xa5));equal(api(t,'PropVariantCopy',[out,source]),0x80020008n);equal(bytes(vm,out,size(t)),new Uint8Array(size(t)).fill(0xa5));
 equal(api(t,'PropVariantClear',[source]),0x80030057n);equal(bytes(vm,source,size(t)),new Uint8Array(size(t)));equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);equal(t.time.calls,0);vm.dispose();
}

// Task strings, binary data and GUIDs are cloned without changing encodings or sharing their owner.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,source=0x22000n,out=0x23000n;
 for(const[vt,data]of [[30,[0x81,0x40,65,0]],[31,[0x3d,0xd8,0,0xde,0,0]],[72,Array.from({length:16},(_,i)=>i)],[65,[1,0,2,0,3]],[70,[9,8,7]]]){
  const p=task(t,data);value(t,source,vt,p,vt===65||vt===70?data.length:undefined);equal(api(t,'PropVariantCopy',[out,source]),0n);
  const next=vm.readInt(out+BigInt(vt===65||vt===70?cp(t):8),ptr(t));assert.notEqual(next,p);equal(bytes(vm,next,data.length),Uint8Array.from(data));vm.write(p,[0xff]);equal(vm.readInt(next,1),BigInt(data[0]));
  equal(api(t,'PropVariantClear',[out]),0n);freed(t,next);equal(api(t,'PropVariantClear',[source]),0n);freed(t,p);
 }
 for(const[vt,unit]of [[16,1],[2,2],[3,4],[5,8],[72,16]]){
  const data=Uint8Array.from({length:unit*3},(_,i)=>(i*7+3)&255),p=task(t,data);value(t,source,0x1000|vt,p,3);equal(api(t,'PropVariantCopy',[out,source]),0n);
  const next=vm.readInt(out+BigInt(cp(t)),ptr(t));assert.notEqual(next,p);equal(bytes(vm,next,data.length),data);equal(api(t,'PropVariantClear',[out]),0n);equal(api(t,'PropVariantClear',[source]),0n);
 }
 // A CLIPDATA count includes its four-byte format; both storage blocks get independent owners.
 const payload=task(t,[3,1,4,1,5]),clip=new Uint8Array(8+ptr(t));clip.set(encode(9n,4));clip.set(encode(13n,4),4);clip.set(encode(payload,ptr(t)),8);const p=task(t,clip);value(t,source,71,p);equal(api(t,'PropVariantCopy',[out,source]),0n);
 const next=vm.readInt(out+8n,ptr(t)),nextData=vm.readInt(next+8n,ptr(t));assert.notEqual(next,p);assert.notEqual(nextData,payload);equal(vm.readInt(next),9n);equal(bytes(vm,nextData,5),Uint8Array.from([3,1,4,1,5]));equal(api(t,'PropVariantClear',[out]),0n);freed(t,next);freed(t,nextData);equal(api(t,'PropVariantClear',[source]),0n);
 const clips=new Uint8Array((8+ptr(t))*2),owners=[];
 for(let n=0;n<2;n++){const data=task(t,[n+7,n+8]);owners.push(data);clips.set(encode(6n,4),n*(8+ptr(t)));clips.set(encode(BigInt(n+1),4),n*(8+ptr(t))+4);clips.set(encode(data,ptr(t)),n*(8+ptr(t))+8);}
 const vector=task(t,clips);value(t,source,0x1047,vector,2);equal(api(t,'PropVariantCopy',[out,source]),0n);const copied=vm.readInt(out+BigInt(cp(t)),ptr(t));
 for(let n=0;n<2;n++){const child=vm.readInt(copied+BigInt(n*(8+ptr(t))+8),ptr(t));assert.notEqual(child,owners[n]);equal(bytes(vm,child,2),Uint8Array.from([n+7,n+8]));}
 equal(api(t,'PropVariantClear',[out]),0n);equal(api(t,'PropVariantClear',[source]),0n);for(const owner of owners)freed(t,owner);
 value(t,source,65,1n,16*1024*1024+1);vm.write(out,new Uint8Array(size(t)).fill(0xa5));equal(api(t,'PropVariantCopy',[out,source]),0x8007000en);equal(bytes(vm,out,size(t)),new Uint8Array(size(t)).fill(0xa5));
 value(t,source,0x1048,1n,0xffffffff);equal(api(t,'PropVariantCopy',[out,source]),0x8007000en);equal(t.time.calls,0);vm.dispose();
}

// Composite vectors deep-copy BSTRs/strings, and recursive properties share the existing SAFEARRAY owner.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,source=0x22000n,out=0x23000n;
 for(const vt of [8,30,31]){
  const p=vt===8?bstr(t):task(t,vt===30?[65,0]:[65,0,0,0]),root=task(t,[...encode(p,ptr(t)),...encode(0n,ptr(t))]);value(t,source,0x1000|vt,root,2);equal(api(t,'PropVariantCopy',[out,source]),0n);
  const next=vm.readInt(out+BigInt(cp(t)),ptr(t)),child=vm.readInt(next,ptr(t));assert.notEqual(next,root);assert.notEqual(child,p);if(vt===8)equal(automation(t,'SysStringByteLen',[child]),6n);else equal(vm.readInt(child,1),65n);
  equal(api(t,'PropVariantClear',[out]),0n);equal(api(t,'PropVariantClear',[source]),0n);
 }
 const p=task(t,[65,0]),text=bstr(t),a=array(t),storage=arrayData(t,a);vm.write(storage,[3,4,5]);
 const records=new Uint8Array(size(t)*3);value(t,0x25000n,30,p);records.set(bytes(vm,0x25000n,size(t)));value(t,0x25000n,8,text);records.set(bytes(vm,0x25000n,size(t)),size(t));value(t,0x25000n,0x2011,a);records.set(bytes(vm,0x25000n,size(t)),size(t)*2);
 const root=task(t,records);value(t,source,0x100c,root,3);equal(api(t,'PropVariantCopy',[out,source]),0n);
 const copied=vm.readInt(out+BigInt(cp(t)),ptr(t)),copiedArray=vm.readInt(copied+BigInt(size(t)*2+8),ptr(t));assert.notEqual(copiedArray,a);equal(bytes(vm,arrayData(t,copiedArray),3),Uint8Array.from([3,4,5]));
 equal(automation(t,'SafeArrayLock',[a]),0n);const original=bytes(vm,source,size(t));equal(api(t,'PropVariantClear',[source]),0x8002000dn);equal(bytes(vm,source,size(t)),original);
 equal(automation(t,'SafeArrayUnlock',[a]),0n);equal(api(t,'PropVariantClear',[source]),0n);freed(t,p);freed(t,root);equal(api(t,'PropVariantClear',[out]),0n);
 // Duplicate ownership and cycles reject destruction before any member is freed.
 const child=task(t,[66,0]),duplicate=task(t,[...encode(child,ptr(t)),...encode(child,ptr(t))]);value(t,source,0x101e,duplicate,2);equal(api(t,'PropVariantClear',[source]),0x80070057n);vm.write(duplicate+BigInt(ptr(t)),encode(0n,ptr(t)));equal(api(t,'PropVariantClear',[source]),0n);
 const cycle=task(t,new Uint8Array(size(t)));value(t,cycle,0x100c,cycle,1);value(t,source,0x100c,cycle,1);equal(api(t,'PropVariantClear',[source]),0x80070057n);equal(api(t,'PropVariantCopy',[out,source]),0x8007000en);vm.write(cycle,new Uint8Array(size(t)));equal(api(t,'PropVariantClear',[source]),0n);vm.dispose();
}

// A late failure must release already cloned task/BSTR/array members without publishing a partial destination.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,source=0x22000n,out=0x23000n,p=task(t,[65,0]),text=bstr(t),a=array(t),root=task(t,new Uint8Array(size(t)*4));value(t,root,30,p);value(t,root+BigInt(size(t)),8,text);value(t,root+BigInt(size(t)*2),0x2011,a);value(t,root+BigInt(size(t)*3),13,0x60000n);value(t,source,0x100c,root,4);vm.write(out,new Uint8Array(size(t)).fill(0xa5));
 const taskSlot=api(t,'CoTaskMemAlloc',[2n]);api(t,'CoTaskMemFree',[taskSlot]);const stringSlot=bstr(t);automation(t,'SysFreeString',[stringSlot]);
 let baseline;for(let n=0;n<4;n++){equal(api(t,'PropVariantCopy',[out,source]),0x80004001n);equal(bytes(vm,out,size(t)),new Uint8Array(size(t)).fill(0xa5));const reused=api(t,'CoTaskMemAlloc',[2n]);equal(reused,taskSlot);api(t,'CoTaskMemFree',[reused]);const reusedString=bstr(t);if(mode===32)equal(reusedString,stringSlot);automation(t,'SysFreeString',[reusedString]);if(n===0)baseline=memoryStatus(t);else equal(memoryStatus(t),baseline);}
 vm.write(root+BigInt(size(t)*3),new Uint8Array(size(t)));equal(api(t,'PropVariantClear',[source]),0n);
 vm.dispose();
}

// Full-record output and return faults precede mutation or free; HRESULT errors never need a writable LastError.
for(const mode of [32,64])for(const name of ['PropVariantCopy','PropVariantClear'])for(const kind of ['output','return']){
 const t=await machine(mode),{vm}=t,p=task(t,[65,0]),source=0x22000n,out=0x24ff8n;value(t,source,30,p);value(t,out,30,p);const old=bytes(vm,out,size(t));
 frame(t,name,name.endsWith('Copy')?[out,source]:[out],'ole32.dll');const page=kind==='output'?0x25000n:t.sp&~4095n,before=snapshot(t),work=vm.work;
 equal(core.wire_protect(t.handle,page,4096n,kind==='output'?2:1),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(vm,out,size(t)),old);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 if(name.endsWith('Copy')){equal(api(t,'PropVariantClear',[out]),0n);equal(api(t,'PropVariantClear',[source]),0n);}else freed(t,p);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=task(t,new Uint8Array(64));value(t,p,30,p);const old=bytes(vm,p,size(t));equal(api(t,'PropVariantClear',[p]),0x80070057n);equal(bytes(vm,p,size(t)),old);api(t,'CoTaskMemFree',[p]);
 vm.write(0x4fffcn,[65,0,0,0]);value(t,0x22000n,31,0x4fffcn);equal(api(t,'PropVariantCopy',[0x23000n,0x22000n]),0n);equal(api(t,'PropVariantClear',[0x23000n]),0n);
 vm.write(0x24fffn,[65,0,0,0]);value(t,0x22000n,31,0x24fffn);vm.write(0x23000n,new Uint8Array(size(t)).fill(0xa5));frame(t,'PropVariantCopy',[0x23000n,0x22000n],'ole32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,0x23000n,size(t)),new Uint8Array(size(t)).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(api(t,'PropVariantClear',[0x23000n]),0n);
 value(t,0x22000n,13,1n);vm.write(t.error,encode(77n,4));equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal(api(t,'PropVariantClear',[0x22000n]),0x80004001n);equal(vm.readInt(t.error),77n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);vm.dispose();
}

const added=['PropVariantClear','PropVariantCopy','CoCreateFreeThreadedMarshaler','CoSetProxyBlanket','RoGetAgileReference'];
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=mode===32?0x23000n:0x123400000000n;if(mode===64)vm.map(out,4096n,3);vm.write(t.error,encode(77n,4));
 for(const dll of ['ole32.dll','combase.dll'])for(const name of added){assert.ok(vm.bind(dll,name));assert.throws(()=>vm.bind('oleaut32.dll',name),/unknown/);}
 for(const name of added)equal(vm.bind(name==='RoGetAgileReference'?'api-ms-win-core-com-l1-1-1.dll':'api-ms-win-core-com-l1-1-0.dll',name),vm.bind('ole32.dll',name));
 for(const[name,args]of [['CoCreateFreeThreadedMarshaler',[0x60000n,out]],['RoGetAgileReference',[0n,0x60000n,0x60000n,out]]]){
  vm.write(out,new Uint8Array(16).fill(0xa5));equal(api(t,name,args),0x80004001n);equal(vm.readInt(out,ptr(t)),0n);equal(vm.readInt(out+BigInt(ptr(t)),1),0xa5n);
  const a=args.slice();a[a.length-1]=0n;equal(api(t,name,a),0x80004003n);a[a.length-1]=t.sp;equal(api(t,name,a),0x80004001n);equal(vm.ip,vm.entryPoints.returned);
 }
 equal(api(t,'RoGetAgileReference',[2n,1n,1n,out]),0x80070057n);equal(api(t,'CoSetProxyBlanket',[0n,0n,0n,0n,0n,0n,0n,0n]),0x80070057n);equal(api(t,'CoSetProxyBlanket',Array(8).fill(0x60000n)),0x80004001n);equal(vm.readInt(t.error),77n);equal(t.time.calls,0);vm.dispose();
}

// Both canonical DLLs bind the complete group; the guest executes a real PropVariantCopy import.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);let at=0x2280;
 for(const[n,dll]of ['ole32.dll','combase.dll'].entries()){
  const descriptor=0x2000+n*20,lookup=0x2080+n*0x100,iat=0x2100+n*0x100;f.rput(descriptor,lookup);f.rput(descriptor+12,at);f.rtext(at,dll+'\0');at+=dll.length+1;f.rput(descriptor+16,iat);
  for(const[i,name]of added.entries()){f.rput(lookup+i*f.ptr,at,f.ptr);f.rput(iat+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 }
 assert.ok(at<0x2600);f.bytes.fill(0x55,f.offset(0x3100),f.offset(0x3140));f.rput(0x3100,19,2);f.rput(0x3108,0x76543210);const b=[],iat=0x2100+f.ptr;f.relocations.length=0;
 if(mode===32){b.push(0x68,...encode(f.preferred+0x3100n,4),0x68,...encode(f.preferred+0x3120n,4),0xff,0x15,...encode(f.preferred+BigInt(iat),4),0xc3);f.relocations.push(0x1001,0x1006,0x100c);}
 else{b.push(0x48,0x83,0xec,40,0x48,0x8d,0x0d,...encode(0x3120n-0x100bn,4),0x48,0x8d,0x15,...encode(0x3100n-0x1012n,4),0xff,0x15);b.push(...encode(BigInt(iat-0x1000-b.length-4),4),0x48,0x83,0xc4,40,0xc3);}
 f.bytes.set(b,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);
 for(const[n,dll]of ['ole32.dll','combase.dll'].entries())for(const[i,name]of added.entries())equal(vm.readInt(image.base+BigInt(0x2100+n*0x100+i*f.ptr),f.ptr),vm.bind(dll,name));
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(image.base+0x3120n,2),19n);equal(vm.readInt(image.base+0x3128n),0x76543210n);vm.dispose();
}
console.log(`${checks} PROPVARIANT ownership/vector/array/rollback/COM/ABI/JIT/fault checks passed`);
