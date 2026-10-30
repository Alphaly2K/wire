import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window,invoke,finish} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';
const names=['RegisterRawInputDevices','GetRegisteredRawInputDevices','GetRawInputDeviceList','GetRawInputBuffer','GetRawInputDeviceInfoA','GetRawInputDeviceInfoW','GetRawInputData'];
const devices=0x22000n,size=0x23000n,out=0x24000n,msg=0x21000n;
const data=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const header=t=>BigInt(t.mode===32?16:24),stride=t=>BigInt(t.mode===32?12:16);
const lp=t=>msg+BigInt(t.mode===32?12:24);
const host=(window,message=0x200,wparam=0n,lparam=0n,point=[512,384])=>({window,message,wparam,lparam,time:55,point});
const peek=(t,remove=1n,min=0xffn,max=min)=>call(t,'PeekMessageW',[msg,0n,min,max,remove]);
function registration(t,rows,p=devices){t.vm.write(p,rows.flatMap(([usage,flags=0,target=0n,page=1])=>[...encode(BigInt(page),2),...encode(BigInt(usage),2),...encode(BigInt(flags),4),...encode(target,t.mode/8)]));return call(t,'RegisterRawInputDevices',[p,BigInt(rows.length),stride(t)]);}
function raw(t,handle,command=0x10000003n,p=out){
 equal(call(t,'GetRawInputData',[handle,command,0n,size,header(t)]),0n);const n=t.vm.readInt(size);equal(call(t,'GetRawInputData',[handle,command,p,size,header(t)]),n);return data(t.vm,p,Number(n));
}
async function setup(mode){const t=await machine(mode);t.hwnd=await window(t);return t;}

// Each ABI registers the complete batch or preserves the original registration map.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;
 for(const name of names){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x72));vm.write(t.error,encode(123n,4));
 equal(registration(t,[[6,0,hwnd],[2,0x2000,hwnd]]),1n);equal(call(t,'GetRegisteredRawInputDevices',[0n,size,stride(t)]),0n);equal(vm.readInt(size),2n);
 equal(call(t,'GetRegisteredRawInputDevices',[out,size,stride(t)]),2n);equal(vm.readInt(out,2),1n);equal(vm.readInt(out+2n,2),2n);equal(vm.readInt(out+4n),0x2000n);equal(vm.readInt(out+8n,mode/8),hwnd);equal(vm.readInt(out+stride(t)+2n,2),6n);
 const before=data(vm,out,Number(stride(t)*2n));
 for(const [rows,error]of [[[[2,1,hwnd]],87n],[[[0,0]],87n],[[[2,0x100]],50n],[[[2,0,0x1234n]],1400n],[[[2,0],[6,0,0n,2]],50n]]){equal(registration(t,rows),0n);equal(vm.readInt(t.error),error);vm.write(size,encode(2n,4));equal(call(t,'GetRegisteredRawInputDevices',[out,size,stride(t)]),2n);equal(data(vm,out,Number(stride(t)*2n)),before);}
 for(const [pointer,count,entry]of [[devices+1n,1n,stride(t)],[devices,0n,stride(t)],[devices,1025n,stride(t)],[0x50000n,1n,stride(t)+4n]]){equal(call(t,'RegisterRawInputDevices',[pointer,count,entry]),0n);equal(vm.readInt(t.error),87n);}
 equal(registration(t,[[2,1],[6,1]]),1n);equal(call(t,'GetRegisteredRawInputDevices',[0n,size,stride(t)]),0n);equal(vm.readInt(size),0n);
 equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x72));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Device enumeration, A/W names and RID_DEVICE_INFO use the logical device state.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,entry=BigInt(mode===32?8:16);
 equal(call(t,'GetRawInputDeviceList',[0n,size,entry]),0n);equal(vm.readInt(size),2n);vm.write(out,new Uint8Array(40).fill(0xa5));equal(call(t,'GetRawInputDeviceList',[out,size,entry]),2n);equal(vm.readInt(out,mode/8),0x57010001n);equal(vm.readInt(out+BigInt(mode/8)),0n);equal(vm.readInt(out+entry,mode/8),0x57010002n);equal(vm.readInt(out+entry+BigInt(mode/8)),1n);
 for(const [h,name,kind]of [[0x57010001n,'MOUSE',0n],[0x57010002n,'KEYBOARD',1n]])for(const wide of [false,true]){
  const api='GetRawInputDeviceInfo'+(wide?'W':'A'),wanted=Array.from(Buffer.from('\\\\?\\WIRE#'+name+'#1\0',wide?'utf16le':'ascii'));
  equal(call(t,api,[h,0x20000007n,0n,size]),0n);equal(vm.readInt(size),BigInt(wanted.length/(wide?2:1)));equal(call(t,api,[h,0x20000007n,out,size]),vm.readInt(size));equal(data(vm,out,wanted.length),wanted);
  vm.write(size,encode(32n,4));vm.write(out,encode(32n,4));equal(call(t,api,[h,0x2000000bn,out,size]),32n);equal(vm.readInt(out),32n);equal(vm.readInt(out+4n),kind);equal(vm.readInt(size),32n);
  vm.write(out,encode(0n,4));equal(call(t,api,[h,0x2000000bn,out,size]),0xffffffffn);equal(vm.readInt(t.error),87n);
 }
 equal(call(t,'GetRawInputDeviceInfoA',[0x57010001n+(1n<<32n),0n,out,size]),0xffffffffn);equal(vm.readInt(t.error),mode===32?87n:6n);
 vm.write(size,encode(1n,4));equal(call(t,'GetRawInputDeviceList',[out,size,entry]),0xffffffffn);equal(vm.readInt(size),2n);equal(vm.readInt(t.error),122n);vm.dispose();
}

