import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';

const dialogs=[['DialogBoxParamA',5],['DialogBoxParamW',5],['DialogBoxIndirectParamA',5],['CreateDialogParamA',5],['CreateDialogParamW',5],['CreateDialogIndirectParamA',5],['EndDialog',2]];
const clipboard=[['OpenClipboard',1],['CloseClipboard',0],['EmptyClipboard',0],['GetClipboardData',1],['SetClipboardData',2],['IsClipboardFormatAvailable',1]];
const extra=[['GetUserObjectInformationA',5],['GetThreadDesktop',1],['GetCaretBlinkTime',0],['AllowSetForegroundWindow',1]];
const added=[...dialogs,...clipboard,...extra,['SendMessageTimeoutA',7],['SendMessageTimeoutW',7]];
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));

// Unavailable owners never receive a synthetic handle, a transferred allocation or a callback.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),out=mode===32?0x24000n:0x123400000000n,width=mode/8;
 if(mode===64)vm.map(out,4096n,3);vm.write(out,new Uint8Array(64).fill(0xa5));vm.flags=0x8d5n;
 for(const[name,arity]of added){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','gdi32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 for(const[name,arity]of dialogs){
  vm.write(t.error,encode(77n,4));equal(call(t,name,Array(arity).fill(0x60000n)),name.startsWith('DialogBox')?(1n<<BigInt(mode))-1n:0n);
  equal(vm.readInt(t.error),120n);equal(vm.register(4),t.sp+BigInt(mode===32?(arity+1)*4:8));
 }
 for(const hwnd of [0n,h]){equal(call(t,'OpenClipboard',[hwnd]),0n);equal(vm.readInt(t.error),50n);}
 equal(call(t,'OpenClipboard',[0x60000n]),0n);equal(vm.readInt(t.error),1400n);
 for(const[name,arity]of clipboard.slice(1,-1)){
  equal(call(t,name,arity===2?[0xc001n,out]:[0xc001n]),0n);equal(vm.readInt(t.error),1418n);
  equal(bytes(vm,out,64),new Array(64).fill(0xa5));
 }
 vm.write(0x22000n,Buffer.from('Registered\0','utf16le'));const format=call(t,'RegisterClipboardFormatW',[0x22000n]);assert.ok(format>=0xc000n);
 vm.write(t.error,encode(77n,4));for(const id of [0n,1n,13n,format,0xffffffffn])equal(call(t,'IsClipboardFormatAvailable',[id]),0n);equal(vm.readInt(t.error),77n);
 for(const name of ['GetProcessWindowStation','GetUserObjectInformationA','GetUserObjectInformationW','GetThreadDesktop']){
  equal(call(t,name,[0x60000n,0xffffffffn,out,64n,out+32n]),0n);equal(vm.readInt(t.error),120n);equal(bytes(vm,out,64),new Array(64).fill(0xa5));
 }
 vm.write(t.error,encode(77n,4));equal(call(t,'GetCaretBlinkTime'),530n);equal(vm.readInt(t.error),77n);
 const foreground=call(t,'GetForegroundWindow');equal(call(t,'AllowSetForegroundWindow',[0xffffffffn]),0n);equal(vm.readInt(t.error),50n);equal(call(t,'GetForegroundWindow'),foreground);
 equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);equal(vm.readInt(out,width),(1n<<BigInt(mode))-1n&0xa5a5a5a5a5a5a5a5n);vm.dispose();
}

// Error-slot and return faults preserve the entire CPU and can be repaired in place.
for(const mode of [32,64])for(const name of ['DialogBoxParamW','GetUserObjectInformationA','SetClipboardData','AllowSetForegroundWindow'])for(const kind of ['error','return']){
 const t=await machine(mode),{vm}=t;vm.write(t.error,encode(77n,4));frame(t,name,Array(5).fill(0x60000n));
 const before=snapshot(t),work=vm.work,page=(kind==='error'?t.error:t.sp)&~4095n;
 equal(core.wire_protect(t.handle,page,4096n,kind==='error'?2:1),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.readInt(t.error),77n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.work,work+1n);vm.dispose();
}

// Static imports cover all added declarations and actual seven-argument guest calls read the result slot from the stack.
for(const mode of [32,64])for(let offset=0;offset<added.length;offset+=16){
 const names=added.slice(offset,offset+16).map(x=>x[0]),f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));
 f.put(f.dirs+12,40);f.rput(0x2000,0x2040);f.rput(0x200c,0x2028);f.rput(0x2010,0x2100);f.rtext(0x2028,'user32.dll\0');let at=0x21c0;
 for(const[i,name]of names.entries()){f.rput(0x2040+i*f.ptr,at,f.ptr);f.rput(0x2100+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 assert.ok(at<0x2600);f.relocations.length=0;const b=[],index=names.indexOf('SendMessageTimeoutA');
 if(index<0)b.push(0xc3);
 else{
  f.rput(0x3100,0xa5a5a5a5a5a5a5a5n,8);const iat=0x2100+index*f.ptr;
  if(mode===32){b.push(0x68,...encode(f.preferred+0x3100n,4));for(let n=0;n<6;n++)b.push(0x6a,0);b.push(0xff,0x15,...encode(f.preferred+BigInt(iat),4),0xc3);f.relocations.push(0x1001,0x1013);}
  else{b.push(0x48,0x83,0xec,56,0x31,0xc9,0x31,0xd2,0x45,0x31,0xc0,0x45,0x31,0xc9,0x48,0xc7,0x44,0x24,32,0,0,0,0,0x48,0xc7,0x44,0x24,40,0,0,0,0,0x48,0x8d,0x05);b.push(...encode(0x3100n-0x1000n-BigInt(b.length+4),4),0x48,0x89,0x44,0x24,48,0xff,0x15);b.push(...encode(BigInt(iat-0x1000-b.length-4),4),0x48,0x83,0xc4,56,0xc3);}
 }
 f.bytes.set(b,f.offset(0x1000));f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);
 for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*f.ptr),f.ptr),vm.bind('user32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);
 if(index>=0){equal(vm.register(0),0n);equal(vm.readInt(image.base+0x3100n,8),0xa5a5a5a5a5a5a5a5n);}
 vm.dispose();
}
console.log(`${checks} USER missing-backend/dialog/clipboard/desktop/ABI/import/JIT/fault checks passed`);
