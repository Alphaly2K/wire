import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';
import {WasmMachine} from '../web/runtime.mjs';

const api=(t,name,args=[])=>call(t,name,args,'ws2_32.dll');
const data=0x22000n,out=0x23000n,buffer=0x24000n;
function text(t,s,wide=false,p=data){t.vm.write(p,Buffer.from(s+'\0',wide?'utf16le':'ascii'));return p;}
function string(vm,p,wide=false){let s='';for(let n=0;n<256;n++){const c=Number(vm.readInt(p+BigInt(n*(wide?2:1)),wide?2:1));if(!c)return s;s+=String.fromCharCode(c);}throw Error('unterminated name');}
const bytes=(vm,p,n)=>Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const start=t=>equal(api(t,'WSAStartup',[0x202n,0x20000n]),0n);
const error=(t,n)=>equal(t.vm.readInt(t.error),BigInt(n));
const sockets=[['accept',3],['bind',3],['closesocket',1],['connect',3],['getpeername',3],['getsockname',3],['getsockopt',5],['ioctlsocket',3],['listen',2],['recv',4],['recvfrom',6],['send',4],['sendto',6],['setsockopt',5],['shutdown',2],['WSAAsyncSelect',4],['WSAEventSelect',3],['WSAEnumNetworkEvents',3],['WSAIoctl',9],['WSASendDisconnect',2],['WSARecvDisconnect',2],['WSARecv',7],['WSARecvFrom',9],['WSASend',7],['WSASendTo',9]];
const asyncNames=[['WSAAsyncGetHostByAddr',7],['WSAAsyncGetHostByName',5],['WSAAsyncGetProtoByNumber',5],['WSAAsyncGetProtoByName',5],['WSAAsyncGetServByPort',6],['WSAAsyncGetServByName',6]];
const added=[...sockets.map(x=>x[0]),...asyncNames.map(x=>x[0]),'select','inet_addr','inet_ntoa','gethostbyaddr','gethostbyname','getprotobyname','getprotobynumber','getservbyname','getservbyport','gethostname','WSACancelAsyncRequest','WSASetBlockingHook','WSAUnhookBlockingHook','WSACancelBlockingCall','WSAIsBlocking','__WSAFDIsSet','getaddrinfo','GetAddrInfoW','freeaddrinfo','FreeAddrInfoW','GetAddrInfoExW','FreeAddrInfoExW','GetAddrInfoExCancel','getnameinfo','GetNameInfoW','GetHostNameW','GetAddrInfoExOverlappedResult'];
equal(added.length,58);

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,invalid=(1n<<BigInt(mode))-1n;
 for(const initialized of [false,true]){
  if(initialized)start(t);
  for(const[name,arity]of sockets){vm.write(buffer,new Uint8Array(64).fill(0xa5));equal(api(t,name,Array(arity).fill(buffer)),name==='accept'?invalid:0xffffffffn);error(t,initialized?10038:10093);equal(bytes(vm,buffer,64),new Array(64).fill(0xa5));equal(vm.register(4),t.sp+BigInt(mode===32?(arity+1)*4:8));}
  for(const[name,arity]of asyncNames){equal(api(t,name,Array(arity).fill(buffer)),0n);error(t,initialized?10045:10093);}
  for(const name of ['getprotobyname','getprotobynumber','getservbyname','getservbyport']){equal(api(t,name,[1n,1n]),0n);error(t,initialized?11004:10093);}
  for(const name of ['WSACancelAsyncRequest','WSACancelBlockingCall']){equal(api(t,name,[1n]),0xffffffffn);error(t,initialized?10022:10093);}
 }
 vm.write(t.error,encode(123n,4));equal(api(t,'WSAIsBlocking'),0n);equal(api(t,'WSASetBlockingHook',[0n]),0n);equal(api(t,'WSAUnhookBlockingHook'),0n);error(t,123);equal(api(t,'WSASetBlockingHook',[1n]),0n);error(t,10045);
 equal(api(t,'select',[0n,0n,0n,0n,1n]),0xffffffffn);error(t,10022);vm.write(buffer,encode(0n,4));equal(api(t,'select',[0n,buffer,0n,0n,1n]),0xffffffffn);error(t,10022);vm.write(buffer,encode(1n,4));equal(api(t,'select',[0n,buffer,0n,0n,1n]),0xffffffffn);error(t,10038);
 vm.write(t.error,encode(123n,4));
 for(const[input,ip]of [['127.1',[127,0,0,1]],['0x7f.01',[127,0,0,1]],['127.0.1',[127,0,0,1]],['0177.0.0.1',[127,0,0,1]],['2130706433',[127,0,0,1]],['4.003.002.0x10',[4,3,2,16]],[' ',[0,0,0,0]],['255.255.255.255',[255,255,255,255]]]){
  const n=ip.reduce((v,b,i)=>v+(BigInt(b)<<BigInt(i*8)),0n);equal(api(t,'inet_addr',[text(t,input)]),n);const p=api(t,'inet_ntoa',[n|(1n<<40n)]);assert.ok(p);equal(string(vm,p),ip.join('.'));error(t,123);
 }
 for(const input of ['', '1.2.3.256','1.2.65536','1.16777216','4294967296','09.1','1..2','+1','1.2.3.4junk'])equal(api(t,'inet_addr',[text(t,input)]),0xffffffffn);
 equal(api(t,'inet_addr',[0n]),0xffffffffn);error(t,123);
 const first=api(t,'inet_ntoa',[0x0100007fn]);equal(api(t,'inet_ntoa',[0x0200007fn]),first);equal(string(vm,first),'127.0.0.2');
 const fd=mode===32?0xabcdef01n:0x12345678abcdef01n;vm.write(buffer,new Uint8Array(40));vm.write(buffer,encode(2n,4));vm.write(buffer+BigInt(mode/8),encode(fd,mode/8));vm.write(buffer+BigInt(mode/4),encode(1n,mode/8));equal(api(t,'__WSAFDIsSet',[fd,buffer]),1n);equal(api(t,'__WSAFDIsSet',[2n,buffer]),0n);if(mode===64)equal(api(t,'__WSAFDIsSet',[fd&0xffffffffn,buffer]),0n);equal(api(t,'__WSAFDIsSet',[fd,0n]),0n);error(t,123);
 for(const wide of [false,true]){
  vm.write(buffer,new Uint8Array(16).fill(0xa5));equal(api(t,wide?'GetHostNameW':'gethostname',[buffer,4n]),0xffffffffn);error(t,10014);equal(bytes(vm,buffer,16),new Array(16).fill(0xa5));equal(api(t,wide?'GetHostNameW':'gethostname',[buffer,256n]),0n);equal(string(vm,buffer,wide),'wire');
 }
 const host=api(t,'gethostbyname',[text(t,'WIRE')]),width=mode/8;assert.ok(host);equal(string(vm,vm.readInt(host,width)),'wire');equal(vm.readInt(host+BigInt(width*2),2),2n);equal(vm.readInt(host+BigInt(width*2+2),2),4n);const list=vm.readInt(host+BigInt(mode===32?12:24),width);equal(bytes(vm,vm.readInt(list,width),4),[127,0,0,1]);equal(vm.readInt(list+BigInt(width),width),0n);
 vm.write(data,[127,0,0,1]);equal(api(t,'gethostbyaddr',[data,4n,2n]),host);equal(string(vm,vm.readInt(host,width)),'localhost');equal(api(t,'gethostbyname',[text(t,'unconfigured.invalid')]),0n);error(t,11001);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

