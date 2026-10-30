import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,gdi,finish,invoke,window,rectangle,readRect,trace,color} from './gdi-harness.mjs';

// Window DCs share real pixels; memory DC bitmap ownership and object outputs keep both ABI widths.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);equal(vm.windowPixels(h),null);const dc=call(t,'GetDC',[h]);assert.ok(dc);equal(gdi(t,'SetPixel',[dc,2n,3n,0x123456n]),0x123456n);equal(gdi(t,'GetPixel',[dc,2n,3n]),0x123456n);equal(color(vm.windowPixels(h),2,3),[0x56,0x34,0x12,255]);equal(call(t,'ReleaseDC',[h,dc]),1n);equal(gdi(t,'GetObjectType',[dc]),0n);const next=call(t,'GetDC',[h]);assert.notEqual(next,dc);equal(gdi(t,'GetPixel',[next,2n,3n]),0x123456n);equal(gdi(t,'GetDeviceCaps',[next,8n]),1280n);equal(gdi(t,'GetDeviceCaps',[next,10n]),720n);equal(gdi(t,'GetPixelFormat',[next]),0n);
 const mem=gdi(t,'CreateCompatibleDC',[0n]),bitmap=gdi(t,'CreateCompatibleBitmap',[next,4n,3n]),stock=gdi(t,'SelectObject',[mem,bitmap]);assert.ok(stock);equal(gdi(t,'GetCurrentObject',[mem,7n]),bitmap);equal(gdi(t,'GetObjectW',[bitmap,0n,0n]),BigInt(mode===32?24:32));equal(gdi(t,'GetObjectW',[bitmap,32n,t.sp]),BigInt(mode===32?24:32));equal(vm.ip,vm.entryPoints.returned);equal(readRect(t,t.sp),[0,4,3,16]);equal(gdi(t,'GetObjectType',[mem]),10n);equal(gdi(t,'GetObjectType',[bitmap]),7n);equal(gdi(t,'SetPixel',[mem,1n,1n,0xabcdefn]),0xabcdefn);equal(gdi(t,'BitBlt',[next,5n,4n,3n,2n,mem,0n,0n,0xcc0020n]),1n);equal(gdi(t,'GetPixel',[next,6n,5n]),0xabcdefn);equal(gdi(t,'DeleteObject',[bitmap]),0n);equal(vm.readInt(t.error),170n);equal(gdi(t,'SelectObject',[mem,stock]),bitmap);equal(gdi(t,'DeleteObject',[bitmap]),1n);equal(gdi(t,'DeleteDC',[mem]),1n);
 const brush=gdi(t,'GetStockObject',[0n]);equal(gdi(t,'GetObjectType',[brush]),2n);equal(gdi(t,'SelectObject',[next,brush]),brush);equal(gdi(t,'GetObjectA',[brush,16n,0x21000n]),BigInt(mode===32?12:16));equal(vm.readInt(0x21004n),0xffffffn);equal(await invoke(t,'MoveWindow',[h,10n,20n,6n,5n,0n]),1n);equal([vm.windowPixels(h).width,vm.windowPixels(h).height],[6,5]);equal(gdi(t,'GetPixel',[next,2n,3n]),0x123456n);const owned=vm.windowPixels(h);owned.rgba.fill(9);equal(color(vm.windowPixels(h),2,3),[0x56,0x34,0x12,255]);equal(call(t,'ReleaseDC',[h,next]),1n);equal(t.time.calls,0);vm.dispose();
}

// Region union/validation, checked PAINTSTRUCT output, erase clipping and EndPaint retain new invalidation.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t,{proc:trace(t)});rectangle(t,0x21000n,[2,3,8,9]);equal(call(t,'InvalidateRect',[h,0x21000n,1n]),1n);rectangle(t,0x21000n,[4,2,10,5]);equal(call(t,'InvalidateRect',[h,0x21000n,0n]),1n);equal(await invoke(t,'GetUpdateRect',[h,0x21100n,0n]),1n);equal(readRect(t,0x21100n),[2,2,10,9]);
 frame(t,'BeginPaint',[h,0x24ff0n]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.windowPixels(h),null);equal(vm.readInt(0x23000n),0n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(await finish(t,1n),vm.readInt(0x24ff0n,mode/8));const dc=vm.register(0);assert.ok(dc);equal(vm.readInt(0x23000n),1n);equal(vm.readInt(0x24ff0n+BigInt(mode/8)),0n);equal(readRect(t,0x24ff0n+BigInt(mode===32?8:12)),[2,2,10,9]);equal(gdi(t,'GetPixel',[dc,2n,2n]),0xffffffn);equal(gdi(t,'GetPixel',[dc,0n,0n]),0xffffffffn);equal(color(vm.windowPixels(h),0,0),[0,0,0,255]);equal(color(vm.windowPixels(h),2,2),[255,255,255,255]);
 equal(call(t,'InvalidateRect',[h,0n,0n]),1n);equal(call(t,'EndPaint',[h,0x24ff0n]),1n);equal(call(t,'EndPaint',[h,0x24ff0n]),0n);equal(vm.readInt(t.error),6n);equal(await invoke(t,'GetUpdateRect',[h,0x21100n,0n]),1n);equal(readRect(t,0x21100n),[0,0,16,12]);equal(call(t,'ValidateRect',[h,0n]),1n);equal(await invoke(t,'GetUpdateRect',[h,0x21100n,0n]),0n);equal(t.time.calls,0);vm.dispose();
}

