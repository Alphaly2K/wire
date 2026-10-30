import assert from 'node:assert/strict';
import {WasmMachine,Exit} from '../web/runtime.mjs';
import {core,stateLayout,encode} from './x87-harness.mjs';
import {fixture} from './pe-fixture.mjs';
let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const snapshot=t=>new Uint8Array(core.memory.buffer,core.wire_state(t.handle),stateLayout[0]).slice();
async function machine(mode){
 let handle;const time={ticks:123456789n,calls:0};
 const vm=new WasmMachine({...core,wire_create(...a){handle=core.wire_create(...a);return handle;}},{mode,clock(){time.calls++;if(time.fail)throw time.fail;return time.ticks;},wallClock(){throw new Error('unexpected message wall clock');}});
 equal(vm.windows(),[]);const f=fixture(mode,{tls:false});f.put(f.dirs+8,0);f.put(f.dirs+12,0);f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));f.bytes[f.offset(0x1000)]=0xc3;f.relocations.splice(0,f.relocations.length,...f.relocations.filter(r=>r>=0x1100));f.rebuildRelocations();const image=vm.load(f.bytes);equal((await vm.run(100n)).reason,Exit.Returned);vm.map(0x20000n,0x30000n,7);
 return {vm,handle,mode,image,time,sp:image.sp-2048n,error:image.teb+BigInt(mode===32?0x34:0x68),mask:(1n<<BigInt(mode))-1n};
}
function frame(t,name,args=[],dll='user32.dll'){
 const {vm,mode,sp}=t;vm.ip=vm.bind(dll,name);vm.register(4,sp);vm.write(sp,encode(vm.entryPoints.returned,mode/8));for(let n=0;n<args.length;n++)if(mode===32)vm.write(sp+4n+BigInt(n*4),encode(args[n],4));else if(n<4)vm.register([1,2,8,9][n],args[n]);else vm.write(sp+40n+BigInt(n-4)*8n,encode(args[n],8));
}
function call(t,name,args=[],dll){frame(t,name,args,dll);const {vm}=t,w=vm.work,r=vm.retired;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,w+1n);equal(vm.retired,r);return vm.register(0);}
async function invoke(t,name,args=[]){frame(t,name,args);for(let n=0;n<1000;n++){const r=await t.vm.run(1000n);if(r.reason===Exit.Returned)return t.vm.register(0);equal(r.reason,Exit.Budget);}throw new Error('message call did not finish');}
function text(t,p,s,wide){t.vm.write(p,wide?Uint8Array.from([...s+'\0'].flatMap(c=>[c.charCodeAt(0)&255,c.charCodeAt(0)>>8])):new TextEncoder().encode(s+'\0'));}
async function window(t,wide=true){
 const {vm,mode}=t,p=0x20000n;vm.write(p,new Uint8Array(80));for(const [at,value,size] of [[0,mode===32?48n:80n,4],[8,vm.bind('user32.dll',wide?'DefWindowProcW':'DefWindowProcA'),mode/8],[mode===32?20:24,t.image.base,mode/8],[mode===32?40:64,0x20100n,mode/8]])vm.write(p+BigInt(at),encode(value,size));
 text(t,0x20100n,wide?'QueueClassW':'QueueClassA',wide);text(t,0x20180n,wide?'消息窗口':'Queue title',wide);const atom=call(t,wide?'RegisterClassExW':'RegisterClassExA',[p]);assert.ok(atom);
 frame(t,wide?'CreateWindowExW':'CreateWindowExA',[8n,atom,0x20180n,0n,10n,20n,640n,480n,0n,0n,t.image.base,0n]);equal(vm.windows(),[]);equal((await vm.run(1000n)).reason,Exit.Returned);const h=vm.register(0);assert.ok(h);return h;
}
function msg(t,p=0x21000n){const {vm,mode}=t,offsets=mode===32?[0,4,8,12,16]:[0,8,16,24,32],widths=mode===32?[4,4,4,4,4]:[8,4,8,8,4];return offsets.map((o,n)=>vm.readInt(p+BigInt(o),widths[n]));}
function point(t,p=0x21000n){const at=p+BigInt(t.mode===32?20:36);return [0n,4n].map(o=>Number(BigInt.asIntN(32,t.vm.readInt(at+o))));}
const peek=(t,h=0n,min=0n,max=0n,flags=1n,out=0x21000n)=>call(t,'PeekMessageW',[out,h,min,max,flags]);
const host=(window,message=0x8001,wparam=0n,lparam=0n,time=57,point)=>({window,message,wparam,lparam,time,...(point?{point}:{})});

