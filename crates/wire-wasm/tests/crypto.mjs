import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {WasmMachine} from '../web/runtime.mjs';
import {fixture} from './pe-fixture.mjs';

const names=['CryptAcquireContextA','CryptAcquireContextW','CryptContextAddRef','CryptReleaseContext','CryptCreateHash','CryptHashData','CryptGetHashParam','CryptDuplicateHash','CryptDestroyHash'];
const api=(t,name,args=[])=>call(t,name,args,'advapi32.dll');
const out=0x21000n,len=0x21040n,data=0x30000n,result=0x24000n;
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const expected=(algorithm,payload)=>Uint8Array.from(createHash(algorithm).update(payload).digest());
const pattern=n=>Uint8Array.from({length:n},(_,i)=>(i*73+(i>>>8)*11+19)&255);
function acquire(t,kind=24n,name='CryptAcquireContextW',container=0n,provider=0n,flags=0xf0000000n){equal(api(t,name,[out,container,provider,kind,flags]),1n);return t.vm.readInt(out,t.mode/8);}
function hash(t,p,algorithm=0x8004n){equal(api(t,'CryptCreateHash',[p,algorithm,0n,0n,out]),1n);return t.vm.readInt(out,t.mode/8);}
function duplicate(t,h){equal(api(t,'CryptDuplicateHash',[h,0n,0n,out]),1n);return t.vm.readInt(out,t.mode/8);}
function value(t,h,size){t.vm.write(len,encode(BigInt(size),4));equal(api(t,'CryptGetHashParam',[h,2n,result,len,0n]),1n);equal(t.vm.readInt(len),BigInt(size));return bytes(t.vm,result,size);}
function failure(t,name,args,error){equal(api(t,name,args),0n);equal(t.vm.readInt(t.error),BigInt(error));}

