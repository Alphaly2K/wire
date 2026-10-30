import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const utf16=s=>Array.from({length:s.length},(_,i)=>Array.from(encode(BigInt(s.charCodeAt(i)),2))).flat();
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const input=0x22000n,out=0x23000n,locale=0x26000n;
const ansiTypes=['GetStringTypeA','GetStringTypeExA'];
const names=[...ansiTypes,'GetStringTypeW','GetStringTypeExW','LCMapStringA','LCMapStringW','LCMapStringEx'];

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.flags=0x8d5n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));const saved=fp(t);vm.write(t.error,encode(123n,4));
 for(const name of ansiTypes){
  vm.write(input,[65,0x82,0xa0,0]);vm.write(out,new Uint8Array(12).fill(0xa5));equal(api(t,name,[0x411n,1n,input,3n,out]),1n);equal(vm.readInt(out,2)&1n,1n);equal(vm.readInt(out+2n,2)&0x100n,0x100n);equal(bytes(vm,out+4n,8),new Array(8).fill(0xa5));
  for(const length of [0xffffffffn,0xfffffffen,0x80000000n]){vm.write(out,new Uint8Array(12).fill(0xa5));equal(api(t,name,[0x411n,1n,input,length,out]),1n);equal(vm.readInt(out+4n,2)&0x20n,0x20n);equal(vm.readInt(out+6n,2),0xa5a5n);}
  vm.write(input,[0xe9,0x80,0]);equal(api(t,name,[0x409n,1n,input,0xffffffffn,out]),1n);equal(vm.readInt(out,2)&2n,2n);equal(vm.readInt(out+2n,2)&0x100n,0n);
  vm.write(input,[65,0,97]);equal(api(t,name,[0x409n,1n,input,3n,out]),1n);equal(vm.readInt(out+2n,2)&0x20n,0x20n);equal(vm.readInt(out+4n,2)&2n,2n);
 }
 vm.write(input,utf16('Aa\0'));for(const id of [0n,0x411n,0xdeadbeefn])for(const count of [3n,0xffffffffn,0xfffffffen]){equal(api(t,'GetStringTypeExW',[id,1n,input,count,out]),1n);equal(vm.readInt(out,2)&1n,1n);equal(vm.readInt(out+2n,2)&2n,2n);equal(vm.readInt(out+4n,2)&0x20n,0x20n);}
 vm.write(input,[...encode(0xd800n,2),...encode(0xdc00n,2)]);equal(api(t,'GetStringTypeExW',[0n,1n,input,2n,out]),1n);equal(vm.readInt(out),0n);
 for(const[id,data,flags,wanted]of [
  [0x409,[0xe9,0xff,0x80,0],0x200,[0xc9,0x9f,0x80,0]],
  [0x409,[0xc9,0x9f,0],0x100,[0xe9,0xff,0]],
  [0x411,[0x82,0x81,0],0x200,[0x82,0x60,0]],
  [0x409,[0x82,0x81,0],0x40000200,[0x82,0x60,0]],
  [0x7f,[0x82,0x81,0],0x200,[0x82,0x60,0]],
  [0x400,[97,0,98],0x200,[65,0,66]],
 ]){
  vm.write(input,data);const count=id===0x400?3n:0xffffffffn;equal(api(t,'LCMapStringA',[BigInt(id),BigInt(flags),input,count,0xdead0000n,0n]),BigInt(wanted.length));equal(api(t,'LCMapStringA',[BigInt(id),BigInt(flags),input,count,out,32n]),BigInt(wanted.length));equal(bytes(vm,out,wanted.length),wanted);
 }
 for(const name of ['LCMapStringW','LCMapStringEx']){
  vm.write(locale,utf16('en-US\0'));const id=name.endsWith('Ex')?locale:0x409n;
  for(const[source,flags,wanted]of [['Ab\0',0x200,'AB\0'],['\ud800a\udfff\0',0x200,'\ud800A\udfff\0'],['\ud801\udc28',0x200,'\ud801\udc00'],['ΟΣ',0x100,'ος']]){
   vm.write(input,utf16(source));equal(api(t,name,[id,BigInt(flags),input,BigInt(source.length),0xdead0000n,0n,0n,0n,0n]),BigInt(wanted.length));equal(api(t,name,[id,BigInt(flags),input,BigInt(source.length),out,32n,0n,0n,0n]),BigInt(wanted.length));equal(bytes(vm,out,wanted.length*2),utf16(wanted));
  }
  vm.write(input,utf16('Ab\0'));equal(api(t,name,[id,0x200n,input,0xfffffffen,input,32n,0n,0n,0n]),3n);equal(bytes(vm,input,6),utf16('AB\0'));
 }
 vm.write(input,[97,98,0]);equal(api(t,'LCMapStringA',[0x409n,0x200n,input,0xfffffffen,input,32n]),3n);equal(bytes(vm,input,3),[65,66,0]);
 vm.write(0x27000n,[2024,2,4,29,13,5,7,0].flatMap(n=>Array.from(encode(BigInt(n),2))));vm.write(input,[0x82,0x81,0]);for(const name of ['GetDateFormatA','GetTimeFormatA']){equal(api(t,name,[0x7fn,0n,0x27000n,input,out,32n]),3n);equal(bytes(vm,out,3),[0x82,0x81,0]);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(fp(t),saved);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(input,[65,0]);
 for(const name of [...ansiTypes,'GetStringTypeExW'])for(const[kind,error]of [[0n,1004n],[2n,50n],[3n,1004n],[4n,50n]]){vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,name,[0x409n,kind,0xdead0000n,1n,out]),0n);equal(vm.readInt(t.error),error);equal(bytes(vm,out,8),new Array(8).fill(0xa5));}
 for(const[name,args,error]of [
  ['GetStringTypeA',[0xdeadn,1n,input,1n,out],87n],
  ['GetStringTypeA',[0x409n,1n,input,0n,out],87n],
  ['GetStringTypeExW',[0xdeadn,1n,input,1n,0n],87n],
  ['GetStringTypeExA',[0x409n,1n,0n,1n,out],87n],
  ['LCMapStringA',[0x409n,0x200n,input,0n,out,32n],87n],
  ['LCMapStringA',[0x409n,0x200n,input,1n,out,0xffffffffn],87n],
  ['LCMapStringA',[0x409n,0x200n,input,1n,0n,32n],87n],
  ['LCMapStringA',[0xdeadn,0x200n,input,1n,out,32n],87n],
  ['LCMapStringA',[0x409n,0n,input,1n,out,32n],1004n],
  ['LCMapStringA',[0x409n,0x80000000n,input,1n,out,32n],1004n],
  ['LCMapStringA',[0x409n,0x400n,input,1n,out,32n],50n],
  ['LCMapStringA',[0x409n,0x300n,input,1n,out,32n],50n],
  ['LCMapStringA',[0x409n,0x01000200n,input,1n,out,32n],50n],
 ]){vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,name,args),0n);equal(vm.readInt(t.error),error);equal(bytes(vm,out,8),new Array(8).fill(0xa5));}
 for(const name of ['LCMapStringA','LCMapStringW','LCMapStringEx']){const wide=!name.endsWith('A');vm.write(input,wide?utf16('Ab\0'):[65,98,0]);vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,name,[name.endsWith('Ex')?0n:0x409n,0x200n,input,0xffffffffn,out,2n,0n,0n,0n]),0n);equal(vm.readInt(t.error),122n);equal(bytes(vm,out,8),new Array(8).fill(0xa5));}
 for(const name of names){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,cross=0x24ffcn;
 // Two DBCS-decoded characters fit exactly; the third WORD crosses the page.
 for(const name of ansiTypes){vm.write(input,[65,0x82,0xa0,0]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(api(t,name,[0x411n,1n,input,3n,cross]),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);vm.write(cross,new Uint8Array(6).fill(0xa5));frame(t,name,[0x411n,1n,input,4n,cross],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,cross,4),new Array(4).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);}
 for(const name of ['LCMapStringA','LCMapStringW','LCMapStringEx']){const wide=!name.endsWith('A');vm.write(input,wide?utf16('AbCd\0'):[65,98,67,100,0]);vm.write(cross,new Uint8Array(12).fill(0xa5));frame(t,name,[name.endsWith('Ex')?0n:0x409n,0x200n,input,0xffffffffn,cross,32n,0n,0n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,cross,4),new Array(4).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);}
 vm.write(0x24fffn,[0x82]);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(api(t,'LCMapStringA',[0x411n,0x200n,0x24fffn,1n,out,32n]),1n);equal(vm.readInt(out,1),63n);frame(t,'LCMapStringA',[0x411n,0x200n,0x24fffn,2n,out,32n],'kernel32.dll');let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);vm.write(0x25000n,[0xa0]);equal(vm.service().reason,Exit.Dispatch);equal(bytes(vm,out,2),[0x82,0xa0]);
 vm.write(input,[97,98,0]);equal(api(t,'LCMapStringA',[0x409n,0x200n,input,3n,t.sp,32n]),3n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,3),[65,66,0]);equal(api(t,'GetStringTypeA',[0x409n,1n,input,3n,input]),1n);equal(vm.readInt(input,2)&2n,2n);equal(vm.readInt(input+2n,2)&2n,2n);
 frame(t,'LCMapStringA',[0xdeadn,0x200n,input,1n,out,32n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),87n);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);vm.write(high,[97,98,0]);equal(api(t,'LCMapStringA',[0x100000409n,0x100000200n,high,0x100000003n,high+64n,0x100000010n]),3n);equal(bytes(vm,high+64n,3),[65,66,0]);equal(api(t,'GetStringTypeExA',[0x100000409n,0x100000001n,high,0x100000003n,high+128n]),1n);equal(vm.readInt(high+128n,2)&2n,2n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} ANSI/Ex character classification/case/DBCS/UTF-16/count/ABI/transaction checks passed`);