// MSG widths, filters, metadata, output faults and saved return/output aliases.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),wp=0xfedcba9876543210n&t.mask,lp=0xabcdef0123456789n&t.mask;
 equal(vm.windows(),[{handle:h,title:'消息窗口',width:640,height:480,visible:false,topmost:true}]);const before=snapshot(t);vm.queueWindowEvent(host(h,0x8001,wp,lp,57,[-7,123]));equal(snapshot(t),before);equal(call(t,'SetMessageExtraInfo',[wp]),0n);
 equal(peek(t,0n,0x8002n,0x8002n),0n);equal(call(t,'GetMessageExtraInfo'),wp);equal(peek(t,0n,0n,0n,0n),1n);equal(msg(t),[h,0x8001n,wp,lp,57n]);equal(point(t),[-7,123]);equal(call(t,'GetMessagePos'),BigInt((123<<16)|65529));equal(call(t,'GetMessageTime'),57n);equal(call(t,'GetMessageExtraInfo'),0n);
 call(t,'SetMessageExtraInfo',[wp]);const out=0x24ff0n;vm.write(out,new Uint8Array(64).fill(0xcc));equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);frame(t,'PeekMessageW',[out,0n,0n,0n,1n]);const cpu=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),cpu);equal(vm.readInt(out,8),0xccccccccccccccccn);equal(call(t,'GetMessageExtraInfo'),wp);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(peek(t,0n,0n,0n,1n,out),1n);equal(msg(t,out),[h,0x8001n,wp,lp,57n]);equal(peek(t),0n);
 vm.queueWindowEvent(host(h,0x8001));frame(t,'GetMessageW',[t.sp,0n,0n,0n]);equal(vm.service().reason,Exit.Dispatch);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(4),t.sp+BigInt(mode===32?20:8));equal(msg(t,t.sp)[0],h);
 equal(call(t,'PostMessageW',[h,0x8002n,wp,lp]),1n);equal(t.time.calls,1);equal(peek(t,t.mask),0n);equal(peek(t,h,0x8002n,0x8002n),1n);equal(msg(t),[h,0x8002n,wp,lp,12345n]);equal(point(t),[-7,123]);
 equal(call(t,'PostThreadMessageA',[1n,0x8003n,wp,lp]),1n);equal(peek(t,h),0n);equal(peek(t,t.mask),1n);equal(msg(t).slice(0,4),[0n,0x8003n,wp,lp]);equal(call(t,'DispatchMessageW',[0x21000n]),0n);equal(vm.register(4),t.sp+8n);
 call(t,'PostQuitMessage',[73n]);equal(call(t,'GetMessageA',[0x21000n,h,1n,1n]),0n);equal(msg(t).slice(0,4),[0n,0x12n,73n,0n]);equal(peek(t),0n);
 equal(call(t,'PostMessageA',[h,0xcn,0n,0n]),0n);equal(vm.readInt(t.error),1159n);equal(call(t,'PostThreadMessageW',[999n,0x8000n,0n,0n]),0n);equal(vm.readInt(t.error),1444n);equal(call(t,'GetMessageW',[0x21000n,h+1n,0n,0n]),0xffffffffn);equal(vm.readInt(t.error),1400n);equal(peek(t,0n,9n,8n),0n);equal(vm.readInt(t.error),87n);
 text(t,0x20400n,'user32.dll',true);text(t,0x20440n,'WaitMessage',false);const provider=call(t,'GetModuleHandleW',[0x20400n],'kernel32.dll'),wait=call(t,'GetProcAddress',[provider,0x20440n],'kernel32.dll');equal(wait,vm.bind('user32.dll','WaitMessage'));equal(call(t,'GetModuleHandleExW',[6n,wait,0x20480n],'kernel32.dll'),1n);equal(vm.readInt(0x20480n,mode/8),provider);

 assert.throws(()=>vm.bind('kernel32.dll','WaitMessage'),/unknown|migrated/);assert.throws(()=>vm.bind('kernel32.dll','MsgWaitForMultipleObjects'),/unknown|migrated/);
 await invoke(t,'ShowWindow',[h,1n]);equal(vm.windows()[0].visible,true);vm.queueWindowEvent(host(h,0x10));equal(peek(t),1n);equal(await invoke(t,'DispatchMessageW',[0x21000n]),0n);equal(vm.register(4),t.sp+8n);equal(vm.windows(),[]);equal(peek(t),0n);vm.dispose();
}

