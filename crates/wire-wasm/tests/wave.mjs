import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';

const api=(t,name,args=[])=>call(t,name,args,'winmm.dll');
const operations=[
 ['waveOutClose',1],['waveInClose',1],
 ['waveOutPrepareHeader',3],['waveOutUnprepareHeader',3],['waveOutWrite',3],
 ['waveInPrepareHeader',3],['waveInUnprepareHeader',3],['waveInAddBuffer',3],
 ['waveOutPause',1],['waveOutRestart',1],['waveOutReset',1],['waveOutBreakLoop',1],
 ['waveInStart',1],['waveInStop',1],['waveInReset',1],
 ['waveOutGetPosition',3],['waveInGetPosition',3],['waveOutGetID',2],['waveInGetID',2],
 ['waveOutGetVolume',2],['waveOutSetVolume',2],['waveOutGetPitch',2],['waveOutSetPitch',2],
 ['waveOutGetPlaybackRate',2],['waveOutSetPlaybackRate',2],['waveOutMessage',4],['waveInMessage',4],
];
const caps=['waveOutGetDevCapsA','waveOutGetDevCapsW','waveInGetDevCapsA','waveInGetDevCapsW'];
const errors=['waveOutGetErrorTextA','waveOutGetErrorTextW','waveInGetErrorTextA','waveInGetErrorTextW'];
const opens=['waveOutOpen','waveInOpen'],devices=['waveOutGetNumDevs','waveInGetNumDevs'];
const names=[...devices,...opens,...caps,...errors,...operations.map(x=>x[0])];
equal(new Set(names).size,39);
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const string=(vm,p,wide)=>{let s='';for(let n=0;n<256;n++){const c=vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1);if(!c)return s;s+=String.fromCharCode(Number(c));}throw Error('unterminated wave error');};

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8,out=mode===32?0x22000n:0x123500000000n;
 if(mode===64)vm.map(out,4096n,3);
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 for(const name of names){assert.ok(vm.bind('winmm.dll',name));assert.throws(()=>vm.bind('kernel32.dll',name),/unknown/);}
 for(const name of devices)equal(api(t,name),0n);
 for(const name of opens){
  vm.write(out,new Uint8Array(16).fill(0xa5));
  equal(api(t,name,[out,0xffffffffn,0x60000n,0x60000n,0x60000n,0n]),6n);
  equal(bytes(vm,out,width),new Array(width).fill(0));equal(vm.readInt(out+BigInt(width),1),0xa5n);
  for(const output of [out,0n,0x60000n]){
   vm.write(out,new Uint8Array(16).fill(0xa5));
   equal(api(t,name,[output,0xffffffffn,0x60000n,0x60000n,0x60000n,0xffffffffn]),6n);
   equal(bytes(vm,out,16),new Array(16).fill(0xa5));
  }
  equal(api(t,name,[t.sp,0n,0x60000n,0n,0n,0n]),6n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),0n);
 }
 vm.write(out,new Uint8Array(128).fill(0xa5));
 for(const name of caps){
  for(const id of [0n,0xffffffffn,mode===64?0x100000000n:1n]){
   equal(api(t,name,[id,out,128n]),2n);equal(bytes(vm,out,128),new Array(128).fill(0xa5));
  }
  equal(api(t,name,[0n,0x60000n,0n]),name.startsWith('waveOut')?0n:2n);
 }
 for(const[name,arity]of operations){
  equal(api(t,name,Array(arity).fill(out)),5n);equal(vm.register(4),t.sp+BigInt(mode===32?(arity+1)*4:8));
  equal(bytes(vm,out,128),new Array(128).fill(0xa5));
  equal(api(t,name,Array(arity).fill(0x60000n)),5n);
 }
 for(const name of errors){
  const wide=name.endsWith('W'),unit=wide?2:1;
  for(const capacity of [1,4,256]){
   vm.write(out,new Uint8Array(256).fill(0xa5));
   equal(api(t,name,[6n,out,BigInt(capacity)]),0n);
   equal(string(vm,out,wide),'No device driver is present.'.slice(0,capacity-1));
   const end=Math.min(capacity,29)*unit;equal(vm.readInt(out+BigInt(end),1),0xa5n);
  }
  equal(api(t,name,[32n,out,256n]),0n);equal(string(vm,out,wide),'The waveform audio format is not supported.');
  vm.write(out,new Uint8Array(64).fill(0xa5));
  equal(api(t,name,[0xffffffffn,out,256n]),9n);equal(bytes(vm,out,64),new Array(64).fill(0xa5));
  equal(api(t,name,[0xffffffffn,0x60000n,0n]),0n);
  equal(api(t,name,[6n,t.sp,256n]),0n);equal(vm.ip,vm.entryPoints.returned);
  // Only the terminated result is accessed, even when the declared capacity crosses an unmapped page.
  const edge=0x50000n-BigInt(29*unit);equal(api(t,name,[6n,edge,0xffffffffn]),0n);equal(string(vm,edge,wide),'No device driver is present.');
 }
 equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Cross-page copyout and return faults preserve the original call and all output bytes.
