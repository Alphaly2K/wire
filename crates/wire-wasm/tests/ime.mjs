import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call,window} from './gdi-harness.mjs';
const names=['ImmGetContext','ImmReleaseContext','ImmGetCompositionStringA','ImmGetCompositionStringW','ImmSetCompositionStringA','ImmSetCompositionStringW','ImmNotifyIME','ImmGetDefaultIMEWnd','ImmIsIME','ImmGetOpenStatus','ImmSetOpenStatus','ImmAssociateContext','ImmAssociateContextEx','ImmSetCompositionFontW','ImmSetConversionStatus','ImmGetConversionStatus','ImmSetCompositionWindow'];
const api=(t,name,args=[])=>call(t,name,args,'imm32.dll');
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,h=await window(t),out=0x24000n;
 for(const name of names){assert.ok(vm.bind('imm32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 vm.flags=0x8d5n;vm.write(t.error,encode(123n,4));equal(api(t,'ImmIsIME',[1n]),0n);equal(api(t,'ImmGetContext',[h]),0n);equal(api(t,'ImmGetDefaultIMEWnd',[h]),0n);equal(api(t,'ImmAssociateContext',[h,0n]),0n);equal(api(t,'ImmAssociateContextEx',[h,0n,0n]),1n);equal(api(t,'ImmAssociateContextEx',[h,0n,1n]),1n);equal(vm.readInt(t.error),123n);
 for(const name of ['ImmGetContext','ImmReleaseContext','ImmAssociateContext','ImmAssociateContextEx']){equal(api(t,name,Array(6).fill(1n)),0n);equal(vm.readInt(t.error),1400n);}
 equal(api(t,'ImmGetDefaultIMEWnd',[1n]),0n);equal(vm.readInt(t.error),1400n);
 for(const name of names.filter(n=>!['ImmGetContext','ImmAssociateContext','ImmAssociateContextEx','ImmGetDefaultIMEWnd','ImmIsIME'].includes(n))){vm.write(out,new Uint8Array(64).fill(0xa5));equal(api(t,name,[h,0n,out,64n,1n,64n]),0n);equal(vm.readInt(t.error),6n);equal(vm.readInt(out),0xa5a5a5a5n);}
 frame(t,'ImmGetConversionStatus',[1n,1n,1n],'imm32.dll');const before=snapshot(t);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,2),0);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,t.error&~4095n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} IME shared-profile/ABI/invalid-context/output-preservation checks passed`);
