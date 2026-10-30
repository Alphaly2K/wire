import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const audio=['mciSendCommandA','mciSendCommandW','mciSendStringA','mciSendStringW','waveOutGetNumDevs','waveOutOpen'];
const fonts=['AddFontMemResourceEx','RemoveFontMemResourceEx','AddFontResourceExA','AddFontResourceExW','RemoveFontResourceExA','RemoveFontResourceExW'];
const api=(t,name,args=[])=>call(t,name,args,audio.includes(name)?'winmm.dll':'gdi32.dll');
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of [...audio,...fonts]){assert.ok(vm.bind(audio.includes(name)?'winmm.dll':'gdi32.dll',name));for(const wrong of ['kernel32.dll','user32.dll',audio.includes(name)?'gdi32.dll':'winmm.dll'])assert.throws(()=>vm.bind(wrong,name),/unknown/);}
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 for(const name of audio.slice(0,4))equal(api(t,name,[0x60000n,0x60000n,0xffffffffn,0x60000n]),274n);
 equal(api(t,'waveOutGetNumDevs'),0n);equal(api(t,'waveOutOpen',[0n,0xffffffffn,0x60000n,0x60000n,0x60000n,0xffffffffn]),6n);
 const out=mode===64?0x123500000000n:0x22000n;if(mode===64)vm.map(out,4096n,3);vm.write(out,new Uint8Array(16).fill(0xa5));
 equal(api(t,'waveOutOpen',[out,0xffffffffn,0x60000n,0x60000n,0x60000n,0xfffffffen]),6n);equal(vm.readInt(out,mode/8),0n);equal(vm.readInt(out+BigInt(mode/8),1),0xa5n);equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);
 equal(api(t,'waveOutOpen',[t.sp,0n,0n,0n,0n,0n]),6n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,mode/8),0n);
 for(const name of fonts){vm.write(out,new Uint8Array(16).fill(0xa5));vm.write(t.error,encode(77n,4));equal(api(t,name,[0x60000n,0xffffffffn,0x60000n,out]),0n);equal(vm.readInt(t.error),120n);equal(vm.readInt(out,8),0xa5a5a5a5a5a5a5a5n);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Output and error writes are atomic, and a faulting return is checked first.
for(const mode of [32,64])for(const kind of ['output','error','return']){
 const t=await machine(mode),{vm}=t,out=0x24ffen;vm.write(out,new Uint8Array(16).fill(0xa5));
 frame(t,kind==='error'?'AddFontMemResourceEx':'waveOutOpen',kind==='error'?[0x60000n,0n,0x60000n,out]:[out,0n,0x60000n,0n,0n,0n],kind==='error'?'gdi32.dll':'winmm.dll');const before=snapshot(t),work=vm.work;
 const page=kind==='output'?0x25000n:kind==='error'?t.image.teb:t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,kind==='return'?1:2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(out,2),0xa5a5n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='error'?0n:6n);equal(vm.work,work+1n);vm.dispose();
}
// Static imports bind both complete groups; generated code calls waveOutOpen through the JIT.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,60);let string=0x2200;
 const groups=[['winmm.dll',audio,0x2080,0x2140],['gdi32.dll',fonts,0x20c0,0x21c0]];
 for(const[n,[provider,names,lookup,iat]]of groups.entries()){
  const descriptor=0x2000+n*20;f.rput(descriptor,lookup);f.rput(descriptor+12,string);f.rtext(string,provider+'\0');string+=provider.length+1;f.rput(descriptor+16,iat);
  for(const[i,name]of names.entries()){f.rput(lookup+i*f.ptr,string,f.ptr);f.rput(iat+i*f.ptr,string,f.ptr);f.rtext(string+2,name+'\0');string+=name.length+3;}
 }
 const iat=0x2140+5*f.ptr,b=[];
 if(mode===32){for(let n=0;n<5;n++)b.push(0x6a,0);b.push(0x68,...encode(f.preferred+0x3100n,4),0xff,0x15,...encode(f.preferred+BigInt(iat),4),0xc3);f.relocations.splice(0,f.relocations.length,0x100b,0x1011);}
 else{b.push(0x48,0x83,0xec,0x38,0x48,0x8d,0x0d,...encode(0x3100n-0x100bn,4),0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xc7,0x44,0x24,32,0,0,0,0,0x48,0xc7,0x44,0x24,40,0,0,0,0,0xff,0x15);b.push(...encode(BigInt(iat-0x1000-b.length-4),4),0x48,0x83,0xc4,0x38,0xc3);f.relocations.splice(0,f.relocations.length);}
 f.bytes.set(b,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode,clock(){throw new Error('unexpected clock');}}),image=vm.load(f.bytes);
 for(const[provider,names,,iat]of groups)for(const[i,name]of names.entries())equal(vm.readInt(image.base+BigInt(iat+i*f.ptr),f.ptr),vm.bind(provider,name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),6n);equal(vm.readInt(image.base+0x3100n,mode/8),0n);vm.dispose();
}
console.log(`${checks} unavailable audio/font/result/ABI/JIT/fault checks passed`);
