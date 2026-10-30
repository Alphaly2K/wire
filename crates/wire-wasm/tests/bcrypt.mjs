import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
const wide=s=>Array.from({length:s.length+1},(_,n)=>[...encode(BigInt(n<s.length?s.charCodeAt(n):0),2)]).flat();
const api=(t,name,args)=>call(t,name,args,'bcrypt.dll');
const slot=0x21000n,algorithm=0x22000n,provider=0x23000n,out=0x24000n;
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const pattern=n=>Uint8Array.from({length:n},(_,i)=>(i*73+19)&255);
for(const mode of [32,64]){
 let calls=0,answer='valid';
 const t=await machine(mode,{random(length){calls++;if(answer instanceof Error)throw answer;return answer==='valid'?pattern(length):answer;}}),{vm}=t;
 vm.write(algorithm,wide('RNG'));vm.write(provider,wide('Microsoft Primitive Provider'));vm.write(t.error,encode(123n,4));
 equal(api(t,'BCryptOpenAlgorithmProvider',[slot,algorithm,0n,0n]),0n);const handle=vm.readInt(slot,mode/8);assert.ok(handle>0xffffn);if(mode===64)assert.ok(handle>0xffffffffn);
 for(const length of [1,3,16,17,257,65537]){
  const target=length>4096?0x30000n:out;vm.write(target,new Uint8Array(length+1).fill(0xa5));
  const before=calls;equal(api(t,'BCryptGenRandom',[handle,target,BigInt(length),0n]),0n);equal(calls,before+1);equal(bytes(vm,target,length),Array.from(pattern(length)));equal(vm.readInt(target+BigInt(length),1),0xa5n);equal(vm.readInt(t.error),123n);
 }
 equal(api(t,'BCryptGenRandom',[0x81n,out,4n,1n]),0n);equal(api(t,'BCryptGenRandom',[0n,out,4n,2n]),0n);
 for(const [h,len,flags,status] of [[0n,1n,0n,0xc0000008n],[1n,1n,2n,0xc000000dn],[0n,1n,4n,0xc000000dn],[0n,0x1000001n,2n,0xc0000017n]]){
  const before=calls;vm.write(out,[0xa5]);equal(api(t,'BCryptGenRandom',[h,out,len,flags]),status);equal(calls,before);equal(vm.readInt(out,1),0xa5n);
 }
 let before=calls;equal(api(t,'BCryptGenRandom',[0n,0xffffffffffffffffn,0n,2n]),0n);equal(calls,before);
 if(mode===64){equal(api(t,'BCryptGenRandom',[0x100000081n,out,1n,0n]),0xc0000008n);equal(api(t,'BCryptGenRandom',[0n,out,(1n<<32n)|3n,(1n<<32n)|2n]),0n);equal(bytes(vm,out,3),Array.from(pattern(3)));}
 vm.write(out,new Uint8Array(8).fill(0xa5));answer=null;equal(api(t,'BCryptGenRandom',[handle,out,8n,0n]),0xc0000001n);equal(bytes(vm,out,8),new Array(8).fill(0xa5));
 for(const bad of [undefined,new Uint8Array(7),new Uint16Array(8),new Error('entropy unavailable')]){
  answer=bad;frame(t,'BCryptGenRandom',[handle,out,8n,0n],'bcrypt.dll');const saved=snapshot(t);
  assert.throws(()=>vm.service(),bad instanceof Error?/entropy unavailable/:/requested Uint8Array/);equal(snapshot(t),saved);equal(bytes(vm,out,8),new Array(8).fill(0xa5));
 }
 answer='valid';equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);
 equal(api(t,'BCryptCloseAlgorithmProvider',[handle,1n]),0xc000000dn);equal(api(t,'BCryptGenRandom',[handle,out,1n,0n]),0n);
 equal(api(t,'BCryptCloseAlgorithmProvider',[handle,0n]),0n);equal(api(t,'BCryptCloseAlgorithmProvider',[handle,0n]),0xc0000008n);equal(api(t,'BCryptCloseAlgorithmProvider',[0x81n,0n]),0xc0000008n);
 equal(api(t,'BCryptOpenAlgorithmProvider',[slot,algorithm,provider,0n]),0n);equal(vm.readInt(slot,mode/8),handle);
 for(const [name,implementation,flags,status] of [['SHA256','Microsoft Primitive Provider',0n,0xc00000bbn],['RNG','other',0n,0xc00000bbn],['RNG','Microsoft Primitive Provider',1n,0xc000000dn]]){
  vm.write(algorithm,wide(name));vm.write(provider,wide(implementation));vm.write(slot,encode(0xfeedn,mode/8));equal(api(t,'BCryptOpenAlgorithmProvider',[slot,algorithm,provider,flags]),status);equal(vm.readInt(slot,mode/8),0xfeedn);
 }
 for(const name of ['BCryptGenRandom','BCryptOpenAlgorithmProvider','BCryptCloseAlgorithmProvider'])for(const dll of ['kernel32.dll','kernelbase.dll','advapi32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64])for(const kind of ['output','return','arguments','guard','open-output']){
 if(mode===64&&kind==='arguments')continue;
 let calls=0;const t=await machine(mode,{random(length){calls++;return pattern(length);}}),{vm}=t;
 vm.write(algorithm,wide('RNG'));vm.write(0x24ffen,[0xa5,0xa5]);
 let page=0x25000n,protection=2;
 if(kind==='open-output')frame(t,'BCryptOpenAlgorithmProvider',[0x24ffen,algorithm,0n,0n],'bcrypt.dll');
 else frame(t,'BCryptGenRandom',[0n,0x24ffen,8n,2n],'bcrypt.dll');
 if(kind==='return'){page=t.sp&~4095n;protection=1;}
 if(kind==='arguments'){vm.register(4,0x24ff0n);vm.write(0x24ff0n,encode(vm.entryPoints.returned,4));vm.write(0x24ff4n,encode(0n,4));vm.write(0x24ff8n,encode(0x24ffen,4));vm.write(0x24ffcn,encode(8n,4));vm.write(0x25000n,encode(2n,4));protection=1;}
 if(kind==='guard'){protection=0x104;}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const saved=snapshot(t);
 equal(core.wire_service_source(t.handle),1);equal(core.wire_service_source(t.handle),1);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),saved);equal(calls,0);
 if(kind!=='guard')equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(calls,kind==='open-output'?0:1);
 if(kind!=='open-output')equal(bytes(vm,0x24ffen,8),Array.from(pattern(8)));
 equal(t.time.calls,0);vm.dispose();
}

