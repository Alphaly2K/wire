import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,gdi,invoke,finish,window,rectangle,readRect} from './gdi-harness.mjs';
const names=['EnableWindow','SetParent','DefDlgProcA','DefDlgProcW'];
const create=(t,atom,{parent=0n,id=0n,style=0n,x=10n,y=20n,width=100n,height=80n}={})=>invoke(t,'CreateWindowExW',[0n,atom,0n,style,x,y,width,height,parent,id,t.image.base,0n]);
const rect=(t,h)=>{equal(call(t,'GetWindowRect',[h,0x24000n]),1n);return readRect(t,0x24000n);};
const origin=(t,h)=>{rectangle(t,0x24000n,[0,0,0,0]);equal(call(t,'ClientToScreen',[h,0x24000n]),1n);return readRect(t,0x24000n).slice(0,2);};
for(const mode of [32,64]){const t=await machine(mode);for(const name of names){assert.ok(t.vm.bind('user32.dll',name));assert.throws(()=>t.vm.bind('gdi32.dll',name),/migrated|unknown/);}t.vm.dispose();}

// Real child windows keep parent-relative geometry and derive absolute coordinates from the current ancestry.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,base=await window(t),atom=mode===32?call(t,'GetClassLongW',[base,0xffffffe0n]):0x20100n;
 const parent=await create(t,atom,{style:0xc00000n,x:100n,y:200n,width:400n,height:300n}),id=mode===32?call(t,'CreateMenu'):0x12340000002an;
 const child=await create(t,atom,{parent,id,style:0x40800000n}),nested=await create(t,atom,{parent:child,id:7n,style:0x40000000n,x:3n,y:4n});assert.ok(child);assert.ok(nested);
 equal(call(t,'GetParent',[child]),parent);equal(call(t,'GetDlgItem',[parent,id]),child);equal(call(t,'GetDlgCtrlID',[child]),BigInt.asUintN(32,id));if(mode===32)equal(call(t,'GetMenu',[child]),0n);equal(rect(t,child),[113,246,213,326]);equal(origin(t,child),[114,247]);equal(origin(t,nested),[117,251]);
 equal(await invoke(t,'MoveWindow',[parent,101n,202n,400n,300n,0n]),1n);equal(rect(t,child),[114,248,214,328]);equal(origin(t,nested),[118,253]);equal(vm.windows().map(w=>w.handle),[base,parent]);
 const other=await create(t,atom,{x:400n,y:500n}),before=rect(t,child),point=origin(t,child);equal(call(t,'SetParent',[child,other]),parent);equal(rect(t,child),before);equal(origin(t,child),point);equal(call(t,'GetParent',[child]),other);equal(call(t,'GetDlgItem',[parent,id]),0n);equal(call(t,'GetDlgItem',[other,id]),child);
 equal(call(t,'SetParent',[other,nested]),0n);equal(vm.readInt(t.error),87n);equal(call(t,'SetParent',[child,0xbadn]),0n);equal(vm.readInt(t.error),1400n);equal(call(t,'GetParent',[child]),other);
 equal(call(t,'SetParent',[child,0n]),other);equal(rect(t,child),before);equal(call(t,'SetParent',[child,parent]),0n);equal(rect(t,child),before);
 vm.queueWindowEvent({window:child,message:0x200,lparam:2n|(3n<<16n),time:55});equal(call(t,'PeekMessageW',[0x21000n,0n,0x200n,0x200n,1n]),1n);equal([vm.readInt(mode===32?0x21014n:0x21024n),vm.readInt(mode===32?0x21018n:0x21028n)],[BigInt(point[0]+2),BigInt(point[1]+3)]);
 await invoke(t,'SetCapture',[nested]);vm.queueWindowEvent({window:child,message:0x201,lparam:2n|(3n<<16n),time:55});equal(call(t,'PeekMessageW',[0x21000n,0n,0x201n,0x201n,1n]),1n);equal(vm.readInt(0x21000n,mode/8),nested);equal(vm.readInt(mode===32?0x2100cn:0x21018n),0xffffn|(0xffffn<<16n));
 const dc=call(t,'GetDC',[nested]);gdi(t,'SetPixel',[dc,0n,0n,0x123456n]);equal(await invoke(t,'DestroyWindow',[parent]),1n);for(const h of [parent,child,nested])equal(call(t,'IsWindow',[h]),0n);equal(gdi(t,'GetObjectType',[dc]),0n);if(mode===32)equal(call(t,'IsMenu',[id]),1n);equal(call(t,'GetCapture'),0n);equal(t.time.calls,0);vm.dispose();
}