// WaitMessage checks new input; inspected old messages and unmatched filters do not busy-loop.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);vm.queueWindowEvent(host(h));equal(call(t,'WaitMessage'),1n);frame(t,'WaitMessage');const w=vm.work,cpu=snapshot(t);equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,w);equal(snapshot(t),cpu);equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,w);vm.write(t.sp,encode(0xdeadbeefn,mode/8));vm.queueWindowEvent(host(h,0x8002));equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),1n);equal(vm.work,w+1n);
 equal(peek(t,0n,0n,0n,0n),1n);frame(t,'WaitMessage');equal((await vm.run(100n)).reason,Exit.Waiting);vm.queueWindowEvent(host(h,0x8003));equal((await vm.run(100n)).reason,Exit.Returned);
 frame(t,'GetMessageA',[0x22000n,h,0x8005n,0x8005n]);const work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);vm.write(t.sp,encode(0xbadn,mode/8));if(mode===32)vm.write(t.sp+4n,encode(0x23000n,4));vm.queueWindowEvent(host(h,0x8004));equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);vm.queueWindowEvent(host(h,0x8005,73n));equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(msg(t,0x22000n)[1],0x8005n);equal(vm.readInt(0x23000n),0n);equal(vm.work,work+1n);equal(t.time.calls,0);vm.dispose();
}

