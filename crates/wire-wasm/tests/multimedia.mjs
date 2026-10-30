import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'winmm.dll');
for(const mode of [32,64]){
 const t=await machine(mode),other=await machine(mode),{vm}=t,out=0x24000n;
 for(const name of ['timeGetDevCaps','timeBeginPeriod','timeEndPeriod']){assert.ok(vm.bind('winmm.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 vm.flags=0x8d5n;vm.write(t.error,encode(123n,4));vm.write(out,new Uint8Array(16).fill(0xa5));equal(api(t,'timeGetDevCaps',[out,8n]),0n);equal(vm.readInt(out,8),0xffffffff00000001n);equal(vm.readInt(out+8n,8),0xa5a5a5a5a5a5a5a5n);equal(api(t,'timeGetDevCaps',[1n,7n]),97n);equal(api(t,'timeGetDevCaps',[0n,8n]),97n);
 for(const name of ['timeBeginPeriod','timeEndPeriod'])equal(api(t,name,[0x100000000n]),97n);
 equal(api(t,'timeBeginPeriod',[0x100000001n]),0n);equal(api(t,'timeBeginPeriod',[1n]),0n);equal(api(other,'timeEndPeriod',[1n]),mode===32?0n:97n);equal(api(t,'timeEndPeriod',[1n]),0n);equal(api(t,'timeEndPeriod',[1n]),0n);equal(api(t,'timeEndPeriod',[1n]),mode===32?0n:97n);
 if(mode===64){for(let n=1n;n<=256n;n++)equal(api(t,'timeBeginPeriod',[n]),0n);equal(api(t,'timeBeginPeriod',[257n]),97n);equal(api(t,'timeBeginPeriod',[1n]),0n);equal(api(t,'timeEndPeriod',[2n]),0n);equal(api(t,'timeBeginPeriod',[257n]),0n);}
 vm.write(0x24ffen,new Uint8Array(8).fill(0xa5));frame(t,'timeGetDevCaps',[0x24ffen,8n],'winmm.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);
 if(mode===32){equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),97n);}else{equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);}
 equal(vm.readInt(0x24ffen,2),mode===32?0xa5a5n:1n);equal(vm.readInt(t.error),123n);equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();other.vm.dispose();
}
console.log(`${checks} multimedia capabilities/period ownership/ABI/fault checks passed`);