// Real host mouse/keyboard transitions create independently owned packets with exact headers.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2],[6]]),1n);const head=Number(header(t));let last=0n;
 for(const [message,key,lparam,kind,flags,detail]of [[0x200,0n,0n,0,0,0],[0x201,1n,0n,0,1,0],[0x203,1n,0n,0,1,0],[0x202,0n,0n,0,2,0],[0x206,2n,0n,0,4,0],[0x209,0x10n,0n,0,16,0],[0x20d,0x10000n,0n,0,64,0],[0x20b,0x20000n,0n,0,256,0],[0x20a,0xff880000n,0n,0,0x400,0xff88],[0x20e,0x780000n,0n,0,0x800,120],[0x104,18n,0x01380001n,1,2,0x38],[0x105,18n,0xc1380001n,1,3,0x38]]){
  vm.queueWindowEvent(host(hwnd,message,key,lparam));equal(peek(t,0n),1n);const handle=vm.readInt(lp(t),mode/8);assert.ok(handle>last);last=handle;
  const bytes=raw(t,handle);equal(bytes.length,head+(kind===0?24:16));equal(vm.readInt(out),BigInt(kind));equal(vm.readInt(out+4n),BigInt(bytes.length));equal(vm.readInt(out+8n,mode/8),kind===0?0x57010001n:0x57010002n);equal(vm.readInt(out+8n+BigInt(mode/8),mode/8),0n);
  if(kind===0){equal(vm.readInt(out+BigInt(head),2),3n);equal(vm.readInt(out+BigInt(head+4),2),BigInt(flags));equal(vm.readInt(out+BigInt(head+6),2),BigInt(detail));equal(vm.readInt(out+BigInt(head+12)),32768n);equal(vm.readInt(out+BigInt(head+16)),32768n);}else{equal(vm.readInt(out+BigInt(head),2),BigInt(detail));equal(vm.readInt(out+BigInt(head+2),2),BigInt(flags));equal(vm.readInt(out+BigInt(head+6),2),key);equal(vm.readInt(out+BigInt(head+8)),BigInt(message));}
  equal(raw(t,handle,0x10000005n).length,head);equal(peek(t),1n);equal(raw(t,handle),bytes);equal(peek(t,1n,BigInt(message)),1n);equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,handle]),0n);equal(call(t,'GetRawInputData',[handle,0x10000003n,0n,size,header(t)]),0xffffffffn);equal(vm.readInt(t.error),6n);
 }
 // A default cleanup before Peek removes the corresponding hardware message as well.
 vm.queueWindowEvent(host(hwnd));equal(peek(t,0n),1n);const handle=vm.readInt(lp(t),mode/8);equal(call(t,'DefWindowProcA',[hwnd,0xffn,0n,handle]),0n);equal(peek(t),0n);vm.dispose();
}