// Node's independent digest implementation supplies the expected values and block boundaries.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,provider=acquire(t),full=acquire(t,1n,'CryptAcquireContextA');
 for(const [kind,label]of [[24n,'Microsoft Enhanced RSA and AES Cryptographic Provider'],[1n,'Microsoft Enhanced Cryptographic Provider v1.0'],[1n,'Microsoft Base Cryptographic Provider v1.0'],[1n,'Microsoft Strong Cryptographic Provider']])for(const wide of [false,true]){
  const text=label.toLowerCase()+'\0';vm.write(0x23000n,wide?Uint8Array.from([...text].flatMap(c=>[c.charCodeAt(0),0])):new TextEncoder().encode(text));const p=acquire(t,kind,wide?'CryptAcquireContextW':'CryptAcquireContextA',0n,0x23000n,0xf0000060n);equal(api(t,'CryptReleaseContext',[p,0n]),1n);
 }
 vm.write(t.error,encode(123n,4));
 for(const [algorithm,id,size]of [['md5',0x8003n,16],['sha1',0x8004n,20],['sha256',0x800cn,32],['sha384',0x800dn,48],['sha512',0x800en,64]]){
  for(const length of [0,1,3,55,56,63,64,65,111,112,127,128,129,257,4097,65537]){
   const h=hash(t,provider,id),input=pattern(length);vm.write(data,input);
   for(let offset=0;offset<length;){const n=Math.min(length-offset,offset%2?4093:17);equal(api(t,'CryptHashData',[h,data+BigInt(offset),BigInt(n),0n]),1n);offset+=n;}
   equal(api(t,'CryptHashData',[h,0xdead0000n,0n,1n]),1n);
   for(const [param,n]of [[1n,id],[4n,BigInt(size)]]){vm.write(len,encode(4n,4));equal(api(t,'CryptGetHashParam',[h,param,result,len,0n]),1n);equal(vm.readInt(result),n);}
   equal(value(t,h,size),expected(algorithm,input));equal(value(t,h,size),expected(algorithm,input));
   failure(t,'CryptHashData',[h,0n,0n,0n],0x8009000c);equal(api(t,'CryptDestroyHash',[h]),1n);vm.write(t.error,encode(123n,4));
  }
  if(id<=0x8004n){const h=hash(t,full,id);equal(value(t,h,size),expected(algorithm,new Uint8Array()));equal(api(t,'CryptDestroyHash',[h]),1n);}
  else failure(t,'CryptCreateHash',[full,id,0n,0n,out],0x80090008);
 }
 vm.write(t.error,encode(123n,4));const original=hash(t,provider);vm.write(data,new TextEncoder().encode('prefixAB'));
 equal(api(t,'CryptHashData',[original,data,6n,0n]),1n);const copy=duplicate(t,original);
 equal(api(t,'CryptHashData',[original,data+6n,1n,0n]),1n);equal(api(t,'CryptHashData',[copy,data+7n,1n,0n]),1n);
 equal(value(t,original,20),expected('sha1',new TextEncoder().encode('prefixA')));equal(value(t,copy,20),expected('sha1',new TextEncoder().encode('prefixB')));
 const finished=duplicate(t,original);equal(value(t,finished,20),value(t,original,20));failure(t,'CryptHashData',[finished,data,1n,0n],0x8009000c);
 for(const h of [original,copy,finished])equal(api(t,'CryptDestroyHash',[h]),1n);
 // HASHVAL completes on a size-only request and on ERROR_MORE_DATA, but metadata queries do not.
 for(const output of [0n,result]){
  const h=hash(t,provider);vm.write(len,encode(0n,4));vm.write(result,new Uint8Array(20).fill(0xa5));equal(api(t,'CryptGetHashParam',[h,2n,output,len,0n]),output?0n:1n);equal(vm.readInt(len),20n);if(output)equal(vm.readInt(t.error),234n);equal(bytes(vm,result,20),new Uint8Array(20).fill(0xa5));
  failure(t,'CryptHashData',[h,data,1n,0n],0x8009000c);equal(value(t,h,20),expected('sha1',new Uint8Array()));equal(api(t,'CryptDestroyHash',[h]),1n);
 }
 for(const name of names){assert.ok(vm.bind('advapi32.dll',name));assert.throws(()=>vm.bind('user32.dll',name),/unknown/);}
 for(const [kind,code]of [[0n,0x80090014],[2n,0x80090017]])failure(t,'CryptAcquireContextW',[out,0n,0n,kind,0xf0000000n],code);
 for(const flags of [0n,0x40n])failure(t,'CryptAcquireContextW',[out,0n,0n,24n,flags],0x80090016);
 for(const flags of [1n,0x10000000n,0xf0000080n])failure(t,'CryptAcquireContextW',[out,0n,0n,24n,flags],0x80090009);
 vm.write(0x23000n,new TextEncoder().encode('Unknown\0'));failure(t,'CryptAcquireContextA',[out,0n,0x23000n,24n,0xf0000000n],0x80090013);
 vm.write(0x23000n,Uint8Array.from([88,0,0,0]));failure(t,'CryptAcquireContextW',[out,0x23000n,0n,24n,0xf0000000n],0x80090016);
 failure(t,'CryptCreateHash',[provider,0x8002n,0n,0n,out],0x80090008);failure(t,'CryptCreateHash',[provider,0x8004n,1n,0n,out],0x80090003);failure(t,'CryptCreateHash',[provider,0x8004n,0n,1n,out],0x80090009);
 const live=hash(t,provider);failure(t,'CryptHashData',[live,0n,1n,0n],87);failure(t,'CryptHashData',[live,data,1n,2n],0x80090009);failure(t,'CryptGetHashParam',[live,99n,result,len,0n],0x8009000a);failure(t,'CryptGetHashParam',[live,1n,result,0n,0n],87);failure(t,'CryptGetHashParam',[live,1n,result,len,1n],0x80090009);failure(t,'CryptDuplicateHash',[live,1n,0n,out],87);
 failure(t,'CryptContextAddRef',[provider,1n,0n],87);failure(t,'CryptContextAddRef',[provider,0n,1n],0x80090009);failure(t,'CryptReleaseContext',[provider,1n],0x80090009);
 if(mode===64){failure(t,'CryptHashData',[live|(1n<<48n),data,1n,0n],0x80090002);equal(api(t,'CryptHashData',[live,data,(1n<<32n)|1n,1n<<32n]),1n);const high=0x3000020000n;vm.map(high,4096n,3);vm.write(high,encode(20n,4));equal(api(t,'CryptGetHashParam',[live,(1n<<32n)|2n,high+16n,high,1n<<32n]),1n);equal(bytes(vm,high+16n,20),expected('sha1',new TextEncoder().encode('p')));}
 equal(api(t,'CryptContextAddRef',[provider,0n,0n]),1n);equal(api(t,'CryptReleaseContext',[provider,0n]),1n);equal(api(t,'CryptDestroyHash',[live]),1n);
 const revoked=hash(t,provider),revokedCopy=duplicate(t,revoked);equal(api(t,'CryptReleaseContext',[provider,0n]),1n);
 for(const h of [revoked,revokedCopy])failure(t,'CryptDestroyHash',[h],0x80090002);failure(t,'CryptReleaseContext',[provider,0n],0x80090001);equal(api(t,'CryptReleaseContext',[full,0n]),1n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Equal opaque values in different VMs retain separate providers, streams and release state.
for(const mode of [32,64]){
 const a=await machine(mode),b=await machine(mode),pa=acquire(a),pb=acquire(b),ha=hash(a,pa),hb=hash(b,pb);equal(pa,pb);equal(ha,hb);
 for(const[t,h,n]of [[a,ha,65],[b,hb,66]]){t.vm.write(data,[n]);equal(api(t,'CryptHashData',[h,data,1n,0n]),1n);equal(value(t,h,20),expected('sha1',Uint8Array.of(n)));}
 equal(api(a,'CryptReleaseContext',[pa,0n]),1n);equal(value(b,hb,20),expected('sha1',Uint8Array.of(66)));equal(api(b,'CryptReleaseContext',[pb,0n]),1n);a.vm.dispose();b.vm.dispose();
}

// Source/output/error faults must not partially append, finalize, allocate or publish.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,p=acquire(t),h=hash(t,p),input=pattern(32);vm.write(0x24ff8n,input);
 frame(t,'CryptHashData',[h,0x24ff8n,32n,0n],'advapi32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,1),0);let before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(value(t,h,20),expected('sha1',input));equal(api(t,'CryptDestroyHash',[h]),1n);
 const a=hash(t,p);vm.write(len,encode(20n,4));vm.write(0x24ff8n,new Uint8Array(20).fill(0xa5));frame(t,'CryptGetHashParam',[a,2n,0x24ff8n,len,0n],'advapi32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(len),20n);equal(vm.readInt(0x24ff8n,8),0xa5a5a5a5a5a5a5a5n);
 vm.write(data,[65]);equal(api(t,'CryptHashData',[a,data,1n,0n]),1n);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(value(t,a,20),expected('sha1',Uint8Array.of(65)));equal(api(t,'CryptDestroyHash',[a]),1n);
 const b=hash(t,p);vm.write(len,encode(0n,4));frame(t,'CryptGetHashParam',[b,2n,result,len,0n],'advapi32.dll');equal(core.wire_protect(t.handle,t.image.teb,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(len),0n);equal(api(t,'CryptHashData',[b,data,1n,0n]),1n);equal(core.wire_protect(t.handle,t.image.teb,4096n,4),0);failure(t,'CryptGetHashParam',[b,2n,result,len,0n],234);equal(value(t,b,20),expected('sha1',Uint8Array.of(65)));equal(api(t,'CryptDestroyHash',[b]),1n);
 const last=hash(t,p);frame(t,'CryptCreateHash',[p,0x8004n,0n,0n,0x24ffen],'advapi32.dll');equal(core.wire_protect(t.handle,0x25000n,4096n,2),0);before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(core.wire_protect(t.handle,0x25000n,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);const next=vm.readInt(0x24ffen,mode/8);equal(next,last+1n);equal(api(t,'CryptDestroyHash',[next]),1n);
 equal(api(t,'CryptCreateHash',[p,0x8004n,0n,0n,t.sp]),1n);const alias=vm.readInt(t.sp,mode/8);equal(vm.ip,vm.entryPoints.returned);equal(api(t,'CryptDestroyHash',[alias]),1n);equal(api(t,'CryptReleaseContext',[p,0n]),1n);vm.dispose();
}

// Quota failures preserve handle outputs; final provider release reclaims its hashes.
for(const mode of [32,64]){
 const t=await machine(mode),p=acquire(t),handles=[];
 for(let n=0;n<1023;n++)handles.push(hash(t,p));t.vm.write(out,encode(0xabcdefn,mode/8));failure(t,'CryptCreateHash',[p,0x8004n,0n,0n,out],8);equal(t.vm.readInt(out,mode/8),0xabcdefn);
 equal(api(t,'CryptDestroyHash',[handles.pop()]),1n);const replacement=hash(t,p);assert.ok(replacement>handles.at(-1));equal(api(t,'CryptReleaseContext',[p,0n]),1n);
 const fresh=acquire(t);assert.ok(fresh>replacement);failure(t,'CryptHashData',[handles[0],0n,0n,0n],0x80090002);equal(api(t,'CryptReleaseContext',[fresh,0n]),1n);t.vm.dispose();
}

// Bind every import in one PE and execute the five-argument acquisition through the JIT.
for(const mode of [32,64]){
 const f=fixture(mode,{tls:false});f.bytes.fill(0,f.offset(0x2000),f.offset(0x2600));f.rput(0x2000,0x2080);f.rput(0x200c,0x2260);f.rput(0x2010,0x2180);f.rtext(0x2260,'advapi32.dll\0');let at=0x2300;
 for(const[i,name]of names.entries()){f.rput(0x2080+i*f.ptr,at,f.ptr);f.rput(0x2180+i*f.ptr,at,f.ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
 if(mode===32){f.bytes.set([0x68,...encode(0xf0000000n,4),0x6a,24,0x6a,0,0x6a,0,0x68,...encode(f.preferred+0x3000n,4),0xff,0x15,...encode(f.preferred+0x2184n,4),0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x100c,0x1012);}
 else{f.bytes.set([0x48,0x83,0xec,40,0x48,0xb9,...encode(f.preferred+0x3000n,8),0x31,0xd2,0x45,0x31,0xc0,0x41,0xb9,24,0,0,0,0xc7,0x44,0x24,32,...encode(0xf0000000n,4),0xff,0x15,...encode(0x1161n,4),0x48,0x83,0xc4,40,0xc3],f.offset(0x1000));f.relocations.splice(0,f.relocations.length,0x1006);}
 f.rebuildRelocations();const vm=new WasmMachine(core,{mode}),image=vm.load(f.bytes);for(const[i,name]of names.entries())equal(vm.readInt(image.base+0x2180n+BigInt(i*f.ptr),f.ptr),vm.bind('advapi32.dll',name));equal((await vm.run(100n)).reason,Exit.Returned);equal(vm.register(0),1n);assert.ok(vm.readInt(image.base+0x3000n,mode/8));assert.ok(vm.retired>0n);vm.dispose();
}
console.log(`${checks} CryptoAPI digest/stream/duplicate/lifetime/ABI/fault/quota/JIT checks passed`);
