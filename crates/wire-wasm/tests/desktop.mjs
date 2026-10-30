import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window,readRect} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

// The adapter must preserve process isolation, saved frames and output transactions.
for(const mode of [32,64]){
 const t=await machine(mode),other=await machine(mode),{vm}=t,p=0x22000n;
 vm.flags=0x8d5n;vm.xmm(3,new Uint8Array(16).fill(0x7b));vm.mxcsr=0x5f81;const fpu=fp(t);
 vm.write(t.error,encode(123n,4));
 equal(call(t,'GetDoubleClickTime'),500n);equal(call(t,'GetDialogBaseUnits'),0x100008n);
 for(const name of ['SystemParametersInfoA','SystemParametersInfoW']){
  vm.write(p,new Uint8Array(8).fill(0xa5));
  equal(call(t,name,[0x46n,0n,p,0n]),1n);equal(vm.readInt(p,8),0xa5a5a5a500000000n);
  equal(call(t,name,[0x47n,0x100000001n,0xdeadn,0n]),1n);
  equal(call(t,name,[0x46n,0n,p,0n]),1n);equal(vm.readInt(p),1n);
  equal(call(other,name,[0x46n,0n,p,0n]),1n);equal(other.vm.readInt(p),0n);
  for(const [action,flags] of [[0x47n,1n],[0xffffn,0n]]){
   equal(call(t,name,[action,0n,0xdeadn,flags]),0n);equal(vm.readInt(t.error),50n);
   equal(call(t,name,[0x46n,0n,p,0n]),1n);equal(vm.readInt(p),1n);
  }
  vm.write(t.error,encode(123n,4));
  equal(call(t,name,[0x47n,0x100000000n,0n,0x100000000n]),1n);
  equal(call(t,name,[0x46n,0n,t.sp,0n]),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0n);
  frame(t,name,[0x46n,0n,0x24ffen,0n]);vm.write(0x24ffen,[0x7b,0x6c]);
  equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);
  equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),0x6c7bn);
  equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);
  equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24ffen),0n);
 }
 if(mode===64){
  const high=0x123400000000n;vm.map(high,4096n,3);
  equal(call(t,'SystemParametersInfoW',[0x46n,0n,high,0n]),1n);equal(vm.readInt(high),0n);
 }
 for(const name of ['GetDoubleClickTime','GetDialogBaseUnits','SystemParametersInfoA','SystemParametersInfoW']){
  assert.ok(vm.bind('user32.dll',name));
  for(const dll of ['kernel32.dll','kernelbase.dll','dwmapi.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 }
 equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(vm.xmm(3),new Uint8Array(16).fill(0x7b));equal(vm.mxcsr,0x5f81);equal(fp(t),fpu);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();other.vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 // A missing return frame must not commit a process setting.
 frame(t,'SystemParametersInfoW',[0x47n,1n,0n,0n]);
 equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,1),0);const before=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);
 equal(core.wire_protect(t.handle,t.sp&~4095n,4096n,4),0);
 equal(call(t,'SystemParametersInfoA',[0x46n,0n,0x22000n,0n]),1n);equal(vm.readInt(0x22000n),0n);
 // Unsupported settings report through the checked TEB error slot.
 frame(t,'SystemParametersInfoA',[0xffffn,0n,0xdeadn,0n]);
 equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const errorBefore=snapshot(t);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),errorBefore);
 equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(t.error),50n);
 vm.dispose();
}
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,hwnd=await window(t),p=0x24000n;
 const dwm=(name,args)=>call(t,name,args,'dwmapi.dll');
 vm.write(t.error,encode(123n,4));vm.write(p,new Uint8Array(24).fill(0xa5));equal(dwm('DwmIsCompositionEnabled',[p]),0n);equal(vm.readInt(p),0n);equal(vm.readInt(p+4n),0xa5a5a5a5n);equal(dwm('DwmIsCompositionEnabled',[0n]),0x80070057n);
 equal(dwm('DwmGetWindowAttribute',[hwnd,9n,p,16n]),0n);equal(readRect(t,p),[10,20,26,32]);equal(vm.readInt(p+16n),0xa5a5a5a5n);equal(dwm('DwmGetWindowAttribute',[hwnd,14n,p,4n]),0n);equal(vm.readInt(p),0n);
 for(const [args,result]of [[[1n,9n,1n,16n],0x80070006n],[[hwnd,9n,0n,16n],0x80070057n],[[hwnd,9n,p,4n],0x80070057n],[[hwnd,1n,p,4n],0x80070057n]])equal(dwm('DwmGetWindowAttribute',args),result);
 vm.write(0x24ffen,new Uint8Array(16).fill(0xa5));frame(t,'DwmGetWindowAttribute',[hwnd,9n,0x24ffen,16n],'dwmapi.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(readRect(t,0x24ffen),[10,20,26,32]);
 for(const name of ['DwmIsCompositionEnabled','DwmGetWindowAttribute'])for(const dll of ['user32.dll','kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
// Station routes must reject the legacy synthetic success and preserve query outputs.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['GetProcessWindowStation','GetUserObjectInformationW']){
  for(const dll of ['kernel32.dll','kernelbase.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
  vm.write(0x24000n,new Uint8Array(16).fill(0xa5));
  equal(call(t,name,[0x70070000n,1n,0x24000n,12n,0x2400cn]),0n);equal(vm.readInt(t.error),120n);equal(vm.readInt(0x24000n,8),0xa5a5a5a5a5a5a5a5n);equal(vm.readInt(0x2400cn),0xa5a5a5a5n);
  frame(t,name,[0n,0n,0xdeadn,0xffffffffn,0xdeadn]);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 }
 equal(t.time.calls,0);vm.dispose();
}
console.log(`${checks} desktop settings/station/DWM/ABI/isolation/frame/fault checks passed`);