// NOLEGACY suppresses text and ordinary mouse input, including when pointer promotion is enabled.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2,0x30],[6,0x30]]),1n);equal(call(t,'EnableMouseInPointer',[1n]),1n);
 frame(t,'MsgWaitForMultipleObjectsEx',[0n,0n,0xffffffffn,0x400n,4n]);const waitWork=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(vm.work,waitWork);
 vm.queueWindowEvents([host(hwnd,0x100,65n,0x1e0001n),host(hwnd,0x102,65n),host(hwnd,0x103,65n),host(hwnd,0x106,65n),host(hwnd,0x107,65n),host(hwnd,0x109,65n)]);
 equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.work,waitWork+1n);equal(call(t,'GetQueueStatus',[0x400n])&0x4000000n,0x4000000n);
 equal(call(t,'GetAsyncKeyState',[65n])&0x8000n,0x8000n);equal(call(t,'GetKeyState',[65n])&0x8000n,0n);equal(peek(t,0n),1n);equal(call(t,'GetKeyState',[65n])&0x8000n,0n);
 frame(t,'PeekMessageW',[0x24ffcn,0n,0xffn,0xffn,1n]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(call(t,'GetKeyState',[65n])&0x8000n,0n);equal(peek(t),1n);equal(call(t,'GetKeyState',[65n])&0x8000n,0x8000n);const handle=vm.readInt(lp(t),mode/8);
 equal(peek(t,1n,0x100n,0x109n),0n);equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,handle]),0n);
 vm.queueWindowEvent(host(hwnd,0x20a,0x780000n));equal(peek(t),1n);equal(peek(t,1n,0x200n,0x20en),0n);vm.dispose();
}

// Buffered reads consume only queued packets and leave removed packets available for individual reads.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2,0x30],[6,0x30]]),1n);
 vm.queueWindowEvents([host(hwnd,0x201,1n),host(hwnd,0x202),host(hwnd,0x100,65n,0x1e0001n)]);equal(peek(t),1n);const removed=vm.readInt(lp(t),mode/8);
 vm.write(size,encode(1n,4));equal(call(t,'GetRawInputBuffer',[out,size,header(t)]),0xffffffffn);equal(vm.readInt(size),header(t)+24n);equal(vm.readInt(t.error),122n);equal(peek(t,0n),1n);const buffered=vm.readInt(lp(t),mode/8);
 vm.write(size,encode(128n,4));equal(call(t,'GetRawInputBuffer',[out,size,header(t)]),2n);equal(vm.readInt(size),header(t)*2n+40n);equal(vm.readInt(out),0n);equal(vm.readInt(out+header(t)+24n),1n);equal(call(t,'GetKeyState',[65n])&0x8000n,0x8000n);equal(call(t,'GetKeyState',[1n])&0x8000n,0n);equal(peek(t),0n);equal(call(t,'GetRawInputData',[buffered,0x10000003n,0n,size,header(t)]),0xffffffffn);equal(raw(t,removed).length,Number(header(t)+24n));
 equal(call(t,'GetRawInputBuffer',[0n,size,header(t)]),0n);equal(vm.readInt(size),0n);equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,removed]),0n);vm.dispose();
}

