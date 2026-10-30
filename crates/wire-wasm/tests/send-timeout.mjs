import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,invoke,finish,window} from './gdi-harness.mjs';

const names=['SendMessageTimeoutA','SendMessageTimeoutW'];
const data=0x23200n;
function service(t,name,args=[],dll='user32.dll'){
 const b=[];
 if(t.mode===32){for(const a of args.toReversed())b.push(0x68,...encode(a,4));b.push(0xb8,...encode(t.vm.bind(dll,name),4),0xff,0xd0);}
 else{
  b.push(0x48,0x83,0xec,104);
  for(let n=0;n<args.length;n++)if(n<4)b.push(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n],...encode(args[n],8));else b.push(0x48,0xb8,...encode(args[n],8),0x48,0x89,0x44,0x24,32+(n-4)*8);
  b.push(0x48,0xb8,...encode(t.vm.bind(dll,name),8),0xff,0xd0,0x48,0x83,0xc4,104);
 }
 return b;
}
function procedure(t,value,{yielding=false}={}){
 const {vm,mode}=t,b=[];
 if(mode===32){
  b.push(0xff,0x05,...encode(data,4));
  for(let n=0;n<4;n++)b.push(0x8b,0x44,0x24,4+n*4,0xa3,...encode(data+16n+BigInt(n*4),4));
 }else b.push(0x49,0xbb,...encode(data,8),0x41,0xff,0x03,0x49,0x89,0x4b,16,0x49,0x89,0x53,24,0x4d,0x89,0x43,32,0x4d,0x89,0x4b,40);
 if(yielding)b.push(...service(t,'Sleep',[0n],'kernel32.dll'));
 if(mode===64)b.push(0x48);b.push(0xb8,...encode(value,mode/8),...(mode===32?[0xc2,16,0]:[0xc3]));
 vm.write(0x30000n,b);return 0x30000n;
}
const subclass=(t,h,p)=>call(t,t.mode===32?'SetWindowLongW':'SetWindowLongPtrW',[h,0xfffffffcn,p]);

// Same-queue delivery ignores the timeout, preserves complete message words and separates status from LRESULT.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),width=mode/8,out=mode===32?0x24000n:0x123400000000n;
 if(mode===64)vm.map(out,4096n,3);
 const wp=mode===32?0x87654321n:0x1234567887654321n,lp=mode===32?0xfedcba98n:0xfedcba9876543210n;
 vm.flags=0x8d5n;vm.write(t.error,encode(77n,4));
 for(const name of names)for(const value of [0n,lp]){
  subclass(t,h,procedure(t,value,{yielding:true}));
  for(const flags of [0n,1n,2n,8n,32n,43n]){
   const count=vm.readInt(data);vm.write(out,new Uint8Array(16).fill(0xa5));
   equal(await invoke(t,name,[h,0x8001n,wp,lp,flags,0n,out],1n),1n);
   equal(vm.readInt(out,width),value);equal(vm.readInt(out+BigInt(width),1),0xa5n);
   equal(vm.readInt(data),count+1n);equal([0n,1n,2n,3n].map(n=>vm.readInt(data+16n+n*BigInt(width),width)),[h,0x8001n,wp,lp]);
  }
  equal(await invoke(t,name,[h,0x8001n,wp,lp,0n,0xffffffffn,0n],1n),1n);
  equal(await invoke(t,name,[h,0x8001n,wp,lp,0n,1n,t.sp],1n),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,width),value);
 }
 equal(vm.readInt(t.error),77n);equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Failed qualification must not invoke WNDPROC or inspect a result pointer.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);subclass(t,h,procedure(t,73n));
 for(const name of names){
  for(const[hwnd,message,flags,error]of [[0n,0x8001n,0n,1400n],[0xffffn,0x8001n,0n,50n],[h,0x8001n,4n,87n],[h,0x8001n,0x100000000n|16n,87n]]){
   equal(await invoke(t,name,[hwnd,message,0n,0n,flags,1n,0x60000n]),0n);equal(vm.readInt(t.error),error);
  }
 }
 equal(await invoke(t,'SendMessageTimeoutA',[h,0xcn,0n,0x60000n,0n,1n,0x60000n]),0n);equal(vm.readInt(t.error),50n);equal(vm.readInt(data),0n);equal(t.time.calls,0);vm.dispose();
}

