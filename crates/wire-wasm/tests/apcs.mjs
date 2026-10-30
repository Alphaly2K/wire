import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,finish} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const data=0x23000n,trace=0x24000n;
function asm(mode){
 const bytes=[],emit=(...v)=>bytes.push(...v),value=(v,n)=>emit(...encode(BigInt(v),n));
 const store=(address,reg=0,width=4)=>{if(mode===32){emit(0x89,0x05|(reg<<3));value(address,4);}else{emit(0x49,0xbb);value(address,8);emit(width===8?0x49:0x41,0x89,0x03|(reg<<3));}};
 const load=address=>{if(mode===32){emit(0xa1);value(address,4);}else{emit(0x49,0xbb);value(address,8);emit(0x41,0x8b,0x03);}};
 const service=(pc,args=[])=>{if(mode===32){for(const a of args.toReversed()){emit(0x68);value(a,4);}emit(0xb8);value(pc,4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,0x38);for(let n=0;n<args.length;n++){if(n<4){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}else{emit(0x48,0xb8);value(args[n],8);emit(0x48,0x89,0x44,0x24,32+(n-4)*8);}}emit(0x48,0xb8);value(pc,8);emit(0xff,0xd0,0x48,0x83,0xc4,0x38);}};
 const ret=(v=99)=>emit(0xb8,v&255,(v>>8)&255,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));
 const mark=digit=>{load(trace);emit(0xc1,0xe0,4,0x83,0xc8,digit);store(trace);};
 return {bytes,emit,value,store,load,service,ret,mark};
}
const k=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const w=(t,name,args=[])=>call(t,name,args,'ws2_32.dll');
const queue=(t,proc,parameter=0n,thread=(1n<<BigInt(t.mode))-2n)=>k(t,'QueueUserAPC',[proc,thread,parameter]);
function counter(t,base=0x30000n){const {vm,mode}=t,a=asm(mode);if(mode===32)a.emit(0x8b,0x44,0x24,4);a.store(data,mode===32?0:1,mode/8);a.load(data+8n);a.emit(0x83,0xc0,1);a.store(data+8n);a.ret();vm.write(base,a.bytes);return base;}
for(const mode of [32,64])for(const wait of ['SleepEx','WaitForSingleObjectEx','WaitForMultipleObjectsEx','WSAWaitForMultipleEvents','MsgWaitForMultipleObjectsEx']){
 const t=await machine(mode),{vm}=t,proc=counter(t),parameter=0x8877665544332211n,event=k(t,'CreateEventW',[0n,0n,1n,0n]);vm.write(data+32n,encode(event,mode/8));if(wait.startsWith('WSA'))equal(w(t,'WSAStartup',[0x202n,0x22000n]),0n);equal(queue(t,proc,parameter),1n);
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));vm.mxcsr=0x5f80;const fpu=fp(t);
 const args=wait==='SleepEx'?[1000n,1n]:wait==='WaitForSingleObjectEx'?[event,1000n,1n]:wait==='MsgWaitForMultipleObjectsEx'?[1n,data+32n,1000n,1n,2n]:[1n,data+32n,0n,1000n,1n];frame(t,wait,args,wait.startsWith('WSA')?'ws2_32.dll':wait.startsWith('Msg')?'user32.dll':'kernel32.dll');const registers=Array.from({length:mode===32?8:16},(_,n)=>vm.register(n)),work=vm.work,retired=vm.retired;await finish(t,1n);equal(vm.register(0),0xc0n);equal(vm.register(4),t.sp+BigInt(mode===64?8:4+args.length*4));equal(vm.readInt(data,mode/8),BigInt.asUintN(mode,parameter));equal(vm.readInt(data+8n),1n);equal(vm.work-work,vm.retired-retired+1n);for(let n=1;n<registers.length;n++)if(n!==4)equal(vm.register(n),registers[n]);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(vm.mxcsr,0x5f80);equal(fp(t),fpu);equal(k(t,'WaitForSingleObject',[event,0n]),0n);equal(k(t,'WaitForSingleObject',[event,0n]),258n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Every preflight failure keeps all callbacks queued and leaves the caller intact.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,proc=counter(t),self=(1n<<BigInt(mode))-2n;
 equal(queue(t,1n,0n,0n),0n);equal(vm.readInt(t.error),6n);frame(t,'QueueUserAPC',[1n,self,0n],'kernel32.dll');const invalid=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),invalid);
 for(let n=0;n<256;n++)equal(queue(t,proc,BigInt(n)),1n);equal(queue(t,1n),0n);equal(vm.readInt(t.error),8n);
 frame(t,'SleepEx',[0n,1n],'kernel32.dll');equal(core.wire_protect(t.handle,0x30000n,4096n,4),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(data+8n),0n);equal(core.wire_protect(t.handle,0x30000n,4096n,0x40),0);
 equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,4),0);await finish(t,17n);equal(vm.register(0),0xc0n);equal(vm.readInt(data+8n),256n);equal(vm.readInt(data,mode/8),255n);equal(k(t,'SleepEx',[0n,1n]),0n);vm.dispose();
}
// APCs queued by a running callback can enter a nested alertable wait.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,self=(1n<<BigInt(mode))-2n;
 for(const [address,digit]of [[0x31000n,2],[0x32000n,4]]){const a=asm(mode);a.mark(digit);a.ret();vm.write(address,a.bytes);}
 const a=asm(mode);a.mark(1);a.service(vm.bind('kernel32.dll','QueueUserAPC'),[0x31000n,self,0n]);a.service(vm.bind('kernel32.dll','SleepEx'),[0n,1n]);a.store(data);a.mark(3);a.ret();vm.write(0x30000n,a.bytes);equal(queue(t,0x30000n),1n);equal(queue(t,0x32000n),1n);frame(t,'SleepEx',[0n,1n],'kernel32.dll');await finish(t,1n);equal(vm.register(0),0xc0n);equal(vm.readInt(data),0xc0n);equal(vm.readInt(trace),0x1234n);equal(t.time.calls,0);vm.dispose();
}
// A callback can revoke the next callback frame's write permission without replaying its own work.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(mode);a.mark(1);a.service(vm.bind('kernel32.dll','VirtualProtect'),[t.sp&~4095n,4096n,2n,data+64n]);a.ret();vm.write(0x30000n,a.bytes);const b=asm(mode);b.mark(2);b.ret();vm.write(0x31000n,b.bytes);equal(queue(t,0x30000n),1n);equal(queue(t,0x31000n),1n);frame(t,'SleepEx',[0n,1n],'kernel32.dll');let fault;for(let n=0;n<1000;n++){fault=await vm.run(1n);if(fault.reason===Exit.Fault)break;equal(fault.reason,Exit.Budget);}equal(fault.reason,Exit.Fault);equal(vm.readInt(trace),1n);const before=snapshot(t);equal((await vm.run(1n)).reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,4),0);await finish(t,1n);equal(vm.readInt(trace),0x12n);equal(vm.register(0),0xc0n);vm.dispose();
}
// A queued APC wakes a blocked peer, executes on its TEB, then resumes its saved wait return.
for(const mode of [32,64])for(const wait of ['SleepEx','WSAWaitForMultipleEvents']){
 const t=await machine(mode),{vm}=t,a=asm(mode);if(wait.startsWith('WSA')){equal(w(t,'WSAStartup',[0x202n,0x22000n]),0n);const event=w(t,'WSACreateEvent');vm.write(data+80n,encode(event,mode/8));a.service(vm.bind('ws2_32.dll',wait),[1n,data+80n,0n,0xffffffffn,1n]);}else a.service(vm.bind('kernel32.dll',wait),[0xffffffffn,1n]);a.store(data);a.ret(73);vm.write(0x31000n,a.bytes);const sleeper=k(t,'CreateThread',[0n,65536n,0x31000n,0n,0n,data+64n]),tid=vm.readInt(data+64n);
 const callback=asm(mode);if(mode===32)callback.emit(0x8b,0x44,0x24,4);callback.store(data+8n,mode===32?0:1,mode/8);callback.service(vm.bind('kernel32.dll','GetCurrentThreadId'));callback.store(data+24n);callback.service(vm.bind('ws2_32.dll','WSASetLastError'),[55n]);callback.ret();vm.write(0x30000n,callback.bytes);
 const notifier=asm(mode);notifier.service(vm.bind('kernel32.dll','Sleep'),[0n]);notifier.service(vm.bind('kernel32.dll','QueueUserAPC'),[0x30000n,sleeper,0x8877665544332211n]);notifier.store(data+32n);notifier.ret(0);vm.write(0x32000n,notifier.bytes);k(t,'CreateThread',[0n,65536n,0x32000n,0n,0n,data+68n]);vm.write(t.error,encode(123n,4));frame(t,'WaitForSingleObject',[sleeper,0xffffffffn],'kernel32.dll');await finish(t,1n);equal(vm.register(0),0n);equal(vm.readInt(data),0xc0n);equal(vm.readInt(data+8n,mode/8),BigInt.asUintN(mode,0x8877665544332211n));equal(vm.readInt(data+24n),tid);equal(vm.readInt(data+32n),1n);equal(vm.readInt(t.error),123n);equal(k(t,'GetExitCodeThread',[sleeper,data+40n]),1n);equal(vm.readInt(data+40n),73n);equal(queue(t,0x30000n,0n,sleeper),0n);equal(vm.readInt(t.error),31n);equal(t.time.calls,0);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,self=(1n<<BigInt(mode))-2n,process=(1n<<BigInt(mode))-1n;
 equal(k(t,'DuplicateHandle',[process,self,process,data,0x40n,0n,0n]),1n);const restricted=vm.readInt(data,mode/8);equal(queue(t,1n,0n,restricted),0n);equal(vm.readInt(t.error),5n);equal(k(t,'CloseHandle',[restricted]),1n);
 equal(queue(t,vm.bind('ws2_32.dll','WSASetLastError'),42n),1n);frame(t,'SleepEx',[0n,1n],'kernel32.dll');const retired=vm.retired,work=vm.work;await finish(t,1n);equal(vm.register(0),0xc0n);equal(vm.readInt(t.error),42n);equal(vm.retired,retired);equal(vm.work,work+2n);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,7);equal(queue(t,counter(t,high),0x8877665544332211n),1n);frame(t,'SleepEx',[0n,1n],'kernel32.dll');await finish(t,1n);equal(vm.readInt(data,8),0x8877665544332211n);equal(vm.readInt(data+8n),1n);}vm.dispose();
}
// Exception callbacks nest inside the APC continuation without losing its saved caller.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,handler=asm(mode);handler.mark(2);handler.emit(0xb8,255,255,255,255,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x31000n,handler.bytes);assert.ok(k(t,'AddVectoredExceptionHandler',[1n,0x31000n]));const a=asm(mode);a.mark(1);a.service(vm.bind('kernel32.dll','RaiseException'),[0xe0010023n,0n,0n,0n]);a.mark(3);a.ret();vm.write(0x30000n,a.bytes);equal(queue(t,0x30000n),1n);vm.flags=0x8d5n;frame(t,'SleepEx',[0n,1n],'kernel32.dll');await finish(t,1n);equal(vm.register(0),0xc0n);equal(vm.readInt(trace),0x123n);equal(vm.flags,0x8d5n);vm.dispose();
}
// Exiting from an APC retires its continuation and never runs the worker's next instruction.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,a=asm(mode);a.service(vm.bind('kernel32.dll','SleepEx'),[0n,1n]);a.mark(15);a.ret();vm.write(0x30000n,a.bytes);const h=k(t,'CreateThread',[0n,65536n,0x30000n,0n,4n,data+64n]);equal(queue(t,vm.bind('kernel32.dll','ExitThread'),73n,h),1n);equal(k(t,'ResumeThread',[h]),1n);frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');await finish(t,1n);equal(vm.readInt(trace),0n);equal(k(t,'GetExitCodeThread',[h,data]),1n);equal(vm.readInt(data),73n);vm.dispose();
}
console.log(`${checks} APC queue/alertable wait/ABI/context/nesting/peer wake/fault checks passed`);