// A completed erase callback is retained while a later output fault is repaired, including return-slot aliases.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t,{proc:trace(t)});call(t,'InvalidateRect',[h,0n,1n]);frame(t,'BeginPaint',[h,0x21100n]);equal(vm.service().reason,Exit.Dispatch);for(let n=0;vm.ip!==vm.entryPoints.callback;n++){assert.ok(n<1000);equal((await vm.run(1n)).reason,Exit.Budget);}equal(vm.readInt(0x23000n),1n);equal(core.wire_protect(t.handle,0x21000n,4096n,2),0);const before=snapshot(t);equal((await vm.run(100n)).reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x23000n),1n);equal(core.wire_protect(t.handle,0x21000n,4096n,4),0);const dc=await finish(t);equal(vm.readInt(0x23000n),1n);equal(vm.readInt(0x21100n,mode/8),dc);equal(call(t,'EndPaint',[h,0x21100n]),1n);
 call(t,'InvalidateRect',[h,0n,0n]);equal(await invoke(t,'BeginPaint',[h,t.sp]),vm.readInt(t.sp,mode/8));equal(vm.ip,vm.entryPoints.returned);const ps=t.sp;t.sp-=512n;equal(call(t,'EndPaint',[h,ps]),1n);vm.dispose();
}

// Visible dirty windows synthesize paint once; UpdateWindow/default paint validate it without hiding posted paint.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t);await invoke(t,'ShowWindow',[h,1n]);const samples=t.time.calls;equal(call(t,'MsgWaitForMultipleObjectsEx',[0n,0n,0n,1n,0n]),258n);equal(t.time.calls,samples);equal(call(t,'MsgWaitForMultipleObjectsEx',[0n,0n,0n,0x20n,0n]),0n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,0n]),1n);equal(vm.readInt(0x21000n+BigInt(mode===32?4:8)),0xfn);equal(call(t,'GetQueueStatus',[0x20n]),0x200000n);equal(await invoke(t,'UpdateWindow',[h]),1n);equal(await invoke(t,'GetUpdateRect',[h,0n,0n]),0n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),0n);equal(color(vm.windowPixels(h),0,0),[255,255,255,255]);
 equal(call(t,'PostMessageW',[h,0xfn,0n,0n]),1n);equal(call(t,'InvalidateRect',[h,0n,1n]),1n);const dc=await invoke(t,'BeginPaint',[h,0x21100n]);assert.ok(dc);equal(call(t,'EndPaint',[h,0x21100n]),1n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),1n);equal(await invoke(t,'DispatchMessageW',[0x21000n]),0n);equal(call(t,'PeekMessageW',[0x21000n,0n,0n,0n,1n]),0n);equal(await invoke(t,'DestroyWindow',[h]),1n);equal(vm.windowPixels(h),null);vm.dispose();
}
// A checked origin output commits before the new origin; overlap blits snapshot their source.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),dc=call(t,'GetDC',[h]);for(let x=0;x<4;x++)gdi(t,'SetPixel',[dc,BigInt(x),0n,BigInt(x+1)]);equal(gdi(t,'BitBlt',[dc,1n,0n,3n,1n,dc,0n,0n,0xcc0020n]),1n);equal([0n,1n,2n,3n].map(x=>gdi(t,'GetPixel',[dc,x,0n])),[1n,1n,2n,3n]);
 frame(t,'SetWindowOrgEx',[dc,5n,6n,0x24ffcn],'gdi32.dll');vm.write(0x24ffcn,new Uint8Array(8).fill(0xcc));equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffcn),0xccccccccn);equal(gdi(t,'GetWindowOrgEx',[dc,0x21000n]),1n);equal([vm.readInt(0x21000n),vm.readInt(0x21004n)],[0n,0n]);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(gdi(t,'SetWindowOrgEx',[dc,5n,6n,0x24ffcn]),1n);equal(gdi(t,'GetPixel',[dc,5n,6n]),1n);equal(gdi(t,'SetPixelV',[dc,5n,6n,0x112233n]),1n);equal(color(vm.windowPixels(h),0,0),[0x33,0x22,0x11,255]);vm.dispose();
}