// Host ingress is atomic; stale handles, signed mouse coordinates, timestamps and text keep their identity.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,h=await window(t,wide);const invalid=()=>equal(peek(t),0n);
 assert.throws(()=>vm.queueWindowEvents([host(h),host(h,0xc)]),/unsupported/);invalid();
 assert.throws(()=>vm.queueWindowEvents([host(h),host(h,0x8001,0n,0n,1,[0,2**31])]),/integer/);invalid();assert.throws(()=>vm.queueWindowEvent(host(Number(h))),/BigInt/);invalid();
 const pointer=core.wire_input(t.handle,96);new Uint8Array(core.memory.buffer,pointer,96).fill(0);const raw=new DataView(core.memory.buffer,pointer,96);for(let n=0;n<2;n++){raw.setBigUint64(n*48,h,true);raw.setUint32(n*48+8,0x8001,true);}raw.setUint32(92,1,true);equal(core.wire_queue_window_events(t.handle),1);invalid();
 vm.queueWindowEvent(host(h+(1n<<32n)));invalid();vm.queueWindowEvent(host(h,0x200,0n,BigInt.asUintN(32,BigInt(((-2&65535)<<16)|(-3&65535)))));equal(peek(t),1n);equal(point(t),[7,18]);equal(call(t,'GetMessagePos'),BigInt((18<<16)|7));
 vm.queueWindowEvent(host(h,0x20a,0n,BigInt.asUintN(32,BigInt.asUintN(32,BigInt(((-2&65535)<<16)|(-3&65535))))));equal(peek(t),1n);equal(point(t),[-3,-2]);
 vm.queueWindowEvents([host(h,0x100,65n,0x1e0001n,61),host(h,0x102,0x3042n,0x1e0001n,62)]);equal(peek(t),1n);equal(msg(t)[1],0x100n);equal(call(t,'TranslateMessage',[0x21000n]),1n);equal(t.time.calls,1);equal(peek(t),1n);equal(msg(t).slice(1,3),[0x102n,wide?0x3042n:0x82n]);equal(msg(t)[4],12345n);if(!wide){equal(peek(t),1n);equal(msg(t).slice(1,3),[0x102n,0xa0n]);}invalid();
 vm.queueWindowEvent({...host(h),time:undefined});equal(t.time.calls,2);equal(peek(t),1n);equal(msg(t)[4],12345n);equal(call(t,'TranslateMessage',[0x21000n]),0n);equal(t.time.calls,2);
 const owned=vm.windows();owned[0].title='changed';equal(vm.windows()[0].title,wide?'消息窗口':'Queue title');
 vm.write(0x33000n,[0x90,0xeb,0xfd]);vm.ip=0x33000n;const running=vm.run(1n);assert.throws(()=>vm.windows(),/already running/);assert.throws(()=>vm.queueWindowEvents([]),/already running/);await running;vm.dispose();assert.throws(()=>vm.windows(),/disposed|closed/);
}

// Dispatch enters the JIT WNDPROC and preserves the original one-argument return frame.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),value=mode===32?0x76543210n:0xfedcba9876543210n;
 vm.write(0x30000n,[...(mode===64?[0x48]:[]),0xb8,...encode(value,mode/8),...(mode===32?[0xc2,16,0]:[0xc3])]);call(t,mode===32?'SetWindowLongW':'SetWindowLongPtrW',[h,0xfffffffcn,0x30000n]);vm.queueWindowEvent(host(h));equal(peek(t),1n);const w=vm.work,r=vm.retired;equal(await invoke(t,'DispatchMessageA',[0x21000n]),value);equal(vm.register(4),t.sp+8n);equal(vm.work-w,vm.retired-r+1n);
 vm.queueWindowEvent(host(h));call(t,'SetMessageExtraInfo',[73n]);equal(core.wire_protect(t.handle,0x21000n,4096n,0x104),0);frame(t,'GetMessageW',[0x21000n,0n,0n,0n]);const cpu=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),cpu);equal(vm.service().reason,Exit.Dispatch);equal(msg(t)[0],h);equal(peek(t),0n);vm.dispose();
}

