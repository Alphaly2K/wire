import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';
const names=['WSAStartup','WSACleanup','WSAGetLastError','WSASetLastError','WSACreateEvent','WSACloseEvent','WSASetEvent','WSAResetEvent','WSAWaitForMultipleEvents','htonl','htons','ntohl','ntohs','inet_pton','InetPtonW','inet_ntop','InetNtopW','socket','WSASocketA','WSASocketW'];
const api=(t,name,args=[])=>call(t,name,args,'ws2_32.dll');
const data=0x22000n,out=0x24000n;
const text=(vm,p,s,wide=false)=>vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));
const raw=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const rendered=(vm,p,wide)=>{let s='';for(let n=0;n<64;n++){const v=vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1);if(v===0n)return s;s+=String.fromCharCode(Number(v));}throw new Error('missing terminator');};
for(const mode of [32,64]){
 const t=await machine(mode),other=await machine(mode),{vm}=t,width=mode/8,invalid=(1n<<BigInt(mode))-1n;
 for(const name of names){assert.ok(vm.bind('ws2_32.dll',name));for(const dll of ['kernel32.dll','kernelbase.dll','wsock32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);}
 for(const [ordinal,name]of [[8,'htonl'],[9,'htons'],[14,'ntohl'],[15,'ntohs'],[23,'socket'],[111,'WSAGetLastError'],[112,'WSASetLastError'],[115,'WSAStartup'],[116,'WSACleanup']])equal(vm.bind('ws2_32.dll','#'+ordinal),vm.bind('ws2_32.dll',name));
 equal(api(t,'socket',[2n,1n,6n]),invalid);equal(api(t,'WSAGetLastError'),10093n);equal(api(t,'WSACreateEvent'),0n);equal(api(t,'WSAStartup',[0n,0n]),10092n);equal(api(t,'WSAStartup',[0x202n,0n]),10014n);
 equal(api(t,'WSAStartup',[0n,data]),10092n);equal(api(t,'WSACreateEvent'),0n);equal(api(t,'WSAGetLastError'),10093n);vm.flags=0x8d5n;vm.write(t.error,encode(123n,4));
 for(const [version,negotiated]of [[0x0001n,0x0001n],[0xff01n,0x0101n],[0x0002n,0x0002n],[0x0302n,0x0202n],[0x0003n,0x0202n]]){
  vm.write(data,new Uint8Array(420).fill(0xa5));equal(api(t,'WSAStartup',[version|0xabcdef0000000000n,data]),0n);equal(vm.readInt(data,2),negotiated);equal(vm.readInt(data+2n,2),0x202n);const strings=mode===32?4n:16n;equal(rendered(vm,data+strings,false),'Wire Winsock services');equal(rendered(vm,data+strings+257n,false),'Running');equal(vm.readInt(data+BigInt(mode===32?396:8),width),BigInt('0x'+'a5'.repeat(width)));equal(vm.readInt(data+BigInt(mode===32?394:402),2),0xa5a5n);equal(vm.readInt(data+BigInt(mode===32?390:4),2),(version&255n)===1n?32767n:0n);
 }
 equal(api(other,'WSACreateEvent'),0n);equal(api(other,'WSAGetLastError'),10093n);
 for(const [name,value,expected]of [['htonl',0x12345678n,0x78563412n],['ntohl',0x78563412n,0x12345678n],['htons',0x1234n,0x3412n],['ntohs',0x3412n,0x1234n]])equal(api(t,name,[value|0xabcdef0000000000n]),expected);
 for(const [name,args,error]of [['socket',[2n,1n,6n],10047n],['WSASocketA',[2n,1n,6n,1n,0n,0n],10045n],['WSASocketW',[23n,2n,0n,0n,0n,0n],10047n]]){equal(api(t,name,args),invalid);equal(api(t,'WSAGetLastError'),error);}
 const event=api(t,'WSACreateEvent');assert.ok(event);vm.write(out,encode(event,width));equal(api(t,'WSAWaitForMultipleEvents',[1n,out,0n,0n,0n]),258n);equal(api(t,'WSASetEvent',[event]),1n);for(let n=0;n<2;n++){equal(api(t,'WSAWaitForMultipleEvents',[1n,out,0n,0n,0n]),0n);equal(call(t,'WaitForSingleObject',[event,0n],'kernel32.dll'),0n);}equal(call(t,'ResetEvent',[event],'kernel32.dll'),1n);equal(api(t,'WSAWaitForMultipleEvents',[1n,out,0n,0n,1n]),258n);equal(api(t,'WSAWaitForMultipleEvents',[0n,1n,0n,0n,0n]),0xffffffffn);equal(api(t,'WSAGetLastError'),10022n);
 for(let n=0;n<5;n++)equal(api(t,'WSACleanup'),0n);equal(api(t,'WSACleanup'),0xffffffffn);equal(api(t,'WSAGetLastError'),10093n);equal(api(t,'WSASetEvent',[event]),1n);equal(api(t,'WSAResetEvent',[event]),1n);equal(api(t,'WSACloseEvent',[event]),1n);equal(api(t,'WSACloseEvent',[event]),0n);equal(api(t,'WSAGetLastError'),6n);
 for(const [literal,family,expected]of [['127.2.3.255',2n,[127,2,3,255]],['::1',23n,[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1]],['2001:db8::12',23n,[32,1,13,184,0,0,0,0,0,0,0,0,0,0,0,18]],['::192.0.2.10',23n,[0,0,0,0,0,0,0,0,0,0,0,0,192,0,2,10]]])for(const wide of [false,true]){
  vm.write(t.error,encode(123n,4));text(vm,data,literal,wide);equal(api(t,wide?'InetPtonW':'inet_pton',[family|0x123400000000n,data,out]),1n);equal(raw(vm,out,expected.length),expected);equal(api(t,wide?'InetNtopW':'inet_ntop',[family,out,data,128n]),data);equal(rendered(vm,data,wide),literal);equal(api(t,'WSAGetLastError'),123n);vm.write(out,new Uint8Array(32).fill(0xa5));text(vm,data,'invalid',wide);equal(api(t,wide?'InetPtonW':'inet_pton',[family,data,out]),0n);equal(vm.readInt(out,8),0xa5a5a5a5a5a5a5a5n);equal(api(t,'WSAGetLastError'),wide?10022n:123n);
 }
 for(const wide of [false,true]){const pton=wide?'InetPtonW':'inet_pton',ntop=wide?'InetNtopW':'inet_ntop';equal(api(t,pton,[99n,0n,1n]),0xffffffffn);equal(api(t,'WSAGetLastError'),10014n);equal(api(t,pton,[99n,1n,1n]),0xffffffffn);equal(api(t,'WSAGetLastError'),10047n);vm.write(out,[127,0,0,1]);equal(api(t,ntop,[2n,out,data,3n]),0n);equal(api(t,'WSAGetLastError'),wide?87n:0xc000000dn);}
 // Full WSADATA preflight must keep the reference count unchanged on failure.
 vm.write(0x24ff0n,new Uint8Array(420).fill(0xa5));frame(t,'WSAStartup',[0x202n,0x24ff0n],'ws2_32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(0x24ff0n,8),0xa5a5a5a5a5a5a5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(api(t,'WSACleanup'),0n);equal(api(t,'WSACleanup'),0xffffffffn);
 text(vm,data,'127.0.0.1');frame(t,'inet_pton',[2n,data,0x24ffen],'ws2_32.dll');vm.write(0x24ffen,[0xa5,0xa5]);equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const addressBefore=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),addressBefore);equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.readInt(0x24ffen),0x0100007fn);
 if(mode===64){const high=0x123400000000n;vm.map(high,4096n,3);text(vm,high,'::1');equal(api(t,'inet_pton',[23n,high,high+128n]),1n);equal(api(t,'InetNtopW',[23n,high+128n,high+256n,64n]),high+256n);equal(rendered(vm,high+256n,true),'::1');}
 equal(api(t,'WSASetLastError',[0x1234567800000042n]),0n);equal(api(t,'WSAGetLastError'),66n);equal(vm.flags,0x8d5n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();other.vm.dispose();
}
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2040),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2040);f.rput(0x2010,0x2140);f.rtext(0x2040,'ws2_32.dll\0');f.bytes[f.offset(0x1000)]=0xc3;
 for(const [n,name]of names.entries()){const at=0x2200+n*48;f.rput(0x2080+n*f.ptr,at,f.ptr);f.rput(0x2140+n*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');}f.rput(0x2080,(1n<<BigInt(mode-1))|115n,f.ptr);f.rput(0x2140,(1n<<BigInt(mode-1))|115n,f.ptr);
 const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const [n,name]of names.entries())equal(vm.readInt(image.base+0x2140n+BigInt(n*f.ptr),f.ptr),vm.bind('ws2_32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
}
for(const mode of [32,64]){
 let now=0n;const t=await machine(mode,{clock:()=>now}),{vm}=t;equal(api(t,'WSAStartup',[0x202n,data]),0n);const event=api(t,'WSACreateEvent');vm.write(out,encode(event,mode/8));frame(t,'WSAWaitForMultipleEvents',[1n,out,0n,5n,0n],'ws2_32.dll');const before=snapshot(t),work=vm.work;equal((await vm.run(100n)).reason,Exit.Waiting);equal(snapshot(t),before);vm.write(out,encode(0n,mode/8));vm.write(t.sp,encode(0xdeadbeefn,mode/8));now=49999n;equal((await vm.run(100n)).reason,Exit.Waiting);now=50000n;equal((await vm.run(1n)).reason,Exit.Budget);equal(vm.ip,vm.entryPoints.returned);equal(vm.register(0),258n);equal(vm.work,work+1n);vm.dispose();
}
console.log(`${checks} Winsock startup/layout/address/event/provider/ABI/fault checks passed`);
