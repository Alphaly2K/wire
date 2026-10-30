import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window,rectangle,readRect,invoke} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';
const names=['ShowCursor','GetCursor','SetCursor','GetCursorPos','SetCursorPos','ClipCursor','GetClipCursor','EnableMouseInPointer','IsMouseInPointerEnabled','GetPointerType','GetPointerInfo','GetPointerDevices','TrackMouseEvent'];
const point=(t,p=0x24000n)=>[Number(BigInt.asIntN(32,t.vm.readInt(p))),Number(BigInt.asIntN(32,t.vm.readInt(p+4n)))];
const peek=t=>call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]);
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x24000n,rect=0x23000n;
 for(const name of names){assert.ok(vm.bind('user32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));const beforeFp=fp(t);vm.write(t.error,encode(123n,4));
 equal(call(t,'GetCursor'),0n);equal(call(t,'SetCursor',[0n]),0n);equal(call(t,'ShowCursor',[0n]),0xffffffffn);equal(call(t,'ShowCursor',[0x100000001n]),0n);
 equal(call(t,'SetCursorPos',[-7n,123n]),1n);equal(call(t,'GetCursorPos',[out]),1n);equal(point(t),[-7,123]);
 equal(call(t,'GetClipCursor',[out]),1n);equal(readRect(t,out),[0,0,1024,768]);rectangle(t,rect,[-10,20,90,120]);equal(call(t,'ClipCursor',[rect]),1n);equal(call(t,'GetCursorPos',[out]),1n);equal(point(t),[0,119]);
 equal(call(t,'SetCursorPos',[1000n,1n]),1n);equal(call(t,'GetCursorPos',[out]),1n);equal(point(t),[89,20]);equal(call(t,'GetClipCursor',[out]),1n);equal(readRect(t,out),[0,20,90,120]);
 rectangle(t,rect,[0,0,0,1]);equal(call(t,'ClipCursor',[rect]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'GetClipCursor',[out]),1n);equal(readRect(t,out),[0,20,90,120]);
 equal(call(t,'ClipCursor',[0n]),1n);equal(call(t,'SetCursorPos',[-7n,123n]),1n);equal(call(t,'GetCursorPos',[t.sp]),1n);equal(point(t,t.sp),[-7,123]);equal(vm.ip,vm.entryPoints.returned);
 const other=await machine(mode);equal(call(other,'GetCursorPos',[out]),1n);equal(point(other),[0,0]);other.vm.dispose();
 const hwnd=await window(t);rectangle(t,rect,[10,20,30,40]);equal(call(t,'ClipCursor',[rect]),1n);vm.queueWindowEvent({window:hwnd,message:0x200,wparam:0n,lparam:0n,point:[500,0],time:55});equal(peek(t),1n);equal(vm.readInt(0x21000n+BigInt(mode===32?12:24),mode/8),19n);equal(call(t,'GetCursorPos',[out]),1n);equal(point(t),[29,20]);equal(call(t,'ClipCursor',[0n]),1n);equal(call(t,'IsMouseInPointerEnabled'),0n);equal(call(t,'GetPointerType',[1n,out]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'EnableMouseInPointer',[1n]),1n);equal(call(t,'IsMouseInPointerEnabled'),1n);equal(call(t,'EnableMouseInPointer',[1n]),0n);equal(vm.readInt(t.error),5n);
 vm.write(out,encode(123n,4));equal(call(t,'GetPointerDevices',[out,1n]),1n);equal(vm.readInt(out),0n);
 vm.queueWindowEvent({window:hwnd,message:0x201,wparam:1n,lparam:0n,point:[23,47],time:57});equal(call(t,'GetQueueStatus',[0x1000n]),0x10001000n);equal(peek(t),1n);equal(vm.readInt(0x21000n+BigInt(mode/8)),0x246n);equal(call(t,'GetPointerType',[0x100000001n,out]),1n);equal(vm.readInt(out),4n);
 vm.write(out,new Uint8Array(112).fill(0xa5));equal(call(t,'GetPointerInfo',[1n,out]),1n);equal(vm.readInt(out),4n);equal(vm.readInt(out+4n),1n);equal(vm.readInt(out+BigInt(mode===32?20:24),mode/8),hwnd);equal(point(t,out+BigInt(mode===32?24:32)),[23,47]);equal(vm.readInt(out+BigInt(mode===32?56:64)),57n);equal(vm.readInt(out+BigInt(mode===32?80:88)),1n);equal(vm.readInt(out+BigInt(mode===32?88:96),1),0xa5n);
 equal(call(t,'GetKeyState',[1n],'user32.dll')&0x8000n,0x8000n);equal(call(t,'GetCursorPos',[out]),1n);equal(point(t),[23,47]);
 const track=0x23000n,size=mode===32?16:24;vm.write(track,new Uint8Array(size));vm.write(track,encode(BigInt(size),4));vm.write(track+4n,encode(0x40000000n,4));equal(call(t,'TrackMouseEvent',[track]),1n);equal(vm.readInt(track),BigInt(size));equal(vm.readInt(track+4n),0n);
 vm.write(track+8n,encode(hwnd,mode/8));vm.write(track+4n,encode(1n,4));equal(call(t,'TrackMouseEvent',[track]),0n);equal(vm.readInt(t.error),50n);vm.write(track+4n,encode(0x80000001n,4));equal(call(t,'TrackMouseEvent',[track]),1n);
 vm.write(t.error,encode(123n,4));equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(fp(t),beforeFp);equal(t.time.calls,0);equal(t.time.wallCalls,0);
 await invoke(t,'DestroyWindow',[hwnd]);equal(call(t,'GetPointerInfo',[1n,out]),0n);equal(vm.readInt(t.error),87n);vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,out=0x24ffen;vm.write(out,new Uint8Array(24).fill(0xa5));rectangle(t,0x23000n,[10,20,30,40]);equal(call(t,'ClipCursor',[0x23000n]),1n);
 for(const name of ['GetCursorPos','GetClipCursor']){frame(t,name,[out]);const before=snapshot(t);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);vm.write(out,new Uint8Array(24).fill(0xa5));}
 rectangle(t,out,[50,60,70,80]);frame(t,'ClipCursor',[out]);const before=snapshot(t);equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(call(t,'GetClipCursor',[0x23000n]),1n);equal(readRect(t,0x23000n),[10,20,30,40]);vm.dispose();
}
console.log(`${checks} cursor/clip/pointer-promotion/ABI/lifetime/fault checks passed`);