// A worker's blocked GetMessage owns its captured output/filter/return despite shared stack writes.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,b=[];const emit=(...v)=>b.push(...v),value=(v,n)=>emit(...encode(v,n));
 if(mode===32){emit(0x89,0xe0,0xa3);value(0x22080n,4);}else{emit(0x48,0x89,0xe0,0x49,0xbb);value(0x22080n,8);emit(0x49,0x89,0x03);}
 const service=(name,args,dll='user32.dll')=>{if(mode===32){for(const a of args.toReversed()){emit(0x68);value(a,4);}emit(0xb8);value(vm.bind(dll,name),4);emit(0xff,0xd0);}else{emit(0x48,0x83,0xec,56);for(let n=0;n<args.length;n++){emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n]);value(args[n],8);}emit(0x48,0xb8);value(vm.bind(dll,name),8);emit(0xff,0xd0,0x48,0x83,0xc4,56);}};
 service('PostThreadMessageW',[1n,0x8009n,1n,0n]);service('GetMessageW',[0x22000n,0n,0x8006n,0x8006n]);service('PostThreadMessageW',[1n,0x8007n,73n,0n]);service('PostThreadMessageW',[2n,0x8008n,1n,0n]);emit(0xb8,9,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x30000n,b);
 const h=call(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0x20100n],'kernel32.dll');assert.ok(h);equal(vm.readInt(0x20100n),2n);frame(t,'GetMessageW',[0x21000n,0n,0x8009n,0x8009n]);equal((await vm.run(1000n)).reason,Exit.Returned);const sp=vm.readInt(0x22080n,mode/8)-BigInt(mode===32?20:64);vm.write(sp,encode(0xbadn,mode/8));if(mode===32)vm.write(sp+4n,encode(0x23000n,4));
 equal(call(t,'PostThreadMessageA',[2n,0x8005n,0n,0n]),1n);equal(call(t,'PostThreadMessageW',[2n,0x8006n,42n,0n]),1n);frame(t,'GetMessageW',[0x21000n,0n,0x8007n,0x8007n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(msg(t,0x22000n).slice(0,3),[0n,0x8006n,42n]);equal(vm.readInt(0x23000n),0n);equal(msg(t).slice(0,3),[0n,0x8007n,73n]);
 frame(t,'WaitForSingleObject',[h,0xffffffffn],'kernel32.dll');equal((await vm.run(1000n)).reason,Exit.Returned);equal(call(t,'GetExitCodeThread',[h,0x20108n],'kernel32.dll'),1n);equal(vm.readInt(0x20108n),9n);equal(call(t,'PostThreadMessageW',[2n,0x8000n,0n,0n]),0n);equal(vm.readInt(t.error),1444n);equal(call(t,'CloseHandle',[h],'kernel32.dll'),1n);vm.dispose();
}

// Queue capacity is bounded and destruction releases all queued messages.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),batch=Array.from({length:256},()=>host(h));for(let n=0;n<16;n++)vm.queueWindowEvents(batch);assert.throws(()=>vm.queueWindowEvent(host(h)),/capacity/);equal(call(t,'PostMessageW',[h,0x8001n,0n,0n]),0n);equal(vm.readInt(t.error),1816n);equal(peek(t),1n);equal(call(t,'PostMessageW',[h,0x8001n,0n,0n]),1n);await invoke(t,'DestroyWindow',[h]);equal(peek(t),0n);vm.dispose();
}
// Category checks are independent, and INPUTAVAILABLE deliberately accepts previously inspected input.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),wait=(mask,flags=0n)=>call(t,'MsgWaitForMultipleObjectsEx',[0n,0xdead0000n,0n,mask,flags]);
 vm.queueWindowEvents([host(h,0x100,65n),host(h,0x8001)]);equal(call(t,'GetQueueStatus',[1n]),0x10001n);equal(call(t,'GetQueueStatus',[1n]),0x10000n);equal(wait(1n),258n);equal(wait(1n,4n),0n);equal(wait(8n),0n);equal(wait(8n),258n);equal(call(t,'GetQueueStatus',[0x100n]),0x1000100n);equal(wait(0x100n),258n);equal(peek(t,0n,0n,0n,0n),1n);equal(wait(1n|8n),258n);equal(wait(1n,4n),0n);equal(peek(t),1n);equal(peek(t),1n);equal(call(t,'GetQueueStatus',[0x1dffn]),0n);
 equal(wait(0x200n),0xffffffffn);equal(vm.readInt(t.error),87n);equal(wait(1n,2n),258n);equal(vm.readInt(t.error),87n);equal(wait(1n,8n),0xffffffffn);equal(vm.readInt(t.error),87n);equal(t.time.calls,0);vm.dispose();
}