// A rejected resize keeps geometry/pixels and can retry a fault in its LastError output.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),dc=call(t,'GetDC',[h]);gdi(t,'SetPixel',[dc,0n,0n,0x123456n]);frame(t,'MoveWindow',[h,1n,2n,8192n,8192n,1n]);const page=t.error&~4095n;equal(core.wire_protect(t.handle,page,4096n,2),0);equal((await vm.run(1000n)).reason,Exit.Fault);equal([vm.windowPixels(h).width,vm.windowPixels(h).height],[16,12]);equal(color(vm.windowPixels(h),0,0),[0x56,0x34,0x12,255]);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(await finish(t),0n);equal(vm.readInt(t.error),8n);equal(call(t,'GetWindowRect',[h,0x21000n]),1n);equal(readRect(t,0x21000n),[10,20,26,32]);equal(call(t,'ReleaseDC',[h,dc]),1n);vm.dispose();
}

// Window/DC identity checks precede pointer access, and resource capacity rejects before consuming a handle.
{
 const t=await machine(64),{vm}=t;equal(call(t,'BeginPaint',[0xbadn,0xdead0000n]),0n);equal(vm.readInt(t.error),1400n);equal(gdi(t,'GetTextColor',[0xbadn]),0xffffffffn);equal(vm.readInt(t.error),6n);assert.throws(()=>vm.bind('gdi32.dll','CreateFontW'),/migrated|unknown/);assert.throws(()=>vm.bind('kernel32.dll','BeginPaint'),/migrated|unknown/);
 let first,last;for(let n=0;n<4095;n++){const h=gdi(t,'CreateCompatibleDC',[0n]);if(n===0)first=h;last=h;assert.ok(h);}equal(gdi(t,'CreateCompatibleDC',[0n]),0n);equal(vm.readInt(t.error),8n);equal(gdi(t,'DeleteDC',[first]),1n);equal(gdi(t,'CreateCompatibleDC',[0n]),last+16n);vm.dispose();
}

// Unwinding out of an erase callback releases its unfinished paint DC while the window remains alive.
{
 const t=await machine(64),{vm}=t,h=await window(t),target=0x331f0n,value=0xfedcba9876543210n;
 const api=(name,args)=>{const b=[];for(let n=0;n<args.length;n++){if(n<4)b.push(...[[0x48,0xb9],[0x48,0xba],[0x49,0xb8],[0x49,0xb9]][n],...encode(args[n],8));else b.push(0x48,0xb8,...encode(args[n],8),0x48,0x89,0x44,0x24,32+(n-4)*8);}b.push(0x48,0xb8,...encode(vm.bind(name==='BeginPaint'?'user32.dll':'kernel32.dll',name),8),0xff,0xd0);return b;};
 vm.write(0x33000n,[0x48,0x83,0xec,56,...api('BeginPaint',[h,0x21100n]),0xcc]);vm.write(target,[0x48,0x83,0xc4,56,0xc3]);vm.write(0x34000n,[0x49,0xbb,...encode(0x23300n,8),0x4d,0x89,0x03,0x48,0x83,0xec,56,...api('RtlUnwindEx',[t.sp-56n,target,0n,value,0x26000n,0n]),0xcc]);vm.write(0x27000n,[...encode(0x33000n,4),...encode(0x33200n,4),...encode(0x28000n,4),...encode(0x34000n,4),...encode(0x34100n,4),...encode(0x28100n,4)]);vm.write(0x28000n,[1,4,1,0,4,0x62,0,0]);vm.write(0x28100n,[1,17,1,0,17,0x62,0,0]);equal(call(t,'RtlAddFunctionTable',[0x27000n,2n,0n],'kernel32.dll'),1n);call(t,'SetWindowLongPtrW',[h,0xfffffffcn,0x34000n]);call(t,'InvalidateRect',[h,0n,1n]);vm.ip=0x33000n;vm.register(4,t.sp);vm.write(t.sp,encode(vm.entryPoints.returned,8));equal(await finish(t,1n),value);const dc=vm.readInt(0x23300n,8);assert.ok(dc);equal(gdi(t,'GetObjectType',[dc]),0n);equal(call(t,'IsWindow',[h]),1n);vm.dispose();
}

console.log(`${checks} paint regions/DC/bitmap/pixel/callback/queue checks passed`);
