import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['SafeArrayCreate','SafeArrayCreateVector','SafeArrayDestroy','SafeArrayCopy','SafeArrayGetDim','SafeArrayGetElemsize','SafeArrayGetLBound','SafeArrayGetUBound','SafeArrayLock','SafeArrayUnlock','SafeArrayAccessData','SafeArrayUnaccessData','SafeArrayPutElement','SafeArrayGetElement','SafeArrayPtrOfIndex','SafeArrayGetVartype','SafeArrayCreateEx','SafeArrayCreateVectorEx'];
const ordinals=[15,411,16,27,17,18,20,19,21,22,23,24,26,25,148,77,42,43];
const api=(t,name,args=[])=>call(t,name,args,'oleaut32.dll');
const read=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const bounds=(vm,p,values)=>vm.write(p,values.flatMap(([count,lower])=>[...encode(BigInt(count),4),...encode(BigInt(lower),4)]));
const indices=(vm,p,values)=>vm.write(p,values.flatMap(n=>[...encode(BigInt(n),4)]));
const value=(t,p,vt,payload=0n)=>{const bytes=new Uint8Array(t.mode===32?16:24);bytes.set(encode(BigInt(vt),2));bytes.set(encode(payload,t.mode/8),8);t.vm.write(p,bytes);return bytes;};
const vector=(t,vt,count=3n,lower=-2n)=>api(t,'SafeArrayCreateVector',[BigInt(vt),lower,count]);
function data(t,p,out=0x24000n){equal(api(t,'SafeArrayAccessData',[p,out]),0n);const result=t.vm.readInt(out,t.mode/8);equal(api(t,'SafeArrayUnaccessData',[p]),0n);return result;}
function live(t,p){if(t.mode===64){try{t.vm.readInt(p);return true;}catch{return false;}}return call(t,'HeapValidate',[call(t,'GetProcessHeap',[],'kernel32.dll'),0n,p-4n],'kernel32.dll')===1n;}
function bstr(t,bytes=[65,0,66,0,67]){t.vm.write(0x26000n,[...bytes,0,0]);const p=api(t,'SysAllocStringLen',[0x26000n,BigInt(Math.ceil(bytes.length/2))]);t.vm.write(p-4n,encode(BigInt(bytes.length),4));return p;}
function memoryStatus(t){t.vm.write(0x28000n,encode(64n,4));equal(call(t,'GlobalMemoryStatusEx',[0x28000n],'kernel32.dll'),1n);return read(t.vm,0x28000n,64);}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,h=BigInt(mode===32?16:24),at=mode===64?0x3000020000n:0x22000n;if(mode===64)vm.map(at,4096n,3);
 for(const[i,name]of names.entries()){equal(vm.bind('oleaut32.dll','#'+ordinals[i]),vm.bind('oleaut32.dll',name));assert.throws(()=>vm.bind('ole32.dll',name),/unknown/);}
 vm.write(t.error,encode(987n,4));
 for(const[vt,size]of [[16,1],[17,1],[2,2],[11,2],[18,2],[3,4],[4,4],[10,4],[19,4],[22,4],[23,4],[5,8],[6,8],[7,8],[20,8],[21,8],[14,16],[37,width],[38,width],[8,width],[12,mode===32?16:24]]){
  const p=vector(t,vt,4n,-3n);assert.ok(p);equal(vm.readInt(p-4n),BigInt(vt));equal(vm.readInt(p,2),1n);equal(vm.readInt(p+2n,2),BigInt(0x2080+(vt===8?0x100:vt===12?0x800:0)));equal(vm.readInt(p+4n),BigInt(size));equal(api(t,'SafeArrayGetDim',[p]),1n);equal(api(t,'SafeArrayGetElemsize',[p]),BigInt(size));
  vm.write(0x24000n,new Uint8Array(8).fill(0xa5));equal(api(t,'SafeArrayGetVartype',[p,0x24000n]),0n);equal(vm.readInt(0x24000n,2),BigInt(vt));equal(vm.readInt(0x24002n,2),0xa5a5n);
  equal(api(t,'SafeArrayAccessData',[p,0x24000n]),0n);const bytes=vm.readInt(0x24000n,width);equal(bytes,p+h+8n);equal(vm.readInt(p+8n),1n);equal(read(vm,bytes,size*4),new Uint8Array(size*4));equal(api(t,'SafeArrayDestroy',[p]),0x8002000dn);equal(api(t,'SafeArrayUnaccessData',[p]),0n);equal(api(t,'SafeArrayUnlock',[p]),0x8000ffffn);
  equal(api(t,'SafeArrayGetLBound',[p,1n,0x24000n]),0n);equal(BigInt.asIntN(32,vm.readInt(0x24000n)),-3n);equal(api(t,'SafeArrayGetUBound',[p,1n,0x24000n]),0n);equal(vm.readInt(0x24000n),0n);equal(api(t,'SafeArrayDestroy',[p]),0n);equal(api(t,'SafeArrayDestroy',[p]),0x80070057n);
 }
 for(const vt of [0,1,9,13,36,0x2011])equal(vector(t,vt),0n);equal(api(t,'SafeArrayCreate',[9n,1n,0xdead0000n]),0n);equal(vector(t,17,0xffffffffn),0n);equal(api(t,'SafeArrayDestroy',[0n]),0n);equal(api(t,'SafeArrayCopy',[0n,0x24000n]),0n);equal(vm.readInt(0x24000n,width),0n);equal(api(t,'SafeArrayGetDim',[0n]),0n);
 // Input dimensions are logical order; descriptor bounds are reversed and the first dimension varies fastest.
 const shape=[[2,-2],[3,7],[4,-9]];bounds(vm,at,shape);const p=api(t,'SafeArrayCreate',[19n,3n,at]),bytes=data(t,p);assert.ok(p);for(let n=0;n<3;n++){equal(vm.readInt(p+h+BigInt(n*8)),BigInt(shape[2-n][0]));equal(BigInt.asIntN(32,vm.readInt(p+h+BigInt(n*8+4))),BigInt(shape[2-n][1]));equal(api(t,'SafeArrayGetLBound',[p,BigInt(n+1),0x24000n]),0n);equal(BigInt.asIntN(32,vm.readInt(0x24000n)),BigInt(shape[n][1]));}
 for(let k=0;k<4;k++)for(let j=0;j<3;j++)for(let i=0;i<2;i++){const n=i+2*j+6*k;indices(vm,at,[-2+i,7+j,-9+k]);vm.write(0x25000n,encode(BigInt(100+n),4));equal(api(t,'SafeArrayPutElement',[p,at,0x25000n]),0n);equal(vm.readInt(bytes+BigInt(n*4)),BigInt(100+n));equal(api(t,'SafeArrayGetElement',[p,at,0x25000n]),0n);equal(vm.readInt(0x25000n),BigInt(100+n));equal(api(t,'SafeArrayPtrOfIndex',[p,at,0x24000n]),0n);equal(vm.readInt(0x24000n,width),bytes+BigInt(n*4));}
 indices(vm,at,[-3,7,-9]);equal(api(t,'SafeArrayGetElement',[p,at,0x25000n]),0x8002000bn);for(const dim of [0n,4n]){vm.write(0x24000n,encode(0xa5a5a5a5n,4));equal(api(t,'SafeArrayGetUBound',[p,dim,0x24000n]),0x8002000bn);equal(vm.readInt(0x24000n),0xa5a5a5a5n);}
 equal(api(t,'SafeArrayCopy',[p,0x24000n]),0n);const copied=vm.readInt(0x24000n,width),copiedData=data(t,copied);assert.notEqual(copiedData,bytes);equal(read(vm,copiedData,96),read(vm,bytes,96));vm.write(bytes,[0xff]);equal(vm.readInt(copiedData),100n);equal(api(t,'SafeArrayDestroy',[p]),0n);equal(api(t,'SafeArrayDestroy',[copied]),0n);
 bounds(vm,at,[[0,-2],[3,4]]);const empty=api(t,'SafeArrayCreateEx',[17n,2n,at,t.image.base]);assert.ok(empty);equal(api(t,'SafeArrayGetUBound',[empty,1n,0x24000n]),0n);equal(BigInt.asIntN(32,vm.readInt(0x24000n)),-3n);indices(vm,at,[-2,4]);equal(api(t,'SafeArrayGetElement',[empty,at,0x25000n]),0x8002000bn);equal(api(t,'SafeArrayDestroy',[empty]),0n);
 bounds(vm,at,[[0x80000000,0],[2,0],[0,0]]);const sparseEmpty=api(t,'SafeArrayCreate',[17n,3n,at]);assert.ok(sparseEmpty);indices(vm,at,[0,0,0]);equal(api(t,'SafeArrayPtrOfIndex',[sparseEmpty,at,0x24000n]),0x8002000bn);equal(api(t,'SafeArrayDestroy',[sparseEmpty]),0n);
 const ex=api(t,'SafeArrayCreateVectorEx',[17n,-1n,2n,0xdead0000n]);assert.ok(ex);vm.write(ex+8n,encode(0xffffn,4));equal(api(t,'SafeArrayLock',[ex]),0x8000ffffn);equal(vm.readInt(ex+8n),0xffffn);equal(api(t,'SafeArrayUnlock',[ex]),0n);vm.write(ex+8n,encode(0n,4));vm.write(ex+4n,encode(8n,4));equal(api(t,'SafeArrayDestroy',[ex]),0x80070057n);vm.write(ex+4n,encode(1n,4));equal(api(t,'SafeArrayDestroy',[ex]),0n);equal(vm.readInt(t.error),987n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,p=vector(t,8),storage=data(t,p),source=bstr(t);indices(vm,0x22000n,[-2]);equal(api(t,'SafeArrayPutElement',[p,0x22000n,source]),0n);let first=vm.readInt(storage,width);assert.notEqual(first,source);equal(api(t,'SysStringByteLen',[first]),5n);equal(read(vm,first,5),Uint8Array.from([65,0,66,0,67]));
 equal(api(t,'SafeArrayGetElement',[p,0x22000n,0x24000n]),0n);const output=vm.readInt(0x24000n,width);assert.notEqual(output,first);equal(api(t,'SysStringByteLen',[output]),5n);equal(api(t,'SafeArrayPutElement',[p,0x22000n,first]),0n);equal(live(t,first),false);first=vm.readInt(storage,width);
 indices(vm,0x22000n,[-1]);equal(api(t,'SafeArrayPutElement',[p,0x22000n,0n]),0n);const zero=vm.readInt(storage+BigInt(width),width);assert.ok(zero);equal(api(t,'SysStringByteLen',[zero]),0n);
 equal(api(t,'SafeArrayCopy',[p,t.sp]),0n);const copied=vm.readInt(t.sp,width);equal(vm.ip,vm.entryPoints.returned);const copiedData=data(t,copied),next=vm.readInt(copiedData,width);assert.notEqual(next,first);equal(api(t,'SysStringByteLen',[next]),5n);api(t,'SysFreeString',[source]);equal(api(t,'SafeArrayDestroy',[p]),0n);equal(live(t,first),false);equal(live(t,next),true);equal(live(t,output),true);equal(api(t,'SafeArrayDestroy',[copied]),0n);equal(live(t,next),false);api(t,'SysFreeString',[output]);
 // A locked late child prevents every earlier element release, then a repaired graph can be destroyed.
 const parent=vector(t,12,3n,0n),parentData=data(t,parent),child=vector(t,17,3n,0n),childData=data(t,child),text=bstr(t);vm.write(childData,[3,4,5]);indices(vm,0x22000n,[0]);value(t,0x23000n,8,text);equal(api(t,'SafeArrayPutElement',[parent,0x22000n,0x23000n]),0n);const ownedText=vm.readInt(parentData+8n,width);
 indices(vm,0x22000n,[1]);value(t,0x23000n,0x2011,child);equal(api(t,'SafeArrayPutElement',[parent,0x22000n,0x23000n]),0n);const ownedChild=vm.readInt(parentData+BigInt(mode===32?24:32),width);assert.notEqual(ownedChild,child);equal(api(t,'SafeArrayDestroy',[child]),0n);equal(api(t,'SafeArrayLock',[ownedChild]),0n);equal(api(t,'SafeArrayDestroy',[parent]),0x8002000dn);equal(live(t,ownedText),true);equal(api(t,'SafeArrayUnlock',[ownedChild]),0n);
 indices(vm,0x22000n,[2]);value(t,0x23000n,0x4008,text);equal(api(t,'SafeArrayPutElement',[parent,0x22000n,0x23000n]),0n);value(t,0x23000n,0x200c,parent);api(t,'VariantInit',[0x24000n]);equal(api(t,'VariantCopy',[0x24000n,0x23000n]),0n);const clone=vm.readInt(0x24008n,width);assert.notEqual(clone,parent);equal(api(t,'VariantClear',[0x23000n]),0n);equal(vm.readInt(0x23000n,2),0n);equal(live(t,ownedText),false);equal(live(t,text),true);equal(api(t,'VariantClear',[0x24000n]),0n);equal(live(t,text),true);api(t,'SysFreeString',[text]);
 // Duplicate ownership and a cycle introduced through AccessData are rejected before any free.
 const duplicate=vector(t,8,2n,0n),duplicateData=data(t,duplicate),s=bstr(t);indices(vm,0x22000n,[0]);api(t,'SafeArrayPutElement',[duplicate,0x22000n,s]);const owned=vm.readInt(duplicateData,width);vm.write(duplicateData+BigInt(width),encode(owned,width));equal(api(t,'SafeArrayDestroy',[duplicate]),0x80070057n);equal(live(t,owned),true);vm.write(duplicateData+BigInt(width),encode(0n,width));equal(api(t,'SafeArrayDestroy',[duplicate]),0n);api(t,'SysFreeString',[s]);
 const cycle=vector(t,12,1n,0n),cycleData=data(t,cycle);value(t,cycleData,0x200c,cycle);equal(api(t,'SafeArrayDestroy',[cycle]),0x80070057n);vm.write(cycleData,new Uint8Array(mode===32?16:24));equal(api(t,'SafeArrayDestroy',[cycle]),0n);
 value(t,0x23000n,0x2008,0n);api(t,'VariantInit',[0x24000n]);equal(api(t,'VariantCopy',[0x24000n,0x23000n]),0n);equal(api(t,'VariantClear',[0x24000n]),0n);equal(api(t,'VariantClear',[0x23000n]),0n);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,p=vector(t,8,2n,0n),storage=data(t,p),s=bstr(t);indices(vm,0x22000n,[0]);api(t,'SafeArrayPutElement',[p,0x22000n,s]);const owned=vm.readInt(storage,width),cross=0x24ffen;
 for(const name of ['SafeArrayGetElement','SafeArrayCopy','SafeArrayAccessData']){
  const args=name==='SafeArrayGetElement'?[p,0x22000n,cross]:[p,cross];vm.write(cross,new Uint8Array(8).fill(0xa5));frame(t,name,args,'oleaut32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(cross,2),0xa5a5n);equal(vm.readInt(p+8n),0n);equal(live(t,owned),true);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);frame(t,name,args,'oleaut32.dll');equal(vm.service().reason,Exit.Dispatch);const result=vm.readInt(cross,width);if(name==='SafeArrayCopy')api(t,'SafeArrayDestroy',[result]);else if(name==='SafeArrayGetElement')api(t,'SysFreeString',[result]);else api(t,'SafeArrayUnaccessData',[p]);
 }
 frame(t,'SafeArrayAccessData',[p,0x24000n],'oleaut32.dll');equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p+8n),0n);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(p+8n),1n);api(t,'SafeArrayUnaccessData',[p]);
 // HRESULT failures preserve LastError even when its page is read-only.
 vm.write(t.error,encode(321n,4));frame(t,'SafeArrayUnlock',[p],'oleaut32.dll');equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x8000ffffn);equal(vm.readInt(t.error),321n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);
 // The copy must release already copied BSTRs if a later source element is invalid.
 vm.write(storage+BigInt(width),encode(0xdead0000n,width));let resources;for(let n=0;n<6;n++){frame(t,'SafeArrayCopy',[p,0x24000n],'oleaut32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(live(t,owned),true);if(n===0)resources=memoryStatus(t);}equal(memoryStatus(t),resources);vm.write(storage+BigInt(width),encode(0n,width));equal(api(t,'SafeArrayDestroy',[p]),0n);api(t,'SysFreeString',[s]);
 const arrays=[];for(let n=0;n<1024;n++){const p=vector(t,17,0n,0n);assert.ok(p);arrays.push(p);}equal(vector(t,17,0n,0n),0n);for(const p of arrays)equal(api(t,'SafeArrayDestroy',[p]),0n);const next=vector(t,17,0n,0n);assert.ok(next);equal(api(t,'SafeArrayDestroy',[next]),0n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2260);f.rput(0x2010,0x2180);f.rtext(0x2260,'oleaut32.dll\0');let at=0x2300;
 for(const[i,name]of names.entries()){const entry=i%2?(1n<<BigInt(mode-1))|BigInt(ordinals[i]):BigInt(at);f.rput(0x2080+i*f.ptr,entry,f.ptr);f.rput(0x2180+i*f.ptr,entry,f.ptr);if(i%2===0){f.rtext(at+2,name+'\0');at+=name.length+3;}}
 if(mode===32){f.bytes.set([0x6a,3,0x6a,0xfe,0x6a,17,0xff,0x15,...encode(f.preferred+0x2184n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1008);}else{f.bytes.set([0x48,0x83,0xec,40,0xb9,17,0,0,0,0xba,0xfe,0xff,0xff,0xff,0x41,0xb8,3,0,0,0,0xff,0x15,...encode(0x116en,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.length=0;}f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2180n+BigInt(i*f.ptr),f.ptr),vm.bind('oleaut32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);const p=vm.register(0);assert.ok(p);equal(vm.readInt(p,2),1n);equal(vm.readInt(p+BigInt(mode===32?16:24)),3n);assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} SAFEARRAY dimensions/ownership/variant/locks/ABI/rollback/JIT checks passed`);