// Message/object wait-any uses object order; wait-all consumes resources only with matching input present.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),event=call(t,'CreateEventW',[0n,0n,1n,0n],'kernel32.dll');vm.write(0x22000n,encode(event,mode/8));
 const wait=(all=0n,flags=4n)=>call(t,'MsgWaitForMultipleObjectsEx',[1n,0x22000n,0n,1n,all|flags]);
 equal(wait(1n),258n);vm.queueWindowEvent(host(h,0x100,65n));equal(wait(1n),0n);equal(call(t,'WaitForSingleObject',[event,0n],'kernel32.dll'),258n);equal(wait(),1n);equal(call(t,'SetEvent',[event],'kernel32.dll'),1n);equal(wait(),0n);equal(wait(),1n);
 vm.write(0x22000n,[...encode(event,mode/8),...encode(event,mode/8)]);equal(call(t,'MsgWaitForMultipleObjects',[2n,0x22000n,1n,0n,1n]),0xffffffffn);equal(vm.readInt(t.error),87n);vm.write(0x22000n,new Uint8Array(63*mode/8));for(let n=0;n<63;n++)vm.write(0x22000n+BigInt(n*mode/8),encode(event,mode/8));equal(call(t,'MsgWaitForMultipleObjects',[63n,0x22000n,0n,0n,1n]),258n);equal(call(t,'MsgWaitForMultipleObjects',[64n,0xdead0000n,0n,0n,1n]),0xffffffffn);equal(vm.readInt(t.error),87n);
 vm.write(0x22000n,encode(event,mode/8));vm.queueWindowEvents([host(h,0x8001)]);call(t,'GetQueueStatus',[8n]);frame(t,'MsgWaitForMultipleObjects',[1n,0x22000n,0n,5n,8n]);const work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);vm.write(t.sp,encode(0xbadn,mode/8));vm.write(0x22000n,encode(0xbadn,mode/8));t.time.ticks+=49999n;equal((await vm.run(100n)).reason,Exit.Waiting);t.time.ticks++;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),258n);equal(vm.work,work+1n);equal(call(t,'CloseHandle',[event],'kernel32.dll'),1n);vm.dispose();
}

// A finite message-only wait honors the original mask, timeout and saved frame through host wakeups.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);frame(t,'MsgWaitForMultipleObjectsEx',[0n,0n,5n,1n,0n]);const work=vm.work,cpu=snapshot(t);equal((await vm.run(100n)).reason,Exit.Waiting);equal(snapshot(t),cpu);vm.write(t.sp,encode(0xbadn,mode/8));vm.queueWindowEvent(host(h,0x8001));equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);vm.queueWindowEvent(host(h,0x100,65n));equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),0n);equal(vm.work,work+1n);vm.dispose();
}

// Message timers coalesce, retain queued messages across KillTimer, and allocate noncolliding full-width IDs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),start=t.time.ticks;
 equal(call(t,'SetTimer',[h,1n,10n,0n]),1n);const second=call(t,'SetTimer',[h,0n,0n,0n]);equal(second,2n);equal(call(t,'KillTimer',[h,second]),1n);equal(call(t,'GetQueueStatus',[1n]),0n);equal(t.time.calls,2);t.time.ticks=start+99999n;equal(peek(t),0n);t.time.ticks++;equal(peek(t,0n,0n,0n,0n),1n);equal(msg(t).slice(0,4),[h,0x113n,1n,0n]);const stamp=msg(t)[4];t.time.ticks+=10000000n;equal(peek(t,0n,0n,0n,0n),1n);equal(msg(t)[4],stamp);equal(call(t,'KillTimer',[h,1n]),1n);equal(peek(t),1n);equal(peek(t),0n);const calls=t.time.calls;equal(call(t,'GetQueueStatus',[0x10n]),0n);equal(t.time.calls,calls);
 const full=mode===32?0x76543210n:0xfedcba9876543210n;equal(call(t,'SetTimer',[h,full,10n,0n]),full);frame(t,'GetMessageW',[0x21000n,h,0x113n,0x113n]);const work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);t.time.ticks+=100000n;equal((await vm.run(100n)).reason,Exit.Returned);equal(msg(t).slice(0,4),[h,0x113n,full,0n]);equal(call(t,'KillTimer',[h,full]),1n);
 const id=call(t,'SetTimer',[0n,999n,10n,0n]);assert.notEqual(id,999n);equal(call(t,'SetTimer',[0n,id,20n,0n]),id);t.time.ticks+=100000n;equal(peek(t),0n);t.time.ticks+=100000n;equal(peek(t),1n);equal(msg(t).slice(0,4),[0n,0x113n,id,0n]);equal(call(t,'KillTimer',[0n,id]),1n);vm.dispose();
}