// Owner and message-only windows retain their original relationship semantics and top-level filtering.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,base=await window(t),atom=mode===32?call(t,'GetClassLongW',[base,0xffffffe0n]):0x20100n,owned=await create(t,atom,{parent:base}),hidden=await create(t,atom,{parent:mode===32?0xfffffffdn:0xfffffffffffffffdn});equal(call(t,'GetParent',[owned]),base);equal(rect(t,owned),[10,20,110,100]);equal(origin(t,owned),[10,20]);equal(vm.windows().map(w=>w.handle),[base,owned]);
 equal(await create(t,atom,{parent:0xbadn,style:0x40000000n,id:123n}),0n);equal(vm.readInt(t.error),1400n);equal(call(t,'IsWindow',[hidden]),1n);
 const c=await create(t,atom,{style:0x40000000n,id:0xbadn});assert.ok(c);equal(call(t,'GetDlgCtrlID',[c]),0xbadn);equal(origin(t,c),[10,20]);vm.dispose();
}

// EnableWindow commits before WM_ENABLE, preserves the previous disabled BOOL and avoids duplicate notifications.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),def=vm.bind('user32.dll','DefWindowProcW');
 vm.write(0x30000n,mode===32?[0xff,0x05,...encode(0x23000n,4),0x8b,0x44,0x24,12,0xa3,...encode(0x23004n,4),0xb8,...encode(def,4),0xff,0xe0]:[0x49,0xbb,...encode(0x23000n,8),0x41,0xff,0x03,0x45,0x89,0x43,4,0x48,0xb8,...encode(def,8),0xff,0xe0]);call(t,mode===32?'SetWindowLongW':'SetWindowLongPtrW',[h,0xfffffffcn,0x30000n]);vm.write(t.error,encode(77n,4));equal(await invoke(t,'EnableWindow',[h,0n],1n),0n);equal(call(t,'IsWindowEnabled',[h]),0n);equal(vm.readInt(0x23000n),1n);equal(vm.readInt(0x23004n),0n);equal(await invoke(t,'EnableWindow',[h,0n]),1n);equal(vm.readInt(0x23000n),1n);equal(await invoke(t,'EnableWindow',[h,0x100n]),1n);equal(vm.readInt(0x23000n),2n);equal(vm.readInt(0x23004n),1n);equal(vm.readInt(t.error),77n);
 t.sp=(t.image.sp&~4095n)+16n;frame(t,'EnableWindow',[h,0n]);equal(core.wire_protect(t.handle,(t.sp&~4095n)-4096n,4096n,0x104),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(call(t,'IsWindowEnabled',[h]),1n);equal(await invoke(t,'EnableWindow',[h,0n],1n),0n);equal(vm.readInt(0x23000n),3n);equal(t.time.calls,0);vm.dispose();
}

// Default dialog exports share the existing default text/close subset and saved return frame.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,h=await window(t),name=wide?'DefDlgProcW':'DefDlgProcA';vm.write(0x24000n,Buffer.from('dialog\0',wide?'utf16le':'ascii'));equal(call(t,name,[h,0xcn,0n,0x24000n]),1n);equal(call(t,name,[h,0xdn,32n,0x24100n]),6n);equal(call(t,name,[h,0x81n,0n,0n]),1n);equal(await invoke(t,name,[h,0x10n,0n,0n]),0n);equal(call(t,'IsWindow',[h]),0n);vm.dispose();
}
console.log(`${checks} child/parent/geometry/input/enable/default-dialog/lifetime checks passed`);