for(const mode of [32,64])for(const name of ['waveInOpen',...errors])for(const kind of ['output','return']){
 const t=await machine(mode),{vm}=t,out=0x24ffen,width=mode/8;
 vm.write(out,new Uint8Array(128).fill(0xa5));
 frame(t,name,name==='waveInOpen'?[out,0n,0x60000n,0n,0n,0n]:[6n,out,256n],'winmm.dll');
 const before=snapshot(t),work=vm.work,page=kind==='output'?0x25000n:t.sp&~4095n;
 equal(core.wire_protect(t.handle,page,4096n,kind==='output'?2:1),0);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(vm,out,128),new Array(128).fill(0xa5));
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);
 equal(vm.register(0),name==='waveInOpen'?6n:0n);
 if(name==='waveInOpen')equal(vm.readInt(out,width),0n);else equal(string(vm,out,name.endsWith('W')),'No device driver is present.');
 vm.dispose();
}

// The complete wave group is imported in bounded batches; the new six-argument open runs as guest code.
for(const mode of [32,64])for(let offset=0;offset<names.length;offset+=16){
 const list=names.slice(offset,offset+16),f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));
 f.put(f.dirs+12,40);f.rput(0x2000,0x2040);f.rput(0x200c,0x2028);f.rput(0x2010,0x2100);f.rtext(0x2028,'winmm.dll\0');let at=0x21c0;
 for(const[i,name]of list.entries()){f.rput(0x2040+i*f.ptr,at,f.ptr);f.rput(0x2100+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 assert.ok(at<0x2600);f.relocations.length=0;
 const index=list.indexOf('waveInOpen'),b=[];
 if(index<0)b.push(0xc3);
 else if(mode===32){for(let n=0;n<5;n++)b.push(0x6a,0);b.push(0x68,...encode(f.preferred+0x3100n,4),0xff,0x15,...encode(f.preferred+BigInt(0x2100+index*f.ptr),4),0xc3);f.relocations.push(0x100b,0x1011);}
 else{b.push(0x48,0x83,0xec,0x38,0x48,0x8d,0x0d,...encode(0x3100n-0x100bn,4),0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xc7,0x44,0x24,32,0,0,0,0,0x48,0xc7,0x44,0x24,40,0,0,0,0,0xff,0x15);b.push(...encode(BigInt(0x2100+index*f.ptr-0x1000-b.length-4),4),0x48,0x83,0xc4,0x38,0xc3);}
 f.bytes.set(b,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);
 for(const[i,name]of list.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*f.ptr),f.ptr),vm.bind('winmm.dll',name));
 equal((await vm.run(100n)).reason,Exit.Returned);
 if(index>=0){equal(vm.register(0),6n);equal(vm.readInt(image.base+0x3100n,mode/8),0n);}
 vm.dispose();
}
console.log(`${checks} wave enumeration/failure/query/error-text/ABI/JIT/fault checks passed`);