// All outputs, error slots and captured returns are checked before publishing copy-out or consumption.
for(const mode of [32,64])for(const kind of ['registration','data','count','buffer','list','error','return']){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2,0x30]]),1n);vm.queueWindowEvent(host(hwnd));equal(peek(t,0n),1n);const handle=vm.readInt(lp(t),mode/8);vm.write(size,encode(128n,4));vm.write(out,new Uint8Array(64).fill(0xa5));let page=0x25000n,protection=2;
 if(kind==='registration'){registration(t,[[2,0x30]]);vm.write(0x24ff8n,[...encode(1n,2),...encode(6n,2),...encode(0n,4),...encode(hwnd,mode/8)]);frame(t,'RegisterRawInputDevices',[0x24ff8n,1n,stride(t)]);protection=1;}
 if(kind==='data')frame(t,'GetRawInputData',[handle,0x10000003n,0x24ffcn,size,header(t)]);
 if(kind==='count')frame(t,'GetRawInputData',[handle,0x10000003n,out,0x24ffen,header(t)]);
 if(kind==='buffer')frame(t,'GetRawInputBuffer',[0x24ff8n,size,header(t)]);
 if(kind==='list')frame(t,'GetRawInputDeviceList',[0x24ffcn,size,BigInt(mode===32?8:16)]);
 if(kind==='error'){vm.write(size,encode(1n,4));frame(t,'GetRawInputData',[handle,0x10000003n,out,size,header(t)]);page=t.image.teb;}
 if(kind==='return'){frame(t,'GetRawInputData',[handle,0x10000003n,out,size,header(t)]);page=t.sp&~4095n;protection=1;}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t),prefix=data(vm,0x24ff8n,8),output=data(vm,out,64),count=vm.readInt(size);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(data(vm,out,64),output);equal(data(vm,0x24ff8n,8),prefix);equal(vm.readInt(size),count);equal(core.wire_protect(t.handle,page,4096n,4),0);const work=vm.work;equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);equal(peek(t,0n),kind==='buffer'?0n:1n);if(kind!=='buffer')equal(vm.readInt(lp(t),mode/8),handle);vm.dispose();
}

// Full pointer widths and output/return aliases preserve saved frames and exact sizes.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2,0x30,hwnd]]),1n);vm.queueWindowEvent(host(hwnd));equal(peek(t),1n);const h=vm.readInt(lp(t),mode/8);vm.write(size,encode(128n,4));equal(call(t,'GetRawInputData',[h,0x10000003n,t.sp,size,header(t)]),header(t)+24n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0n);
 if(mode===64){const high=0x123400000000n;vm.map(high,8192n,7);equal(registration(t,[[2,0x30,hwnd]],high),1n);vm.write(high+4096n,encode(128n,4));equal(call(t,'GetRawInputData',[h,0x10000003n,high+512n,high+4096n,header(t)]),48n);equal(vm.readInt(high+516n),48n);equal(call(t,'GetRawInputData',[h+(1n<<32n),0x10000003n,0n,size,header(t)]),0xffffffffn);equal(vm.readInt(t.error),6n);equal(registration(t,[[2,0,hwnd&0xffffffffn]]),0n);equal(vm.readInt(t.error),1400n);}
 equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,h]),0n);await invoke(t,'DestroyWindow',[hwnd]);equal(call(t,'GetRegisteredRawInputDevices',[0n,size,stride(t)]),0n);equal(vm.readInt(size),0n);vm.dispose();
}

// Packet quota failure rolls back the entire host batch and does not consume new identities.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[2,0x30,hwnd],[6,0x30,hwnd]]),1n);const batch=Array.from({length:256},()=>host(hwnd));let first,last;
 for(let n=0;n<16;n++){vm.queueWindowEvents(batch);for(let i=0;i<256;i++){equal(peek(t),1n);last=vm.readInt(lp(t),mode/8);first??=last;}}
 const before=snapshot(t);assert.throws(()=>vm.queueWindowEvents([host(hwnd,0x8001),host(hwnd,0x100,65n)]),/capacity/);equal(snapshot(t),before);equal(peek(t,1n,0x8001n),0n);equal(call(t,'GetAsyncKeyState',[65n])&0x8000n,0n);equal(raw(t,first).length,Number(header(t)+24n));
 equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,first]),0n);assert.throws(()=>vm.queueWindowEvents([host(hwnd),host(hwnd)]),/capacity/);vm.queueWindowEvent(host(hwnd));equal(peek(t),1n);equal(vm.readInt(lp(t),mode/8),last+1n);
 await invoke(t,'DestroyWindow',[hwnd]);equal(call(t,'GetRegisteredRawInputDevices',[0n,size,stride(t)]),0n);equal(vm.readInt(size),0n);equal(call(t,'GetRawInputData',[last,0x10000003n,0n,size,header(t)]),0xffffffffn);equal(vm.readInt(t.error),6n);vm.dispose();
}