// An invalid initial result span faults before callback entry; a later fault freezes the completed result.
for(const mode of [32,64])for(const name of names){
 const t=await machine(mode),{vm}=t,h=await window(t),out=0x24ffen,value=mode===32?0x87654321n:0xfedcba9876543210n;
 subclass(t,h,procedure(t,value));vm.write(out,new Uint8Array(16).fill(0xa5));
 frame(t,name,[h,0x8001n,0n,0n,0n,0n,out]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);
 let before=snapshot(t),work=vm.work;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(data),0n);equal(vm.readInt(out,2),0xa5a5n);
 equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<100);equal((await vm.run(1n)).reason,Exit.Budget);}
 equal(vm.readInt(data),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);work=vm.work;
 equal((await vm.run(100n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(out,2),0xa5a5n);
 vm.register(0,0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(await finish(t,1n),1n);equal(vm.readInt(out,mode/8),value);equal(vm.readInt(data),1n);vm.dispose();
}

// ERRORONEXIT converts destruction during WM_CLOSE to failure; the error write can be repaired without replay.
for(const mode of [32,64])for(const name of names)for(const flags of [0n,32n]){
 const t=await machine(mode),{vm}=t,h=await window(t),out=0x24000n;
 vm.write(out,new Uint8Array(8).fill(0xa5));vm.write(t.error,encode(77n,4));frame(t,name,[h,0x10n,0n,0n,flags,0n,out]);
 if(flags){equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal(vm.readInt(out,8),0xa5a5a5a5a5a5a5a5n);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);}
 equal(await finish(t,1n),flags?0n:1n);equal(call(t,'IsWindow',[h]),0n);equal(vm.readInt(t.error),flags?1400n:77n);equal(vm.readInt(out,mode/8),flags?(1n<<BigInt(mode))-1n&0xa5a5a5a5a5a5a5a5n:0n);vm.dispose();
}

// Cross-thread delivery is rejected rather than falling back to an unbounded synchronous send.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),atom=mode===32?call(t,'GetClassLongW',[h,0xffffffe0n]):0x20100n;
 const kernel=(name,args=[])=>call(t,name,args,'kernel32.dll'),ready=kernel('CreateEventW',[0n,1n,0n,0n]),release=kernel('CreateEventW',[0n,1n,0n,0n]);
 const store=mode===32?[0xa3,...encode(0x23520n,4)]:[0x48,0xa3,...encode(0x23520n,8)];
 vm.write(0x31000n,[...service(t,'CreateWindowExW',[0n,atom,0n,0n,10n,20n,100n,80n,0n,0n,t.image.base,0n]),...store,...service(t,'SetEvent',[ready],'kernel32.dll'),...service(t,'WaitForSingleObject',[release,0xffffffffn],'kernel32.dll'),0x31,0xc0,...(mode===32?[0xc2,4,0]:[0xc3])]);
 const thread=kernel('CreateThread',[0n,65536n,0x31000n,0n,0n,0x23500n]);assert.ok(thread);frame(t,'WaitForSingleObject',[ready,0xffffffffn],'kernel32.dll');equal(await finish(t,1n),0n);const foreign=vm.readInt(0x23520n,mode/8);assert.ok(foreign);
 for(const name of names){vm.write(0x24000n,new Uint8Array(8).fill(0xa5));equal(await invoke(t,name,[foreign,0x8001n,0n,0n,2n,1n,0x24000n]),0n);equal(vm.readInt(t.error),50n);equal(vm.readInt(0x24000n,8),0xa5a5a5a5a5a5a5a5n);}
 equal(kernel('SetEvent',[release]),1n);frame(t,'WaitForSingleObject',[thread,0xffffffffn],'kernel32.dll');equal(await finish(t,1n),0n);equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} send-timeout same-queue/result/flags/yield/destruction/thread/ABI/fault checks passed`);
