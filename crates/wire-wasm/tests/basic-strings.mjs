import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['CompareStringOrdinal','lstrcmpA','lstrcmpW','lstrcmpiA','lstrcmpiW','lstrlenA','lstrlen','lstrlenW','lstrcpyA','lstrcpyW','lstrcatA','lstrcatW','lstrcpynA','lstrcpynW'];
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const p=0x22000n,q=0x24000n;
const units=(vm,p,a)=>vm.write(p,[...a,0].flatMap(n=>Array.from(encode(BigInt(n),2))));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of names){assert.ok(vm.bind('kernel32.dll',name));equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 vm.write(t.error,encode(123n,4));vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x72));
 for(const name of ['lstrlen','lstrlenA','lstrlenW'])equal(api(t,name,[0n]),0n);
 vm.write(p,[0xff,0x83,0x5c,0]);equal(api(t,'lstrlenA',[p]),3n);equal(api(t,'lstrlen',[p]),3n);equal(api(t,'lstrcpyA',[q,p]),q);equal(bytes(vm,q,4),[0xff,0x83,0x5c,0]);
 units(vm,p,[0xd800,0x3042,0xdc00]);equal(api(t,'lstrlenW',[p]),3n);equal(api(t,'lstrcpyW',[q,p]),q);equal(bytes(vm,q,8),bytes(vm,p,8));
 text(vm,p,'abc');equal(api(t,'lstrcpyA',[p+1n,p]),p+1n);equal(bytes(vm,p,5),[97,97,98,99,0]);
 text(vm,p,'ab',true);equal(api(t,'lstrcatW',[p,p]),p);equal(bytes(vm,p,10),Array.from(Buffer.from('abab\0','utf16le')));
 for(const suffix of ['A','W']){
  const unit=suffix==='W'?2:1;vm.write(q,new Uint8Array(16).fill(0xa5));
  for(const count of [0n,-1n,0x80000000n]){equal(api(t,'lstrcpyn'+suffix,[0x60000n,0x60000n,count]),0x60000n);equal(bytes(vm,q,16),new Array(16).fill(0xa5));}
  equal(api(t,'lstrcpyn'+suffix,[q,0n,1n]),q);equal(bytes(vm,q,unit),new Array(unit).fill(0));equal(vm.readInt(q+BigInt(unit),1),0xa5n);
  text(vm,p,'abcd',unit===2);equal(api(t,'lstrcpyn'+suffix,[q,p,3n]),q);equal(bytes(vm,q,unit*3),Array.from(Buffer.from('ab\0',unit===2?'utf16le':'ascii')));
  const edge=0x50000n-BigInt(unit);vm.write(edge,encode(0x78n,unit));equal(api(t,'lstrcpyn'+suffix,[q,edge,2n]),q);equal(vm.readInt(q,unit),0x78n);equal(vm.readInt(q+BigInt(unit),unit),0n);
 }
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x72));equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Comparison preserves raw UTF-16 units, embedded NULs and single-unit case mapping.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const [left,right,result,insensitive]of [['ab','abc',-1,false],['ß','SS',1,true],['a\ud800','A\ud800',0,true],['\ud800','\ud801',-1,false],['','',0,true]]){
  text(vm,p,left,true);text(vm,q,right,true);equal(api(t,insensitive?'lstrcmpiW':'lstrcmpW',[p,q]),BigInt.asUintN(32,BigInt(result)));
  equal(api(t,'CompareStringOrdinal',[p,BigInt(left.length),q,BigInt(right.length),insensitive?1n:0n]),BigInt(result+2));
 }
 equal(api(t,'lstrcmpA',[0n,0n]),0n);vm.write(p,[0x83,0x5c,0]);vm.write(q,[65,0]);equal(api(t,'lstrcmpA',[p,q]),1n);
 text(vm,p,'aBc');text(vm,q,'AbC');equal(api(t,'lstrcmpiA',[p,q]),0n);equal(api(t,'lstrcmpA',[p,q]),1n);
 units(vm,p,[65,0,122]);units(vm,q,[65,0,97]);equal(api(t,'CompareStringOrdinal',[p,3n,q,3n,0n]),3n);equal(api(t,'CompareStringOrdinal',[p,-1n,q,-1n,0n]),2n);equal(api(t,'CompareStringOrdinal',[0x60000n,0n,0x60000n,0n,0n]),2n);
 const invalid=(1n<<BigInt(mode))-1n;equal(api(t,'CompareStringOrdinal',[invalid,0n,invalid,0n,0n]),2n);
 for(const args of [[0n,0n,q,0n,0n],[p,-2n,q,0n,0n],[p,0x100001n,q,0n,0n]]){equal(api(t,'CompareStringOrdinal',args),0n);equal(vm.readInt(t.error),87n);}
 // An unterminated comparison cannot read beyond its bounded text contract.
 vm.map(0x1000000n,65536n,7);vm.write(0x1000000n,new Uint8Array(65536).fill(65));equal(api(t,'lstrcmpW',[0x1000000n,q]),0n);equal(vm.readInt(t.error),87n);vm.dispose();
}