// TIMERPROC uses the original DispatchMessage ABI, fresh DWORD time, and a void result.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,proc=0x30000n,kill=vm.bind('user32.dll','KillTimer'),b=[];const emit=(...v)=>b.push(...v),value=(v,n)=>emit(...encode(v,n));
 if(mode===32){for(let n=0;n<4;n++){emit(0x8b,0x44,0x24,4+n*4,0xa3);value(0x22000n+BigInt(n*8),4);}emit(0xff,0x74,0x24,12,0x6a,0,0xb8);value(kill,4);emit(0xff,0xd0,0xb8,73,0,0,0,0xc2,16,0);}else{for(let n=0;n<4;n++){emit(0x49,0xbb);value(0x22000n+BigInt(n*8),8);emit(...[[0x49,0x89,0x0b],[0x49,0x89,0x13],[0x4d,0x89,0x03],[0x4d,0x89,0x0b]][n]);}emit(0x48,0x83,0xec,56,0x31,0xc9,0x4c,0x89,0xc2,0x48,0xb8);value(kill,8);emit(0xff,0xd0,0x48,0x83,0xc4,56,0xb8,73,0,0,0,0xc3);}vm.write(proc,b);
 const id=call(t,'SetTimer',[0n,0n,10n,proc]);t.time.ticks+=100000n;equal(peek(t),1n);t.time.ticks+=50000n;const tick=t.time.ticks/10000n;const work=vm.work,retired=vm.retired;equal(await invoke(t,'DispatchMessageW',[0x21000n]),0n);equal(vm.register(4),t.sp+8n);equal([0,1,2,3].map(n=>vm.readInt(0x22000n+BigInt(n*8),mode/8)),[0n,0x113n,id,tick]);equal(vm.work-work,vm.retired-retired+2n);equal(call(t,'KillTimer',[0n,id]),0n);vm.dispose();
}

// Pending mixed waits retain object identities after another thread closes the original handle and edits the array.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,event=call(t,'CreateEventW',[0n,0n,0n,0n],'kernel32.dll');equal(call(t,'DuplicateHandle',[t.mask,event,t.mask,0x22100n,0n,0n,2n],'kernel32.dll'),1n);const alias=vm.readInt(0x22100n,mode/8);vm.write(0x22000n,encode(event,mode/8));
 const b=[];const emit=(...v)=>b.push(...v),value=(v,n)=>emit(...encode(v,n));const api=(name,args)=>{if(mode===32){for(const v of args.toReversed())emit(0x68,...encode(v,4));emit(0xb8,...encode(vm.bind('kernel32.dll',name),4),0xff,0xd0);}else{emit(0x48,0x83,0xec,56);for(let n=0;n<args.length;n++)emit(...[[0x48,0xb9],[0x48,0xba]][n],...encode(args[n],8));emit(0x48,0xb8,...encode(vm.bind('kernel32.dll',name),8),0xff,0xd0,0x48,0x83,0xc4,56);}};
 api('CloseHandle',[event]);if(mode===32)emit(0xc7,0x05,...encode(0x22000n,4),0xad,0x0b,0,0);else emit(0x49,0xbb,...encode(0x22000n,8),0x41,0xc7,0x03,0xad,0x0b,0,0);api('SetEvent',[alias]);emit(0xb8,73,0,0,0,...(mode===32?[0xc2,4,0]:[0xc3]));vm.write(0x30000n,b);const thread=call(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,0n],'kernel32.dll');frame(t,'MsgWaitForMultipleObjects',[1n,0x22000n,0n,0xffffffffn,1n]);equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(call(t,'WaitForSingleObject',[alias,0n],'kernel32.dll'),258n);equal(vm.readInt(0x22000n),0xbadn);equal(call(t,'GetExitCodeThread',[thread,0x22110n],'kernel32.dll'),1n);equal(vm.readInt(0x22110n),73n);equal(call(t,'CloseHandle',[thread],'kernel32.dll'),1n);equal(call(t,'CloseHandle',[alias],'kernel32.dll'),1n);equal(t.time.calls,0);vm.dispose();
}

