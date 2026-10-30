import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const counter=0x23000n,context=0x23010n;
function handler(t,{skip=true,redirect=0n,stack=0n,retry=false,sleep=false}={}){
 const {vm,mode}=t,bytes=[],emit=(...v)=>bytes.push(...v),value=(v,n)=>emit(...encode(v,n)),ip=mode===32?184n:248n,sp=mode===32?196n:152n;
 if(mode===32){emit(0x8b,0x44,0x24,4,0x8b,0x40,4,0xa3);value(context,4);}else{emit(0x48,0x8b,0x41,8,0x49,0xbb);value(context,8);emit(0x49,0x89,0x03);}
 if(mode===32){emit(0xff,0x05);value(counter,4);}else{emit(0x49,0xbb);value(counter,8);emit(0x41,0xff,0x03);}
 let jump;
 if(retry){if(mode===32){emit(0x83,0x3d);value(counter,4);}else emit(0x41,0x83,0x3b);emit(1,0x74,0);jump=bytes.length-1;}
 if(redirect){if(mode===32){emit(0xc7,0x80);value(ip,4);value(redirect,4);}else{emit(0x48,0xc7,0x80);value(ip,4);value(redirect,4);}}
 else if(skip){emit(...(mode===64?[0x48]:[]),0xff,0x80);value(ip,4);}
 if(jump!==undefined)bytes[jump]=bytes.length-jump-1;
 if(stack){emit(...(mode===64?[0x48]:[]),0xc7,0x80);value(sp,4);value(stack,4);}
 if(sleep){if(mode===32){emit(0x6a,0,0xb8);value(vm.bind('kernel32.dll','Sleep'),4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,40,0x31,0xc9,0x48,0xb8);value(vm.bind('kernel32.dll','Sleep'),8);emit(0xff,0xd0,0x48,0x83,0xc4,40);}}
 emit(0xb8,255,255,255,255,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x30000n,bytes);return api(t,'AddVectoredExceptionHandler',[1n,0x30000n]);
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;handler(t,{sleep:true});vm.write(t.error,encode(123n,4));vm.flags=0x247n;vm.xmm(6,new Uint8Array(16).fill(0x6a));
 frame(t,'DebugBreak',[],'kernel32.dll');const gateway=vm.ip,w=vm.work,r=vm.retired;equal(vm.service().reason,Exit.Dispatch);
 const pointers=mode===32?vm.readInt(vm.register(4)+4n,4):vm.register(1),record=vm.readInt(pointers,mode/8),ctx=vm.readInt(pointers+BigInt(mode/8),mode/8);
 equal(vm.readInt(record),0x80000003n);equal(vm.readInt(record+BigInt(mode===32?12:16),mode/8),gateway);equal(vm.readInt(record+BigInt(mode===32?16:24)),mode===32?3n:1n);
 equal(vm.readInt(ctx+BigInt(mode===32?184:248),mode/8),gateway);equal(vm.readInt(ctx+BigInt(mode===32?196:152),mode/8),t.sp);
 await finish(t,1n);equal(vm.readInt(counter),1n);equal(vm.flags,0x247n);equal([...vm.xmm(6)],new Array(16).fill(0x6a));equal(vm.readInt(t.error),123n);equal(vm.register(4),t.sp+BigInt(mode/8));equal(vm.work-w,vm.retired-r+2n);
 equal(vm.bind('kernelbase.dll','DebugBreak'),gateway);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
for(const mode of [32,64])for(const kind of ['retry','redirect','return-fault','preflight']){
 const t=await machine(mode),{vm}=t;handler(t,{retry:kind==='retry',redirect:kind==='redirect'?0x32000n:0n,stack:kind==='return-fault'?0x24000n:0n});
 vm.write(0x32000n,[0xb8,73,0,0,0,0xc3]);vm.write(0x24000n,encode(vm.entryPoints.returned,mode/8));
 if(kind==='preflight')t.sp=(t.sp&~4095n)+256n;
 frame(t,'DebugBreak',[],'kernel32.dll');const w=vm.work,r=vm.retired;
 if(kind==='preflight'){const page=(t.sp&~4095n)-4096n;equal(core.wire_protect(t.handle,page,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(counter),0n);}
 equal(vm.service().reason,Exit.Dispatch);
 if(kind==='return-fault'){
  for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<40);equal((await vm.run(1n)).reason,Exit.Budget);}
  equal(vm.readInt(counter),1n);equal(core.wire_protect(t.handle,0x24000n,4096n,0x104),0);const before=snapshot(t);equal((await vm.run(100n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(counter),1n);
 }
 await finish(t,1n);equal(vm.readInt(counter),kind==='retry'?2n:1n);equal(vm.work-w,vm.retired-r+(kind==='retry'?2n:1n));if(kind==='redirect')equal(vm.register(0),73n);if(kind==='return-fault')equal(vm.register(4),0x24000n+BigInt(mode/8));vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;frame(t,'DebugBreak',[],'kernel32.dll');const w=vm.work;equal(vm.service().reason,Exit.Dispatch);equal((await vm.run(100n)).exception.code,0x80000003);equal(vm.work,w+1n);equal((await vm.run(100n)).exception.code,0x80000003);equal(vm.work,w+1n);vm.dispose();
}
// The logical breakpoint stub unwinds as a leaf into its guest caller's handler.
{
 const t=await machine(64),{vm}=t,entry=0x28000n,body=0x32000n;
 vm.write(0x30000n,[0x49,0xff,0x80,...encode(248n,4),0x49,0xbb,...encode(counter,8),0x41,0xff,0x03,0x31,0xc0,0xc3]);
 vm.write(entry,[...encode(body,4),...encode(body+0x100n,4),...encode(0x28100n,4)]);vm.write(0x28100n,[9,4,1,0,4,0x42,0,0,...encode(0x30000n,4)]);
 equal(api(t,'RtlAddFunctionTable',[entry,1n,0n]),1n);
 vm.write(body,[0x48,0x83,0xec,40,0x48,0xb8,...encode(vm.bind('kernel32.dll','DebugBreak'),8),0xff,0xd0,0x48,0x83,0xc4,40,0xb8,73,0,0,0,0xc3]);
 vm.ip=body;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await finish(t,1n);equal(vm.register(0),73n);equal(vm.readInt(counter),1n);vm.dispose();
}
{
 const t=await machine(64),{vm}=t,body=0x32000n,target=body+0x80n;
 const b=[0x48,0x83,0xec,56,0x49,0xbb,...encode(counter,8),0x41,0xff,0x03];
 const arguments_=[t.sp-40n,target,0n,73n,0x26000n,0n];
 for(let n=0;n<arguments_.length;n++)if(n<4)b.push(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n],...encode(arguments_[n],8));else b.push(0x48,0xb8,...encode(arguments_[n],8),0x48,0x89,0x44,0x24,32+(n-4)*8);
 b.push(0x48,0xb8,...encode(vm.bind('kernel32.dll','RtlUnwindEx'),8),0xff,0xd0,0xcc);vm.write(0x30000n,b);
 vm.write(0x28000n,[...encode(0x30000n,4),...encode(0x30100n,4),...encode(0x28200n,4),...encode(body,4),...encode(body+0x100n,4),...encode(0x28100n,4)]);
 vm.write(0x28100n,[1,4,1,0,4,0x42,0,0]);vm.write(0x28200n,[1,4,1,0,4,0x62,0,0]);equal(api(t,'RtlAddFunctionTable',[0x28000n,2n,0n]),1n);api(t,'AddVectoredExceptionHandler',[1n,0x30000n]);
 vm.write(body,[0x48,0x83,0xec,40,0x48,0xb8,...encode(vm.bind('kernel32.dll','DebugBreak'),8),0xff,0xd0,0xcc]);vm.write(target,[0x48,0x83,0xc4,40,0xc3]);
 vm.ip=body;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));await finish(t,1n);equal(vm.register(0),73n);equal(vm.readInt(counter),1n);vm.dispose();
}
console.log(`${checks} DebugBreak record/CONTEXT/skip/retry/redirect/yield/guard/return-fault/unhandled checks passed`);