// Source/output/error/return faults preserve the original caller and output prefix.
for(const mode of [32,64])for(const kind of ['source','destination','concat','error','return']){
 const t=await machine(mode),{vm}=t;units(vm,p,[65,66,67]);vm.write(q,new Uint8Array(16).fill(0xa5));let page=0x25000n,protection=2;
 if(kind==='source'){frame(t,'lstrcpyW',[q,0x4ffffn],'kernel32.dll');page=0x50000n;protection=null;}
 if(kind==='destination')frame(t,'lstrcpyW',[0x24ffen,p],'kernel32.dll');
 if(kind==='concat'){text(vm,0x24ffcn,'a',true);frame(t,'lstrcatW',[0x24ffcn,p],'kernel32.dll');}
 if(kind==='error'){frame(t,'CompareStringOrdinal',[0n,0n,q,0n,0n],'kernel32.dll');page=t.image.teb;}
 if(kind==='return'){frame(t,'lstrcpyW',[q,p],'kernel32.dll');page=t.sp&~4095n;protection=1;}
 if(protection!==null)equal(core.wire_protect(t.handle,page,4096n,protection),0);
 const before=snapshot(t),output=bytes(vm,q,16),prefix=bytes(vm,0x24ffcn,4);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,q,16),output);equal(bytes(vm,0x24ffcn,4),prefix);
 if(kind==='source'){vm.map(0x50000n,4096n,7);vm.write(0x4ffffn,[0,0]);}else equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='error'?0n:kind==='destination'?0x24ffen:kind==='concat'?0x24ffcn:q);vm.dispose();
}

// Copying onto the return slot uses the saved return; complete high pointers survive every operation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;units(vm,p,[88]);equal(api(t,'lstrcpyW',[t.sp,p]),t.sp);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,2),88n);equal(vm.register(4),t.sp+BigInt(mode===32?12:8));
 if(mode===64){const high=0x123400000000n;vm.map(high,8192n,7);text(vm,high,'high');equal(api(t,'lstrlenA',[high]),4n);equal(api(t,'lstrcpyA',[high+4096n,high]),high+4096n);equal(api(t,'lstrcatA',[high+4096n,high]),high+4096n);equal(api(t,'lstrlenA',[high+4096n]),8n);equal(api(t,'lstrcpynA',[high+4096n,high,3n]),high+4096n);equal(api(t,'lstrcmpA',[high,high+4096n]),1n);}
 vm.dispose();
}

// All exports bind statically; the generated guest invokes the ordinary service path.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false}),iat=mode===64?0x2180:0x2140;f.rtext(0x2220,'kernelbase.dll\0');f.rput(0x200c,0x2220);f.rput(0x2010,iat);let at=0x2300;
 for(const[n,name]of names.entries()){f.rput(0x2080+n*f.ptr,at,f.ptr);f.rput(iat+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 f.rput(0x2080+names.length*f.ptr,0,f.ptr);f.rput(iat+names.length*f.ptr,0,f.ptr);f.rtext(0x2500,'abc\0');
 const slot=iat+names.indexOf('lstrlen')*f.ptr;
 const code=mode===32?[0x68,...encode(f.preferred+0x2500n,4),0xff,0x15,...encode(f.preferred+BigInt(slot),4),0xc3]:[0x48,0x83,0xec,0x28,0x48,0x8d,0x0d,...encode(BigInt(0x2500-0x100b),4),0xff,0x15,...encode(BigInt(slot-0x1011),4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1001,0x1007]:[]));f.rebuildRelocations();
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[n,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+n*f.ptr),f.ptr),vm.bind('kernelbase.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),3n);vm.dispose();
}
console.log(`${checks} basic string/ABI/overlap/encoding/JIT/fault checks passed`);
