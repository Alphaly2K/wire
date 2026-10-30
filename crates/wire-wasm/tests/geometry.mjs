import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,rectangle,readRect} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

function api(t,name,args){
 if(name!=='PtInRect'||t.mode!==32)return call(t,name,args);
 frame(t,name,args);t.vm.write(t.sp+8n,encode(args[1],8));
 const work=t.vm.work,retired=t.vm.retired;
 equal(t.vm.service().reason,Exit.Dispatch);equal(t.vm.work,work+1n);equal(t.vm.retired,retired);equal(t.vm.register(4),t.sp+16n);return t.vm.register(0);
}
const point=(x,y)=>BigInt.asUintN(32,BigInt(x))|(BigInt.asUintN(32,BigInt(y))<<32n);
const names=['SetRect','SetRectEmpty','CopyRect','EqualRect','InflateRect','OffsetRect','PtInRect','IsRectEmpty','IntersectRect','UnionRect','AdjustWindowRect','AdjustWindowRectEx','AdjustWindowRectExForDpi'];

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=0x22000n,q=0x22100n,out=0x22200n;
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));vm.mxcsr=0x5f81;const fpu=fp(t);vm.write(t.error,encode(123n,4));
 equal(api(t,'SetRect',[p,0x12345678fffffffbn,0xabcdef00fffffff6n,20n,30n]),1n);equal(readRect(t,p),[-5,-10,20,30]);
 for(const [x,y,result] of [[-5,-10,1],[19,29,1],[20,10,0],[0,30,0],[-6,0,0],[0,-11,0],[2147483647,-2147483648,0]])equal(api(t,'PtInRect',[p,point(x,y)]),BigInt(result));
 equal(api(t,'CopyRect',[p+4n,p]),1n);equal(readRect(t,p+4n),[-5,-10,20,30]);
 rectangle(t,p,[10,20,110,220]);rectangle(t,q,[30,-40,150,100]);
 equal(api(t,'EqualRect',[p,q]),0n);equal(api(t,'EqualRect',[p,p]),1n);
 for(const [name,expected,result] of [['IntersectRect',[30,20,110,100],1],['UnionRect',[10,-40,150,220],1]]){
  for(const target of [out,p,q]){rectangle(t,p,[10,20,110,220]);rectangle(t,q,[30,-40,150,100]);equal(api(t,name,[target,p,q]),BigInt(result));equal(readRect(t,target),expected);}
 }
 for(const [a,b,intersection,union,unionResult] of [
  [[1,2,3,4],[4,5,6,7],[0,0,0,0],[1,2,6,7],1],
  [[3,2,1,4],[4,5,6,7],[0,0,0,0],[4,5,6,7],1],
  [[1,2,1,4],[6,5,4,7],[0,0,0,0],[0,0,0,0],0],
 ]){
  rectangle(t,p,a);rectangle(t,q,b);equal(api(t,'IntersectRect',[out,p,q]),0n);equal(readRect(t,out),intersection);equal(api(t,'UnionRect',[out,p,q]),BigInt(unionResult));equal(readRect(t,out),union);
 }
 rectangle(t,p,[2147483647,-2147483648,-1,0]);equal(api(t,'OffsetRect',[p,1n,0xffffffffn]),1n);equal(readRect(t,p),[-2147483648,2147483647,0,-1]);
 rectangle(t,p,[0,0,100,200]);equal(api(t,'InflateRect',[p,0xfffffffen,3n]),1n);equal(readRect(t,p),[2,-3,98,203]);
 equal(api(t,'IsRectEmpty',[p]),0n);equal(api(t,'SetRectEmpty',[p]),1n);equal(readRect(t,p),[0,0,0,0]);equal(api(t,'IsRectEmpty',[p]),1n);
 for(const [name,args,expected] of [
  ['AdjustWindowRect',[p,0xcf0000n,0n],[6,-7,114,224]],
  ['AdjustWindowRectEx',[p,0xcf0000n,1n,0x200n],[4,-28,116,226]],
  ['AdjustWindowRectExForDpi',[p,0xcf0000n,0n,0x80n,96n],[6,-2,114,224]],
 ]){rectangle(t,p,[10,20,110,220]);equal(api(t,name,args),1n);equal(readRect(t,p),expected);}
 for(const name of names){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','gdi32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(vm.mxcsr,0x5f81);equal(fp(t),fpu);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Null pointers follow rectangle Boolean semantics without changing LastError.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(123n,4));
 for(const [name,args,result] of [
  ['SetRect',[0n,1n,2n,3n,4n],0n],['SetRectEmpty',[0n],0n],['CopyRect',[0n,0xdeadn],0n],['CopyRect',[0xdeadn,0n],0n],
  ['EqualRect',[0n,0n],0n],['IsRectEmpty',[0n],1n],['PtInRect',[0n,point(-1,2)],0n],['InflateRect',[0n,1n,2n],0n],['OffsetRect',[0n,1n,2n],0n],
  ['IntersectRect',[0xdeadn,0n,0xdeadn],0n],['UnionRect',[0xdeadn,0xdeadn,0n],0n],
 ]){equal(api(t,name,args),result);equal(vm.readInt(t.error),123n);}vm.dispose();
}

// Copy-in and saved return frames make overlapping outputs safe and faults atomic.
for(const mode of [32,64])for(const name of ['SetRect','CopyRect','InflateRect','OffsetRect','IntersectRect','UnionRect','AdjustWindowRectEx']){
 const t=await machine(mode),{vm}=t,out=0x24ff8n,p=0x22000n,q=0x22100n;
 rectangle(t,p,[0,0,10,20]);rectangle(t,q,[5,5,15,30]);rectangle(t,out,[0,0,1,1]);
 const args=name==='SetRect'?[out,1n,2n,3n,4n]:name==='CopyRect'?[out,p]:['IntersectRect','UnionRect'].includes(name)?[out,p,q]:name==='AdjustWindowRectEx'?[out,0xcf0000n,0n,0n]:[out,1n,2n];
 frame(t,name,args);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t),prefix=vm.readInt(out,8);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,8),prefix);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 rectangle(t,p,[10,20,30,40]);equal(api(t,'CopyRect',[t.sp,p]),1n);equal(vm.ip,vm.entryPoints.returned);equal(readRect(t,t.sp),[10,20,30,40]);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;rectangle(t,0x22000n,[1,2,3,4]);
 equal(api(t,'AdjustWindowRectExForDpi',[0x22000n,0n,0n,0n,192n]),0n);equal(vm.readInt(t.error),50n);equal(readRect(t,0x22000n),[1,2,3,4]);
 frame(t,'CopyRect',[0x22000n,0xdead0000n]);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(readRect(t,0x22000n),[1,2,3,4]);
 if(mode===64){vm.map(0x123400000000n,4096n,3);equal(api(t,'SetRect',[0x123400000000n,1n,2n,3n,4n]),1n);equal(readRect(t,0x123400000000n),[1,2,3,4]);}
 vm.dispose();
}
console.log(`${checks} rectangle/frame/packed-POINT/ABI/alias/null/fault checks passed`);