// The descriptor must carry requests above the signed i32 boundary intact.
for(const mode of [32,64]){
 const t=await machine(mode,{random(){throw new Error('descriptor checks do not sample');}}),{vm}=t;
 vm.map(0x10000000n,0x1000000n,3);frame(t,'BCryptGenRandom',[0n,0x10000000n,0x1000000n,2n],'bcrypt.dll');
 const request=core.wire_service_source(t.handle);equal(request&127,1);equal(request&128,128);equal((request>>>8)+1,0x1000000);vm.dispose();
}

for(const mode of [32,64]){
 let t;t=await machine(mode,{random(length){const p=core.wire_input(t.handle,length);const view=new Uint8Array(core.memory.buffer,p,length);view.set(pattern(length));return view;}});
 equal(api(t,'BCryptGenRandom',[0n,out,17n,2n]),0n);equal(bytes(t.vm,out,17),Array.from(pattern(17)));t.vm.dispose();
}

const cryptoDescriptor=Object.getOwnPropertyDescriptor(globalThis,'crypto');
try{
 let sizes=[];Object.defineProperty(globalThis,'crypto',{configurable:true,value:{getRandomValues(output){sizes.push(output.length);output.fill(91);return output;}}});
 const t=await machine(64),{vm}=t;equal(api(t,'BCryptGenRandom',[0n,0x30000n,65537n,2n]),0n);equal(sizes,[65536,1]);equal(vm.readInt(0x30000n,1),91n);equal(vm.readInt(0x40000n,1),91n);vm.dispose();
}finally{if(cryptoDescriptor)Object.defineProperty(globalThis,'crypto',cryptoDescriptor);else delete globalThis.crypto;}
// RtlGenRandom uses the same entropy boundary with a two-argument BOOLEAN ABI.
for(const mode of [32,64]){
 let samples=0,available=true;const t=await machine(mode,{random(n){samples++;return available?pattern(n):null;}}),{vm}=t;
 const system=args=>call(t,'SystemFunction036',args,'advapi32.dll');vm.write(t.error,encode(123n,4));
 equal(system([out,17n]),1n);equal(bytes(vm,out,17),Array.from(pattern(17)));equal(samples,1);
 equal(system([0n,0n]),1n);equal(samples,1);equal(system([out,0x1000001n]),0n);equal(samples,1);
 available=false;vm.write(out,new Uint8Array(8).fill(0xa5));equal(system([out,8n]),0n);equal(bytes(vm,out,8),new Array(8).fill(0xa5));equal(samples,2);available=true;
 if(mode===64){equal(system([out,0x100000003n]),1n);equal(bytes(vm,out,3),Array.from(pattern(3)));}
 frame(t,'SystemFunction036',[0x24ffen,8n],'advapi32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,0x104),0);const before=snapshot(t),count=samples;
 equal(core.wire_service_source(t.handle),1);equal(core.wire_service_source(t.handle),1);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(samples,count);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),1n);equal(samples,count+1);equal(bytes(vm,0x24ffen,8),Array.from(pattern(8)));
 const code=mode===32?[0x6a,9,0x68,...encode(out,4),0xb8,...encode(vm.bind('advapi32.dll','SystemFunction036'),4),0xff,0xd0,0xc3]:[0x48,0x83,0xec,40,0x48,0xb9,...encode(out,8),0xba,9,0,0,0,0x48,0xb8,...encode(vm.bind('advapi32.dll','SystemFunction036'),8),0xff,0xd0,0x48,0x83,0xc4,40,0xc3];
 vm.write(0x30000n,code);vm.ip=0x30000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));const retired=vm.retired;equal(await finish(t),1n);assert.ok(vm.retired>retired);equal(bytes(vm,out,9),Array.from(pattern(9)));equal(vm.readInt(t.error),123n);
 for(const dll of ['kernel32.dll','kernelbase.dll','bcrypt.dll'])assert.throws(()=>vm.bind(dll,'SystemFunction036'),/unknown/);vm.dispose();
}
console.log(`${checks} secure RNG/provider/entropy/descriptor/ABI/fault checks passed`);