// Both message and waitable timers wake mixed waits without consuming work while blocked.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,id=call(t,'SetTimer',[0n,0n,10n,0n]);frame(t,'WaitMessage');let work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);t.time.ticks+=100000n;equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);equal(call(t,'GetQueueStatus',[0x10n]),0x100000n);equal(peek(t),1n);
 frame(t,'MsgWaitForMultipleObjectsEx',[0n,0n,0xffffffffn,0x10n,0n]);work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);t.time.ticks+=100000n;equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(call(t,'KillTimer',[0n,id]),1n);equal(peek(t),1n);
 const timer=call(t,'CreateWaitableTimerW',[0n,0n,0n],'kernel32.dll');vm.write(0x22000n,encode(timer,mode/8));vm.write(0x22100n,encode(BigInt.asUintN(64,-100000n),8));equal(call(t,'SetWaitableTimer',[timer,0x22100n,0n,0n,0n,0n],'kernel32.dll'),1n);frame(t,'MsgWaitForMultipleObjectsEx',[1n,0x22000n,0xffffffffn,1n,0n]);work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,work);t.time.ticks+=100000n;equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(call(t,'WaitForSingleObject',[timer,0n],'kernel32.dll'),258n);equal(call(t,'CloseHandle',[timer],'kernel32.dll'),1n);vm.dispose();
}

// Timer capacity rejects before allocating an identity; release permits a fresh monotonic ID.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;let first,last;for(let n=0;n<1024;n++){const id=call(t,'SetTimer',[0n,0n,10n,0n]);if(n===0)first=id;last=id;assert.ok(id);}
 equal(call(t,'SetTimer',[0n,0n,10n,0n]),0n);equal(vm.readInt(t.error),8n);equal(call(t,'KillTimer',[0n,first]),1n);equal(call(t,'SetTimer',[0n,0n,10n,0n]),last+1n);vm.dispose();
}

// The last representable deadline fires once; message time wraps as a DWORD and overflow rejects a new timer.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,maximum=(1n<<64n)-1n;t.time.ticks=maximum-100000n;const id=call(t,'SetTimer',[0n,0n,10n,0n]);equal(call(t,'SetTimer',[0n,0n,20n,0n]),0n);equal(vm.readInt(t.error),87n);t.time.ticks=maximum;equal(peek(t),1n);equal(msg(t)[4],(maximum/10000n)&0xffffffffn);const calls=t.time.calls;equal(peek(t),0n);equal(t.time.calls,calls);equal(call(t,'KillTimer',[0n,id]),1n);vm.dispose();
}

// Clock failure does not replace a timer; window destruction releases timers and stops sampling.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);equal(call(t,'SetTimer',[h,73n,10n,0n]),73n);frame(t,'SetTimer',[h,73n,1000n,0n]);const cpu=snapshot(t);t.time.fail=new Error('clock source failed');assert.throws(()=>vm.service(),/clock source failed/);equal(snapshot(t),cpu);t.time.fail=null;t.time.ticks+=100000n;equal(peek(t),1n);equal(msg(t)[2],73n);await invoke(t,'DestroyWindow',[h]);const calls=t.time.calls;equal(peek(t),0n);equal(t.time.calls,calls);vm.dispose();
}

console.log(`${checks} message queue/MSG/wait/host-event/dispatch checks passed`);
