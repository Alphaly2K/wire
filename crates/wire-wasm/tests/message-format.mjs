import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
const api=(t,name,args=[])=>call(t,name,args,'kernel32.dll');
const encoded=(text,wide)=>wide?Array.from({length:text.length+1},(_,n)=>Array.from(encode(BigInt(n<text.length?text.charCodeAt(n):0),2))).flat():Array.from(new TextEncoder().encode(text+'\0'));
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const message=(t,wide,args)=>api(t,'FormatMessage'+(wide?'W':'A'),args);
const procedure='The specified procedure could not be found.\r\n';

for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x23000n,slot=0x24000n,width=wide?2:1,ptr=mode/8;
 vm.flags=0x8d7n;vm.mxcsr=0x5f81;vm.xmm(3,new Uint8Array(16).fill(0x79));
 for(const [code,text]of [[0,'The operation completed successfully.\r\n'],[2,'The system cannot find the file specified.\r\n'],[127,procedure]]){
  vm.write(out,new Uint8Array(256).fill(0xa5));vm.write(t.error,encode(123n,4));equal(message(t,wide,[0x1200n,0xdead0000n,BigInt(code),0n,out,BigInt(text.length),0n]),0n);equal(vm.readInt(t.error),122n);equal(bytes(vm,out,256),new Array(256).fill(0xa5));
  vm.write(t.error,encode(123n,4));equal(message(t,wide,[0x1200n,0xdead0000n,BigInt(code),0x409n,out,256n,0n]),BigInt(text.length));equal(bytes(vm,out,encoded(text,wide).length),encoded(text,wide));equal(vm.readInt(t.error),123n);
 }
 for(const code of [3,5,6,8,32,50,87,122,126,183,203]){const count=message(t,wide,[0x1000n,0n,BigInt(code),0x400n,out,256n,0n]);assert.ok(count>0n);equal(vm.readInt(out+(count-2n)*BigInt(width),width),13n);equal(vm.readInt(out+(count-1n)*BigInt(width),width),10n);equal(vm.readInt(out+count*BigInt(width),width),0n);}
 for(const [flags,language,expected]of [[0n,0n,87n],[0x1400n,0n,87n],[0x4000n,0n,87n],[0x2000n,0n,87n],[0x1800n,0n,50n],[0x800n,0n,50n],[0x1001n,0n,50n],[0x1000n,0x411n,50n]]){equal(message(t,wide,[flags,0xdead0000n,127n,language,0xbeef0000n,0n,0n]),0n);equal(vm.readInt(t.error),expected);}
 vm.write(slot,encode(0xfeedn,ptr));equal(message(t,wide,[0x1300n,0n,999999n,0n,slot,128n,0n]),0n);equal(vm.readInt(t.error),317n);equal(vm.readInt(slot,ptr),0xfeedn);
 vm.write(input,encoded('value %1',wide));equal(message(t,wide,[0x400n,input,0n,0n,out,100n,0n]),0n);equal(vm.readInt(t.error),50n);equal(message(t,wide,[0x2600n,input,0n,0n,out,100n,0xdead0000n]),8n);equal(bytes(vm,out,encoded('value %1',wide).length),encoded('value %1',wide));
 for(const text of ['plain','',wide?'日本 %1':'text %1']){
  const data=encoded(text,wide);vm.write(input,data);vm.write(t.error,encode(123n,4));equal(message(t,wide,[0x700n,input,0n,0n,slot,128n,0xbeef0000n]),BigInt(text.length));const p=vm.readInt(slot,ptr);assert.ok(p!==0n);equal(api(t,'LocalSize',[p]),BigInt(128*width));equal(bytes(vm,p,data.length),data);equal(vm.readInt(p+BigInt(128*width-1),1),0n);equal(vm.readInt(t.error),123n);equal(api(t,'LocalFree',[p]),0n);
 }
 if(!wide){const data=[0x93,0xfa,0x96,0x7b,32,37,49,0];vm.write(input,data);equal(message(t,false,[0x600n,input,0n,0n,out,100n,0n]),7n);equal(bytes(vm,out,data.length),data);}
 else {vm.write(input,[0,0xd8,0,0]);equal(message(t,true,[0x600n,input,0n,0n,0xdead0000n,100n,0n]),0n);equal(vm.readInt(t.error),1113n);}
 vm.write(input,encoded('alias',wide));equal(message(t,wide,[0x700n,input,0n,0n,input,0n,0n]),5n);const alias=vm.readInt(input,ptr);equal(bytes(vm,alias,encoded('alias',wide).length),encoded('alias',wide));equal(api(t,'LocalSize',[alias]),BigInt(6*width));equal(api(t,'LocalFree',[alias]),0n);
 vm.write(input,encoded('saved',wide));equal(message(t,wide,[0x600n,input,0n,0n,t.sp,100n,0n]),5n);equal(vm.ip,vm.entryPoints.returned);equal(bytes(vm,t.sp,encoded('saved',wide).length),encoded('saved',wide));equal(message(t,wide,[0x700n,input,0n,0n,t.sp,0n,0n]),5n);equal(vm.ip,vm.entryPoints.returned);equal(api(t,'LocalFree',[vm.readInt(t.sp,ptr)]),0n);
 equal(vm.flags,0x8d7n);equal(vm.mxcsr,0x5f81);equal(vm.xmm(3),new Uint8Array(16).fill(0x79));equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Output pointer preflight and recycled-block checks must precede allocation publication.
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,slot=0x24ffcn,width=wide?2:1,ptr=mode/8,name='FormatMessage'+(wide?'W':'A');
 const p=api(t,'LocalAlloc',[0n,256n]);equal(api(t,'LocalFree',[p]),0n);vm.write(slot,new Uint8Array(8).fill(0xa5));frame(t,name,[0x1300n,0n,127n,0n,slot,128n,0n],'kernel32.dll');
 const blocked=mode===64?0x25000n:0x24000n;equal(core.wire_protect(t.handle,blocked,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(slot),0xa5a5a5a5n);equal(core.wire_protect(t.handle,blocked,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(slot,ptr),p);equal(api(t,'LocalFree',[p]),0n);
 vm.write(slot,encode(0xfeedn,ptr));frame(t,name,[0x1300n,0n,127n,0n,slot,128n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,p,4096n,0x104),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(slot,ptr),0xfeedn);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(slot,ptr),p);equal(api(t,'LocalSize',[p]),BigInt(128*width));equal(api(t,'LocalFree',[p]),0n);
 vm.write(slot,encode(0xfeedn,ptr));frame(t,name,[0x1300n,0n,127n,0n,slot,0xffffffffn,0n],'kernel32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(slot,ptr),0xfeedn);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),0n);equal(vm.readInt(t.error),8n);equal(vm.readInt(slot,ptr),0xfeedn);
 const out=0x24ffen;vm.write(out,[0xa5,0xa5]);frame(t,name,[0x1200n,0n,127n,0n,out,128n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(out,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),45n);
 vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,input=0x22000n,out=0x23000n;
 vm.write(input,encoded('input',true));frame(t,'FormatMessageW',[0x600n,input,0n,0n,out,100n,0n],'kernel32.dll');equal(core.wire_protect(t.handle,input,4096n,1),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,input,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);
 frame(t,'FormatMessageW',[0x1300n,0n,127n,0n,out,100n,0n],'kernel32.dll');const page=t.sp&~4095n;equal(core.wire_protect(t.handle,page,4096n,1),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(api(t,'LocalFree',[vm.readInt(out,mode/8)]),0n);
 if(mode===64){const high=api(t,'VirtualAlloc',[0n,4096n,0x103000n,4n]);assert.ok(high>0xffffffffn);equal(message(t,true,[(1n<<32n)|0x1300n,0n,(1n<<32n)|127n,(1n<<32n)|0x409n,high,(1n<<32n)|100n,0n]),45n);const p=vm.readInt(high,8);equal(api(t,'LocalSize',[p]),200n);equal(bytes(vm,p,92),encoded(procedure,true));equal(api(t,'LocalFree',[p]),0n);equal(api(t,'VirtualFree',[high,0n,0x8000n]),1n);}
 for(const name of ['FormatMessageA','FormatMessageW']){equal(vm.bind('kernelbase.dll',name),vm.bind('kernel32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}
console.log(`${checks} message text/encoding/size/heap ownership/publication/ABI/fault checks passed`);
