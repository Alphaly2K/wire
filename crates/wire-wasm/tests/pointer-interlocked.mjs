import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const i32=n=>BigInt.asIntN(32,n);
function reference(a,b,d){a=i32(a);b=i32(b);d=i32(d);if(!d)return 0xffffffffn;const product=a*b,negative=(product<0n)!==(d<0n),abs=n=>n<0n?-n:n,p=abs(product),q=abs(d);let n=p/q;if((p%q)*2n>=q)n++;if(negative)n=-n;return n<-(1n<<31n)||n>=(1n<<31n)?0xffffffffn:BigInt.asUintN(32,n);}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=0x22000n;
 vm.write(t.error,encode(123n,4));vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x72));
 for(const value of [0n,1n,0xffffffffn,1n<<31n,0xfedcba9876543210n,(1n<<64n)-1n]){const result=BigInt.asUintN(mode,value);equal(api(t,'EncodePointer',[value]),result);equal(api(t,'DecodePointer',[result]),result);}
 const values=[0n,1n,-1n,3n,-3n,0x7fffffffn,-0x80000000n];
 for(const a of values)for(const b of values)for(const d of [0n,2n,-2n,3n,0x7fffffffn,-0x80000000n])equal(api(t,'MulDiv',[a,b,d]),reference(a,b,d));
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x72));equal(vm.readInt(t.error),123n);
 for(const name of ['IsBadReadPtr','IsBadWritePtr']){
  equal(api(t,name,[0xdead0000n,0n]),0n);equal(api(t,name,[0xdead0000n,1n]),1n);equal(api(t,name,[p,4096n]),0n);equal(api(t,name,[0x4ffffn,2n]),1n);
  if(mode===64){equal(api(t,name,[p,1n<<32n]),1n);equal(api(t,name,[p,(1n<<64n)-1n]),1n);}
 }
 equal(core.wire_protect(t.handle,p,4096n,2),0);equal(api(t,'IsBadReadPtr',[p,1n]),0n);equal(api(t,'IsBadWritePtr',[p,1n]),1n);
 equal(api(t,'IsBadCodePtr',[p]),1n);equal(core.wire_protect(t.handle,p,4096n,0x20),0);equal(api(t,'IsBadCodePtr',[p]),0n);
 equal(core.wire_protect(t.handle,p,4096n,0x140),0);equal(api(t,'IsBadCodePtr',[p]),1n);equal(api(t,'IsBadCodePtr',[p]),1n);equal(api(t,'IsBadReadPtr',[p,1n]),1n);equal(api(t,'IsBadCodePtr',[p]),0n);
 equal(core.wire_protect(t.handle,p,4096n,4),0);equal(vm.readInt(t.error),123n);
 for(const [name,args]of [['InterlockedIncrement',[p]],['InterlockedDecrement',[p]],['InterlockedExchange',[p,7n]],['InterlockedCompareExchange',[p,9n,7n]]]){vm.write(p,encode(name==='InterlockedIncrement'?0xffffffffn:name==='InterlockedDecrement'?0n:7n,4));const old=vm.readInt(p);const expected=name==='InterlockedIncrement'?0n:name==='InterlockedDecrement'?0xffffffffn:7n;equal(api(t,name,args),expected);equal(vm.readInt(p),name==='InterlockedCompareExchange'?9n:name==='InterlockedExchange'?7n:expected);if(name==='InterlockedExchange')equal(old,7n);}
 vm.write(p,encode(7n,4));equal(core.wire_protect(t.handle,p,4096n,2),0);equal(api(t,'InterlockedCompareExchange',[p,9n,8n]),7n);frame(t,'InterlockedCompareExchange',[p,9n,7n],'kernel32.dll');const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(p),7n);equal(core.wire_protect(t.handle,p,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),7n);equal(vm.readInt(p),9n);
 vm.write(0x24ffen,encode(0x7fffffffn,4));frame(t,'InterlockedIncrement',[0x24ffen],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const beforeCross=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),beforeCross);equal(vm.readInt(0x24ffen),0x7fffffffn);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0x80000000n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