// Capture changes ordinary delivery; raw registration targets and clipped desktop coordinates stay independent.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t,other=await invoke(t,'CreateWindowExW',[0n,0x20100n,0n,0n,10n,20n,16n,12n,0n,0n,t.image.base,0n]);assert.notEqual(other,0n);
 equal(registration(t,[[2,0,hwnd]]),1n);await invoke(t,'SetCapture',[other]);vm.queueWindowEvent(host(hwnd,0x201,1n,0n,[116,195]));equal(peek(t),1n);equal(vm.readInt(msg,mode/8),hwnd);const handle=vm.readInt(lp(t),mode/8);raw(t,handle);equal(vm.readInt(out+header(t)+12n),7424n);equal(vm.readInt(out+header(t)+16n),16640n);
 equal(call(t,'DefWindowProcW',[other,0xffn,0n,handle]),0n);equal(raw(t,handle).length,Number(header(t)+24n));equal(peek(t,1n,0x201n),1n);equal(vm.readInt(msg,mode/8),other);equal(vm.readInt(lp(t),mode/8)&0xffffffffn,106n|(175n<<16n));equal(call(t,'DefWindowProcW',[hwnd,0xffn,0n,handle]),0n);
 vm.write(devices,[...encode(0n,4),...encode(0n,4),...encode(100n,4),...encode(100n,4)]);equal(call(t,'ClipCursor',[devices]),1n);vm.queueWindowEvent(host(hwnd));equal(peek(t),1n);raw(t,vm.readInt(lp(t),mode/8));equal(vm.readInt(out+header(t)+12n),6336n);equal(vm.readInt(out+header(t)+16n),8448n);vm.dispose();
}

// Message quota is checked for the whole batch before updating either raw identity or key state.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t;equal(registration(t,[[6]]),1n);const batch=Array.from({length:256},()=>host(hwnd,0x8000));for(let n=0;n<15;n++)vm.queueWindowEvents(batch);vm.queueWindowEvents(batch.slice(0,255));
 const before=snapshot(t);assert.throws(()=>vm.queueWindowEvents([host(hwnd,0x8001),host(hwnd,0x100,65n)]),/capacity/);equal(snapshot(t),before);equal(peek(t,1n,0x8001n),0n);equal(call(t,'GetAsyncKeyState',[65n])&0x8000n,0n);equal(peek(t,1n,0x8000n),1n);vm.queueWindowEvent(host(hwnd,0x100,65n));equal(peek(t),1n);equal(vm.readInt(lp(t),mode/8),0x57020000n);vm.dispose();
}

function worker(t,calls){
 const b=[],emit=(...v)=>b.push(...v),imm=(n,w)=>Array.from(encode(n,w));
 for(const[name,args,store,dll='user32.dll']of calls){
  if(t.mode===32){for(const value of args.toReversed())emit(0x68,...imm(value,4));emit(0xb8,...imm(t.vm.bind(dll,name),4),0xff,0xd0);if(store)emit(0xa3,...imm(store,4));}
  else{const frame=Math.ceil((32+Math.max(0,args.length-4)*8+8)/16)*16-8;emit(0x48,0x83,0xec,frame);for(let n=0;n<args.length;n++){if(n<4)emit(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n],...imm(args[n],8));else emit(0x48,0xb8,...imm(args[n],8),0x48,0x89,0x44,0x24,32+(n-4)*8);}emit(0x48,0xb8,...imm(t.vm.bind(dll,name),8),0xff,0xd0,0x48,0x83,0xc4,frame);if(store)emit(0x49,0xbb,...imm(store,8),0x49,0x89,0x03);}
 }
 emit(0xb8,73,0,0,0,...(t.mode===32?[0xc2,4,0]:[0xc3]));t.vm.write(0x30000n,b);return 0x30000n;
}