function addrInfo(t,node,service,wide=false,{ex=false,hints=0n,output=out,overlapped=0n,cancel=0n,namespace=12n}={}){
 const n=node===null?0n:text(t,node,wide,data),s=service===null?0n:text(t,service,wide,data+512n);
 return api(t,ex?'GetAddrInfoExW':wide?'GetAddrInfoW':'getaddrinfo',ex?[n,s,namespace,0n,hints,output,0n,overlapped,0n,cancel]:[n,s,hints,output]);
}
for(const mode of [32,64])for(const wide of [false,true]){
 const t=await machine(mode),{vm}=t,width=mode/8;equal(addrInfo(t,'127.0.0.1','80',wide),10093n);error(t,10093);equal(vm.readInt(out,width),0n);start(t);
 for(const[node,family,ip]of [['127.0.0.1',2,[127,0,0,1]],['::1',23,new Array(15).fill(0).concat(1)],['localhost',2,[127,0,0,1]],['wire',2,[127,0,0,1]]]){
  vm.write(t.error,encode(123n,4));equal(addrInfo(t,node,'80',wide),0n);error(t,123);const p=vm.readInt(out,width);assert.ok(p);equal(vm.readInt(p+4n),BigInt(family));equal(vm.readInt(p+16n,width),family===2?16n:28n);const sock=vm.readInt(p+16n+BigInt(width*2),width);equal(vm.readInt(sock,2),BigInt(family));equal(bytes(vm,sock+2n,2),[0,80]);equal(bytes(vm,sock+BigInt(family===2?4:8),ip.length),ip);equal(vm.readInt(p+16n+BigInt(width*3),width),0n);
  equal(api(t,wide?'GetNameInfoW':'getnameinfo',[sock,BigInt(family===2?16:28),buffer,64n,buffer+256n,16n,10n]),0n);equal(string(vm,buffer,wide),family===2?'127.0.0.1':'::1');equal(string(vm,buffer+256n,wide),'80');
  api(t,wide?'FreeAddrInfoW':'freeaddrinfo',[p]);assert.throws(()=>vm.readInt(p),/memory access/);
 }
 vm.write(0x25000n,new Uint8Array(72));vm.write(0x25000n,encode(3n,4));vm.write(0x25004n,encode(23n,4));vm.write(0x25008n,encode(1n,4));equal(addrInfo(t,'localhost','443',wide,{hints:0x25000n}),0n);let p=vm.readInt(out,width);equal(vm.readInt(p+8n),1n);equal(vm.readInt(p+12n),6n);equal(string(vm,vm.readInt(p+16n+BigInt(width),width),wide),'localhost');api(t,'freeaddrinfo',[p]);
 vm.write(0x25000n,encode(1n,4));equal(addrInfo(t,null,'0',wide,{hints:0x25000n}),0n);p=vm.readInt(out,width);equal(bytes(vm,vm.readInt(p+16n+BigInt(width*2),width)+8n,16),new Array(16).fill(0));api(t,'freeaddrinfo',[p]);
 for(const[node,service,code]of [['unconfigured.invalid','80',11001],['127.0.0.1','http',10109],['127.0.0.1','65536',10109],[null,null,11001]]){vm.write(out,new Uint8Array(width).fill(0xa5));equal(addrInfo(t,node,service,wide),BigInt(code));error(t,code);equal(vm.readInt(out,width),0n);}
 vm.write(0x25000n,new Uint8Array(72));vm.write(0x25000n,encode(4n,4));equal(addrInfo(t,'localhost','80',wide,{hints:0x25000n}),11001n);vm.write(0x25000n,encode(16n,4));equal(addrInfo(t,'127.0.0.1','80',wide,{hints:0x25000n}),10022n);vm.write(0x25000n,new Uint8Array(72));vm.write(0x25010n,encode(1n,width));equal(addrInfo(t,'127.0.0.1','80',wide,{hints:0x25000n}),11003n);
 equal(addrInfo(t,'127.0.0.1','80',wide,{output:t.sp}),0n);p=vm.readInt(t.sp,width);equal(vm.ip,vm.entryPoints.returned);api(t,'freeaddrinfo',[p]);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,width=mode/8;start(t);equal(addrInfo(t,'::1','443',true,{ex:true,cancel:out+16n}),0n);const p=vm.readInt(out,width);equal(vm.readInt(out+16n,width),0n);equal(vm.readInt(p+BigInt(mode===32?40:64),width),0n);api(t,'FreeAddrInfoExW',[p]);assert.throws(()=>vm.readInt(p),/memory access/);
 vm.write(buffer,new Uint8Array(32).fill(0xa5));equal(addrInfo(t,'localhost','80',true,{ex:true,overlapped:buffer,cancel:out+16n}),10045n);equal(vm.readInt(out,width),0n);equal(vm.readInt(out+16n,width),0n);equal(bytes(vm,buffer,32),new Array(32).fill(0xa5));equal(addrInfo(t,'localhost','80',true,{ex:true,namespace:37n}),10108n);
 equal(addrInfo(t,'localhost','80',true,{ex:true,cancel:out}),10022n);equal(vm.readInt(out,width),0n);
 equal(api(t,'GetAddrInfoExCancel',[1n]),6n);equal(api(t,'GetAddrInfoExOverlappedResult',[1n]),6n);
 for(const[name,args]of [['getaddrinfo',[text(t,'127.0.0.1'),0n,0n,0x24ffen]],['GetAddrInfoExW',[text(t,'::1',true),0n,12n,0n,0n,out,0n,0n,0n,0x24ffen]]]){
  vm.write(out,new Uint8Array(width).fill(0xa5));vm.write(0x24ffen,[0xa5,0xa5]);frame(t,name,args,'ws2_32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);const before=snapshot(t),work=vm.work;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(bytes(vm,out,width),new Array(width).fill(0xa5));equal(vm.readInt(0x24ffen,2),0xa5a5n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const result=vm.readInt(name==='getaddrinfo'?0x24ffen:out,width);api(t,name==='getaddrinfo'?'freeaddrinfo':'FreeAddrInfoExW',[result]);
 }
 // A protected thread buffer faults without changing its previous text.
 const scratch=api(t,'inet_ntoa',[0x0100007fn]);frame(t,'inet_ntoa',[0x0200007fn],'ws2_32.dll');equal(core.wire_protect(t.handle,scratch,4096n,2),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(string(vm,scratch),'127.0.0.1');equal(core.wire_protect(t.handle,scratch,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(string(vm,scratch),'127.0.0.2');
 // NameInfo checks both output spans before writing either one.
 vm.write(data,[2,0,0,80,127,0,0,1,...new Array(8).fill(0)]);vm.write(buffer,new Uint8Array(32).fill(0xa5));frame(t,'getnameinfo',[data,16n,buffer,32n,0x24ffen,16n,10n],'ws2_32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,buffer,32),new Array(32).fill(0xa5));equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(string(vm,buffer),'127.0.0.1');
 // Error-slot faults cannot publish a NULL result before reporting the failure.
 text(t,'unconfigured.invalid');vm.write(out,new Uint8Array(width).fill(0xa5));frame(t,'getaddrinfo',[data,0n,0n,out],'ws2_32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(bytes(vm,out,width),new Array(width).fill(0xa5));equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),11001n);equal(vm.readInt(out,width),0n);
 equal(addrInfo(t,'127.0.0.1','80'),0n);const owned=vm.readInt(out,width);assert.throws(()=>api(t,'FreeAddrInfoExW',[owned]),/unowned Winsock/);equal(vm.readInt(owned+4n),2n);const heap=call(t,'GetProcessHeap',[],'kernel32.dll');equal(call(t,'HeapFree',[heap,0n,owned],'kernel32.dll'),0n);equal(call(t,'VirtualFree',[owned,0n,0x8000n],'kernel32.dll'),0n);api(t,'freeaddrinfo',[owned]);assert.throws(()=>api(t,'freeaddrinfo',[owned]),/unowned Winsock/);
 vm.dispose();
}

// A worker's address text is distinct and its owned buffer is reclaimed on exit.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,main=api(t,'inet_ntoa',[0x0100007fn]),gateway=vm.bind('ws2_32.dll','inet_ntoa');
 vm.write(0x30000n,mode===32?[0x68,...encode(0x0200007fn,4),0xb8,...encode(gateway,4),0xff,0xd0,0xa3,...encode(0x26000n,4),0x31,0xc0,0xc2,4,0]:[0x48,0x83,0xec,40,0xb9,...encode(0x0200007fn,4),0x48,0xb8,...encode(gateway,8),0xff,0xd0,0x48,0xa3,...encode(0x26000n,8),0x31,0xc0,0x48,0x83,0xc4,40,0xc3]);
 const thread=call(t,'CreateThread',[0n,65536n,0x30000n,0n,0n,out],'kernel32.dll');assert.ok(thread);frame(t,'WaitForSingleObject',[thread,0xffffffffn],'kernel32.dll');equal((await vm.run(1000n)).reason,Exit.Returned);equal(vm.register(0),0n);const worker=vm.readInt(0x26000n,mode/8);assert.ok(worker);assert.notEqual(worker,main);assert.throws(()=>vm.readInt(worker),/memory access/);equal(string(vm,main),'127.0.0.1');equal(call(t,'CloseHandle',[thread],'kernel32.dll'),1n);vm.dispose();
}

// Result ownership is finite and a failed copyout cannot consume its quota.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,all=[];start(t);
 for(let n=0;n<1024;n++){equal(addrInfo(t,'127.0.0.1','0'),0n);all.push(vm.readInt(out,mode/8));}
 equal(addrInfo(t,'127.0.0.1','0'),8n);equal(vm.readInt(out,mode/8),0n);api(t,'freeaddrinfo',[all.pop()]);equal(addrInfo(t,'127.0.0.1','0'),0n);all.push(vm.readInt(out,mode/8));for(const p of all)api(t,'freeaddrinfo',[p]);vm.dispose();
}

const ordinals=[[1,'accept'],[2,'bind'],[3,'closesocket'],[4,'connect'],[5,'getpeername'],[6,'getsockname'],[7,'getsockopt'],[10,'ioctlsocket'],[11,'inet_addr'],[12,'inet_ntoa'],[13,'listen'],[16,'recv'],[17,'recvfrom'],[18,'select'],[19,'send'],[20,'sendto'],[21,'setsockopt'],[22,'shutdown'],[51,'gethostbyaddr'],[52,'gethostbyname'],[53,'getprotobyname'],[54,'getprotobynumber'],[55,'getservbyname'],[56,'getservbyport'],[57,'gethostname'],[101,'WSAAsyncSelect'],[102,'WSAAsyncGetHostByAddr'],[103,'WSAAsyncGetHostByName'],[104,'WSAAsyncGetProtoByNumber'],[105,'WSAAsyncGetProtoByName'],[106,'WSAAsyncGetServByPort'],[107,'WSAAsyncGetServByName'],[108,'WSACancelAsyncRequest'],[109,'WSASetBlockingHook'],[110,'WSAUnhookBlockingHook'],[113,'WSACancelBlockingCall'],[114,'WSAIsBlocking'],[151,'__WSAFDIsSet']];
for(const mode of [32,64]){
 const t=await machine(mode);for(const[n,name]of ordinals)equal(t.vm.bind('ws2_32.dll','#'+n),t.vm.bind('ws2_32.dll',name));for(const name of added){assert.ok(t.vm.bind('ws2_32.dll',name));assert.throws(()=>t.vm.bind('wsock32.dll',name),/unknown/);}t.vm.dispose();
 for(let offset=0;offset<added.length;offset+=20){
  const list=added.slice(offset,offset+20),f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.put(f.dirs+12,40);f.rput(0x2000,0x2040);f.rput(0x200c,0x2028);f.rput(0x2010,0x2100);f.rtext(0x2028,'ws2_32.dll\0');let at=0x21c0;
  for(const[i,name]of list.entries()){const ordinal=ordinals.find(x=>x[1]===name)?.[0];const value=ordinal?(1n<<BigInt(mode-1))|BigInt(ordinal):BigInt(at);f.rput(0x2040+i*f.ptr,value,f.ptr);f.rput(0x2100+i*f.ptr,value,f.ptr);if(!ordinal){f.rtext(at+2,name+'\0');at+=name.length+3;}}
  assert.ok(at<0x2600);f.bytes[f.offset(0x1000)]=0xc3;f.relocations.length=0;f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of list.entries())equal(vm.readInt(image.base+0x2100n+BigInt(i*f.ptr),f.ptr),vm.bind('ws2_32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);vm.dispose();
 }
}
console.log(`${checks} Winsock ordinal/transport-failure/local-name/owned-result/ABI/fault checks passed`);