function header(t,p,{next=0n,depth=0n,sequence=0n,reserved=0n}={}){const wide=t.mode===64;t.vm.write(p,wide?[...encode(depth|(sequence<<16n),8),...encode(next|reserved,8)]:encode(next|(depth<<32n)|(sequence<<48n),8));}
function inspect(t,p){const wide=t.mode===64,a=t.vm.readInt(p,8),b=wide?t.vm.readInt(p+8n,8):0n;return {next:wide?b&~15n:a&0xffffffffn,depth:wide?a&65535n:(a>>32n)&65535n,sequence:wide?a>>16n:a>>48n,reserved:wide?b&15n:0n};}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=0x22000n,e1=0x23000n,e2=0x24000n,width=mode/8,size=mode===64?16:8;
 vm.write(t.error,encode(123n,4));vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));
 vm.write(h,new Uint8Array(size+1).fill(0xa5));equal(api(t,'InitializeSListHead',[h]),0n);equal(inspect(t,h),{next:0n,depth:0n,sequence:0n,reserved:0n});equal(vm.readInt(h+BigInt(size),1),0xa5n);
 vm.write(e1,new Uint8Array(16).fill(0xa5));vm.write(e2,new Uint8Array(16).fill(0xa5));equal(api(t,'InterlockedPushEntrySList',[h,e1]),0n);equal(api(t,'InterlockedPushEntrySList',[h,e2]),e1);equal(vm.readInt(e1,width),0n);equal(vm.readInt(e2,width),e1);equal(vm.readInt(e1+BigInt(width),1),0xa5n);equal(inspect(t,h),{next:e2,depth:2n,sequence:2n,reserved:0n});
 equal(api(t,'InterlockedPopEntrySList',[h]),e2);equal(inspect(t,h),{next:e1,depth:1n,sequence:3n,reserved:0n});equal(api(t,'InterlockedFlushSList',[h]),e1);equal(inspect(t,h),{next:0n,depth:0n,sequence:4n,reserved:0n});equal(vm.readInt(e2,width),e1);
 equal(core.wire_protect(t.handle,h,4096n,2),0);equal(api(t,'InterlockedPopEntrySList',[h]),0n);equal(core.wire_protect(t.handle,h,4096n,4),0);
 const maxSequence=mode===64?(1n<<48n)-1n:65535n,reserved=mode===64?13n:0n;
 header(t,h,{depth:65535n,sequence:maxSequence,reserved});equal(api(t,'InterlockedPushEntrySList',[h,e1]),0n);equal(inspect(t,h),{next:e1,depth:0n,sequence:0n,reserved});equal(api(t,'InterlockedPopEntrySList',[h]),e1);equal(inspect(t,h),{next:0n,depth:65535n,sequence:1n,reserved});
 header(t,h,{next:e1,depth:1n,sequence:maxSequence,reserved});equal(api(t,'InterlockedFlushSList',[h]),e1);equal(inspect(t,h),{next:0n,depth:0n,sequence:0n,reserved});
 for(const [name,args]of [['InitializeSListHead',[h+1n]],['InterlockedPushEntrySList',[h,0n]],['InterlockedPushEntrySList',[h,e1+1n]]]){frame(t,name,args,'kernel32.dll');const before=snapshot(t),old=inspect(t,h);assert.throws(()=>vm.service(),/unaligned|NULL/);equal(snapshot(t),before);equal(inspect(t,h),old);}
 header(t,h,{next:e1,depth:1n});vm.write(e1,encode(e2+1n,width));frame(t,'InterlockedPopEntrySList',[h],'kernel32.dll');const malformed=snapshot(t);assert.throws(()=>vm.service(),/unaligned/);equal(snapshot(t),malformed);equal(inspect(t,h).next,e1);
 // Failed publication cannot change entry.Next, head, or the captured call.
 header(t,h);vm.write(e1,encode(0x12345678n,width));frame(t,'InterlockedPushEntrySList',[h,e1],'kernel32.dll');equal(core.wire_protect(t.handle,h,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(e1,width),0x12345678n);equal(inspect(t,h).next,0n);equal(core.wire_protect(t.handle,h,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(inspect(t,h).next,e1);
 header(t,h);vm.write(e1,encode(0x12345678n,width));frame(t,'InterlockedPushEntrySList',[h,e1],'kernel32.dll');equal(core.wire_protect(t.handle,e1,4096n,0x104),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(inspect(t,h).next,0n);equal(vm.readInt(e1,width),0x12345678n);equal(vm.service().reason,Exit.Dispatch);equal(inspect(t,h).next,e1);
 header(t,h,{next:e1,depth:1n});vm.write(e1,encode(0n,width));frame(t,'InterlockedPopEntrySList',[h],'kernel32.dll');equal(core.wire_protect(t.handle,e1,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(inspect(t,h).next,e1);equal(core.wire_protect(t.handle,e1,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),e1);
 // The return frame is read before any header store, on both ABIs.
 const aliased=t.sp&~15n;frame(t,'InitializeSListHead',[aliased],'kernel32.dll');vm.register(4,aliased);vm.write(aliased,encode(vm.entryPoints.returned,width));if(mode===32)vm.write(aliased+4n,encode(aliased,4));equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(aliased,8),0n);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(api(t,'InitializeSListHead',[high]),0n);equal(api(t,'InterlockedPushEntrySList',[high,high+32n]),0n);equal(api(t,'InterlockedPopEntrySList',[high]),high+32n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Source writes through the API revoke code under the same VMM authority.
for(const mode of [32,64]){
 const t=await machine(mode,{dep:false}),{vm}=t,h=0x30000n;vm.write(h,[0xb8,83,0,0,0,0xc3]);vm.ip=h;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,mode/8));equal((await vm.run(100n)).reason,Exit.Returned);const ticket=core.wire_ticket(t.handle);equal(core.wire_live(t.handle,ticket),1);equal(api(t,'InitializeSListHead',[h]),0n);equal(core.wire_live(t.handle,ticket),0);
 for(const name of ['IsBadReadPtr','IsBadWritePtr','IsBadCodePtr','MulDiv','EncodePointer','DecodePointer','InitializeSListHead','InterlockedIncrement','InterlockedDecrement','InterlockedExchange','InterlockedCompareExchange','InterlockedPushEntrySList','InterlockedPopEntrySList','InterlockedFlushSList']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 vm.dispose();
}
console.log(`${checks} pointer/MulDiv/LONG/SLIST ABI/layout/permission/transaction/source checks passed`);