// Raw registration can route to another real JIT thread; buffer consumption and cleanup follow its owner.
for(const mode of [32,64]){
 const t=await setup(mode),{vm,hwnd}=t,event=call(t,'CreateEventW',[0n,0n,0n,0n],'kernel32.dll');vm.write(0x28100n,encode(128n,4));
 const proc=worker(t,[['CreateWindowExW',[0n,0x20100n,0n,0n,0n,0n,16n,12n,0n,0n,t.image.base,0n],0x28000n],['PostThreadMessageW',[1n,0x8001n,0n,0n]],['WaitForSingleObject',[event,0xffffffffn],null,'kernel32.dll'],['PeekMessageW',[0x29000n,0n,0xffn,0xffn,1n]],['GetRawInputBuffer',[0x2a000n,0x28100n,header(t)],0x28010n],['GetKeyState',[65n],0x28018n],['GetKeyState',[1n],0x28020n],['PostThreadMessageW',[1n,0x8002n,0n,0n]]]);
 const thread=call(t,'CreateThread',[0n,65536n,proc,0n,0n,0n],'kernel32.dll');frame(t,'GetMessageW',[msg,0n,0x8001n,0x8001n]);await finish(t);const target=vm.readInt(0x28000n,mode/8);assert.notEqual(target,0n);equal(registration(t,[[2,0x30,target],[6,0x30,target]]),1n);
 vm.queueWindowEvents([host(hwnd,0x201,1n),host(hwnd,0x202),host(hwnd,0x100,65n,0x1e0001n)]);equal(peek(t),0n);equal(call(t,'GetRawInputBuffer',[0n,size,header(t)]),0n);equal(vm.readInt(size),0n);equal(call(t,'SetEvent',[event],'kernel32.dll'),1n);frame(t,'GetMessageW',[msg,0n,0x8002n,0x8002n]);await finish(t);frame(t,'WaitForSingleObject',[thread,0xffffffffn],'kernel32.dll');await finish(t);
 equal(vm.readInt(0x28010n,mode/8),2n);equal(vm.readInt(0x28018n),0xffff8000n);equal(vm.readInt(0x28020n),0n);equal(call(t,'GetKeyState',[65n])&0x8000n,0n);equal(call(t,'IsWindow',[target]),0n);equal(call(t,'GetRegisteredRawInputDevices',[0n,size,stride(t)]),0n);equal(vm.readInt(size),0n);const held=vm.readInt(0x29000n+BigInt(mode===32?12:24),mode/8);equal(call(t,'GetRawInputData',[held,0x10000003n,0n,size,header(t)]),0xffffffffn);equal(vm.readInt(t.error),6n);equal(call(t,'CloseHandle',[event],'kernel32.dll'),1n);equal(call(t,'CloseHandle',[thread],'kernel32.dll'),1n);vm.dispose();
}

// Every export is admitted by real PE imports; a guest call executes through the JIT service boundary.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.rtext(0x2220,'user32.dll\0');f.rput(0x200c,0x2220);f.rput(0x2010,0x2140);let at=0x2300;
 for(const[n,name]of names.entries()){f.rput(0x2080+n*f.ptr,at,f.ptr);f.rput(0x2140+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 f.rput(0x2080+names.length*f.ptr,0,f.ptr);f.rput(0x2140+names.length*f.ptr,0,f.ptr);const slot=0x2140+2*f.ptr;
 const code=mode===32?[0x68,...encode(8n,4),0x68,...encode(f.preferred+0x3000n,4),0x6a,0,0xff,0x15,...encode(f.preferred+BigInt(slot),4),0xc3]:[0x48,0x83,0xec,0x28,0x31,0xc9,0x48,0x8d,0x15,...encode(BigInt(0x3000-0x100d),4),0x41,0xb8,...encode(16n,4),0xff,0x15,...encode(BigInt(slot-0x1019),4),0x48,0x83,0xc4,0x28,0xc3];
 f.bytes.set(code,f.offset(0x1000));f.relocations.splice(0,f.relocations.length,...(mode===32?[0x1006,0x100e]:[]));f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[n,name]of names.entries())equal(vm.readInt(image.base+0x2140n+BigInt(n*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),0n);equal(vm.readInt(image.base+0x3000n),2n);vm.dispose();
}
console.log(`${checks} raw input/ABI/host/ownership/NOLEGACY/batch/fault checks passed`);
