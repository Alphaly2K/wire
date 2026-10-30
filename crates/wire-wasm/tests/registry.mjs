import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fp} from './x87-harness.mjs';

const HKCU=0x80000001n, out=0x23000n, type=0x24000n, size=0x24010n, data=0x26000n;
const api=(t,name,args=[])=>call(t,name,args,'advapi32.dll');
const bytes=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const wide=s=>Array.from(Buffer.from(s,'utf16le'));
function text(t,s,w=true,p=0x20000n){t.vm.write(p,w?wide(s+'\0'):new TextEncoder().encode(s+'\0'));return p;}
function create(t,s='Software\\Wire',w=true,access=0xf003fn){
 equal(api(t,'RegCreateKeyEx'+(w?'W':'A'),[HKCU,text(t,s,w),0n,0n,0n,access,0n,out,type]),0n);
 return t.vm.readInt(out,t.mode/8);
}
function query(t,key,w=true,{name=0n,output=data,capacity=64n,kind=type,length=size}={}){
 if(length)t.vm.write(length,encode(capacity,4));
 return api(t,'RegQueryValueEx'+(w?'W':'A'),[key,name,0n,kind,output,length]);
}

// Both ABIs expose all registry exports, independently of legacy provider aliases.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t;
 for(const name of ['RegCreateKeyA','RegCreateKeyW','RegCreateKeyExA','RegCreateKeyExW','RegOpenKeyA','RegOpenKeyW','RegOpenKeyExA','RegOpenKeyExW','RegQueryValueA','RegQueryValueW','RegQueryValueExA','RegQueryValueExW','RegSetValueExA','RegSetValueExW','RegDeleteValueA','RegDeleteValueW','RegCloseKey']){
  assert.ok(vm.bind('advapi32.dll',name));
  for(const dll of ['kernel32.dll','kernelbase.dll','user32.dll'])assert.throws(()=>vm.bind(dll,name),/unknown/);
 }
 const module=call(t,'GetModuleHandleW',[text(t,'advapi32.dll')],'kernel32.dll');assert.ok(module);
 for(const name of ['RegQueryValueA','RegQueryValueW','RegCreateKeyExW'])equal(call(t,'GetProcAddress',[module,text(t,name,false)],'kernel32.dll'),vm.bind('advapi32.dll',name));
 vm.write(t.error,encode(123n,4));vm.flags=0x8d5n;vm.mxcsr=0x1fa0;vm.xmm(7,new Uint8Array(16).fill(0xa5));const fpu=fp(t);
 const first=create(t);equal(vm.readInt(type),1n);equal(first,mode===32?0xa0010000n:0x7ffd04000000n);
 const again=create(t,'software\\WIRE',false);equal(vm.readInt(type),2n);equal(again,first+16n);
 for(const w of [false,true]){
  const suffix=w?'W':'A';
  equal(api(t,'RegCreateKey'+suffix,[first,text(t,'child'+suffix,w),out]),0n);
  const child=vm.readInt(out,mode/8);
  equal(api(t,'RegOpenKey'+suffix,[first,text(t,'CHILD'+suffix,w),out]),0n);assert.notEqual(vm.readInt(out,mode/8),child);
  equal(api(t,'RegOpenKeyEx'+suffix,[first,text(t,'CHILD'+suffix,w),0n,1n,out]),0n);
  const read=vm.readInt(out,mode/8);vm.write(data,[1,2,3,4]);
  equal(api(t,'RegSetValueEx'+suffix,[read,0n,0n,3n,data,4n]),5n);
  equal(api(t,'RegSetValueEx'+suffix,[child,0n,0n,3n,data,4n]),0n);
  equal(query(t,read,w),0n);equal(bytes(vm,data,4),new Uint8Array([1,2,3,4]));
  equal(api(t,'RegDeleteValue'+suffix,[read,0n]),5n);equal(api(t,'RegDeleteValue'+suffix,[child,0n]),0n);equal(api(t,'RegDeleteValue'+suffix,[child,0n]),2n);
  equal(api(t,'RegCloseKey',[child]),0n);equal(api(t,'RegCloseKey',[child]),6n);equal(api(t,'RegCloseKey',[read]),0n);
 }
 for(const root of [0n,1n,2n,3n,5n])for(const sign of mode===64?[0n,0xffffffff00000000n]:[0n]){
  const h=sign|0x80000000n|root;
  equal(api(t,'RegOpenKeyW',[h,0n,out]),0n);equal(vm.readInt(out,mode/8),h);
  equal(api(t,'RegOpenKeyExA',[h,0n,0n,0n,out]),0n);equal(vm.readInt(out,mode/8),h);equal(api(t,'RegCloseKey',[h]),0n);
 }
 for(const h of [0n,0x80000004n,0x80000006n,first+1n,...(mode===64?[first&0xffffffffn,0x1234000080000001n]:[])]){
  vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,'RegOpenKeyW',[h,0n,out]),6n);equal(vm.readInt(out,mode/8),0n);equal(api(t,'RegCloseKey',[h]),6n);
 }
 equal(vm.flags,0x8d5n);equal(vm.mxcsr,0x1fa0);equal(vm.xmm(7),new Uint8Array(16).fill(0xa5));equal(fp(t),fpu);equal(vm.readInt(t.error),123n);equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// A/W conversions count bytes, retain embedded NULs and preserve arbitrary wide data.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,key=create(t);
 const cases=[
  {w:false,kind:1,source:[0x83,0x5c,0,0x83,0x74,0],a:[0x83,0x5c,0,0x83,0x74,0],wdata:wide('ソ\0フ\0')},
  {w:true,kind:2,source:wide('A\0B\0'),a:[65,0,66,0],wdata:wide('A\0B\0')},
  {w:false,kind:7,source:[65,0,66,0,0],a:[65,0,66,0,0],wdata:wide('A\0B\0\0')},
  {w:true,kind:1,source:[0,0xd8,0],a:[63],wdata:[0,0xd8,0]},
  {w:true,kind:3,source:[0,0xff,7],a:[0,0xff,7],wdata:[0,0xff,7]},
  {w:true,kind:0x1234,source:[],a:[],wdata:[]},
 ];
 for(const c of cases){
  vm.write(data,[...c.source,0x7f,0x7f]);equal(api(t,'RegSetValueEx'+(c.w?'W':'A'),[key,0n,0n,BigInt(c.kind),data,BigInt(c.source.length)]),0n);
  for(const w of [false,true]){
   const expected=c[w?'wdata':'a'];vm.write(data,new Uint8Array(64).fill(0xa5));
   equal(query(t,key,w,{capacity:BigInt(expected.length)}),0n);equal(vm.readInt(type),BigInt(c.kind));equal(vm.readInt(size),BigInt(expected.length));equal(bytes(vm,data,expected.length+1),Uint8Array.from([...expected,0xa5]));
   equal(query(t,key,w,{output:0n,length:0n,kind:0n}),0n);
   equal(query(t,key,w,{output:0n,capacity:0xfffffff0n}),0n);equal(vm.readInt(size),BigInt(expected.length));
   if(expected.length){vm.write(data,new Uint8Array(64).fill(0xa5));equal(query(t,key,w,{capacity:BigInt(expected.length-1)}),234n);equal(vm.readInt(size),BigInt(expected.length));equal(bytes(vm,data,64),new Uint8Array(64).fill(0xa5));}
  }
 }
 // An adjacent uncounted terminator is included at set time; query can append one without increasing the reported size.
 for(const w of [false,true]){
  const suffix=w?'W':'A',width=w?2:1;vm.write(data,w?wide('Hi\0'):new Uint8Array([72,105,0]));
  equal(api(t,'RegSetValueEx'+suffix,[key,0n,0n,1n,data,BigInt(2*width)]),0n);equal(query(t,key,w,{output:0n}),0n);equal(vm.readInt(size),BigInt(3*width));
  vm.write(data,w?wide('HiX'):new Uint8Array([72,105,88]));equal(api(t,'RegSetValueEx'+suffix,[key,0n,0n,1n,data,BigInt(2*width)]),0n);
  vm.write(data,new Uint8Array(16).fill(0xa5));equal(query(t,key,w,{capacity:BigInt(3*width)}),0n);equal(vm.readInt(size),BigInt(2*width));equal(bytes(vm,data,3*width),Uint8Array.from(w?wide('Hi\0'):[72,105,0]));
 }
 vm.write(0x21000n,[0x83,0x5c,0]);equal(api(t,'RegSetValueExA',[key,0x21000n,0n,4n,data,0n]),0n);
 equal(query(t,key,true,{name:text(t,'ソ',true,0x21000n)}),0n);
 equal(api(t,'RegDeleteValueW',[key,0x21000n]),0n);equal(query(t,key,true,{name:0x21000n,output:0n}),2n);equal(vm.readInt(size),0n);
 vm.dispose();
}

// The legacy query addresses a subkey's unnamed value and supplies an empty default.
for(const mode of [32,64])for(const w of [false,true]){
 const t=await machine(mode),{vm}=t,key=create(t,'Software\\Empty',w),suffix=w?'W':'A',width=w?2:1;
 vm.write(data,new Uint8Array(8).fill(0xa5));vm.write(size,encode(0n,4));
 equal(api(t,'RegQueryValue'+suffix,[key,0n,data,size]),0n);equal(vm.readInt(size),BigInt(width));equal(bytes(vm,data,width+1),Uint8Array.from([...new Uint8Array(width),0xa5]));
 equal(query(t,key,w,{output:0n}),2n);equal(vm.readInt(size),0n);
 vm.write(data,[1,2,3,4]);equal(api(t,'RegSetValueExW',[key,0n,0n,3n,data,4n]),0n);
 vm.write(size,encode(4n,4));equal(api(t,'RegQueryValue'+suffix,[HKCU,text(t,'Software\\Empty',w),data,size]),0n);equal(vm.readInt(size),4n);equal(vm.readInt(data),0x04030201n);
 vm.write(size,encode(1n,4));equal(api(t,'RegQueryValue'+suffix,[key,0n,data,size]),234n);equal(vm.readInt(size),4n);
 equal(api(t,'RegQueryValue'+suffix,[key,0n,data,0n]),87n);
 equal(api(t,'RegQueryValue'+suffix,[HKCU,text(t,'absent',w),data,size]),2n);vm.dispose();
}

// Parameter errors retain create outputs; failed opens clear only their handle output.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,key=create(t),path=text(t,'child');
 const args=[key,path,0n,0n,0n,0xf003fn,0n,out,type];
 for(const [index,value,error] of [[2,1n,87n],[3,1n,50n],[4,2n,50n],[4,3n,50n],[4,16n,87n],[5,0x300n,87n],[5,0x201n,50n],[5,0x4000n,87n],[6,1n,50n],[7,0n,1010n]]){
  vm.write(out,new Uint8Array(8).fill(0xa5));vm.write(type,encode(123n,4));const a=args.slice();a[index]=value;
  equal(api(t,'RegCreateKeyExW',a),error);equal(vm.readInt(out,mode/8),BigInt.asUintN(mode,0xa5a5a5a5a5a5a5a5n));equal(vm.readInt(type),123n);
 }
 for(const s of ['missing','\\invalid','invalid\\','invalid\\\\child']){
  vm.write(out,new Uint8Array(8).fill(0xa5));equal(api(t,'RegOpenKeyExW',[key,text(t,s),0n,1n,out]),s==='missing'?2n:87n);equal(vm.readInt(out,mode/8),0n);
 }
 for(const access of [0x80000000n,0x20000000n,0x40000000n,0x10000000n,0x02000000n,0x101n]){
  equal(api(t,'RegOpenKeyExW',[HKCU,text(t,'Software\\Wire'),0n,access,out]),0n);const h=vm.readInt(out,mode/8);
  equal(query(t,h,true,{output:0n}),access===0x40000000n?5n:2n);equal(api(t,'RegCloseKey',[h]),0n);
 }
 equal(api(t,'RegCreateKeyW',[0x80000002n,text(t,'new'),out]),5n);
 equal(api(t,'RegSetValueExW',[HKCU,0n,0n,3n,0n,1n]),998n);
 equal(api(t,'RegSetValueExW',[HKCU,0n,0n,1n,0n,0n]),0n);equal(query(t,HKCU,true,{output:0n}),0n);equal(vm.readInt(size),0n);
 equal(api(t,'RegSetValueExW',[HKCU,0n,1n,3n,data,0n]),87n);
 equal(api(t,'RegSetValueExW',[HKCU,0n,0n,3n,data,0x100001n]),87n);
 equal(api(t,'RegQueryValueExW',[HKCU,0n,1n,type,0n,size]),87n);
 equal(api(t,'RegOpenKeyExW',[key,text(t,'child'),8n,1n,out]),50n);equal(vm.readInt(out,mode/8),0n);
 equal(api(t,'RegOpenKeyW',[0x80000000n,text(t,'\\'),out]),0n);const classes=vm.readInt(out,mode/8);assert.notEqual(classes,0x80000000n);equal(api(t,'RegCloseKey',[classes]),0n);
 equal(api(t,'RegCreateKeyW',[0x80000000n,text(t,'\\'),out]),87n);
 vm.dispose();
}

// Copyout, input and return-frame faults never publish a key, consume a handle or replace a value.
for(const mode of [32,64])for(const kind of ['handle','disposition','name','return','guard']){
 const t=await machine(mode),{vm}=t,path=text(t,'Software\\Atomic');
 let target=out,disposition=type,page=0x25000n,protection=2;
 if(kind==='handle')target=0x24ffen;
 if(kind==='disposition'||kind==='guard')disposition=0x25000n;
 if(kind==='name'){page=0x20000n;protection=1;}
 if(kind==='return'){page=t.sp&~4095n;protection=1;}
 if(kind==='guard')protection=0x104;
 vm.write(out,new Uint8Array(8).fill(0xa5));vm.write(type,encode(123n,4));
 frame(t,'RegCreateKeyExW',[HKCU,path,0n,0n,0n,0xf003fn,0n,target,disposition],'advapi32.dll');equal(core.wire_protect(t.handle,page,4096n,protection),0);
 const before=snapshot(t),work=vm.work,retired=vm.retired;equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.work,work);equal(vm.retired,retired);equal(vm.readInt(out,mode/8),BigInt.asUintN(mode,0xa5a5a5a5a5a5a5a5n));equal(vm.readInt(type),123n);
 equal(core.wire_protect(t.handle,page,4096n,4),0);
 equal(api(t,'RegOpenKeyW',[HKCU,text(t,'Software'),out]),2n);
 equal(api(t,'RegCreateKeyExW',[HKCU,text(t,'Software\\Atomic'),0n,0n,0n,0xf003fn,0n,target,disposition]),0n);
 equal(vm.readInt(target,mode/8),mode===32?0xa0010000n:0x7ffd04000000n);equal(vm.readInt(disposition),1n);vm.dispose();
}

for(const mode of [32,64])for(const kind of ['type','size','payload','source','lookahead']){
 const t=await machine(mode),{vm}=t,key=create(t);vm.write(data,[1,2,3,4]);equal(api(t,'RegSetValueExW',[key,0n,0n,3n,data,4n]),0n);
 vm.write(type,encode(123n,4));vm.write(size,encode(4n,4));vm.write(data,new Uint8Array(8).fill(0xa5));
 let page=0x25000n,protection=2;
 if(kind==='type'||kind==='size'){
  const p=kind==='type'?0x25000n:type,s=kind==='size'?0x25000n:size;vm.write(s,encode(4n,4));
  frame(t,'RegQueryValueExW',[key,0n,0n,p,data,s],'advapi32.dll');
 }else if(kind==='payload')frame(t,'RegQueryValueExW',[key,0n,0n,type,0x24ffen,size],'advapi32.dll');
 else{
  const source=kind==='source'?0x24ffen:0x24ffcn;vm.write(source,[65,0,66,0]);protection=1;
  frame(t,'RegSetValueExW',[key,0n,0n,kind==='source'?3n:1n,source,4n],'advapi32.dll');
 }
 equal(core.wire_protect(t.handle,page,4096n,protection),0);const before=snapshot(t);equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(vm.readInt(type),123n);equal(vm.readInt(size),4n);equal(bytes(vm,data,8),new Uint8Array(8).fill(0xa5));
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(query(t,key),0n);equal(vm.readInt(data),0x04030201n);vm.dispose();
}

// Aliased outputs use captured inputs/returns; high guest pointers and handles stay full-width.
for(const mode of [32,64]){
 const t=await machine(mode),{vm}=t,key=create(t);vm.write(data,[0x12,0x34,0x56,0x78]);equal(api(t,'RegSetValueExW',[key,0n,0n,3n,data,4n]),0n);
 equal(query(t,key,true,{kind:size,output:size,length:size,capacity:4n}),0n);equal(vm.readInt(size),0x78563412n);
 equal(query(t,key,true,{kind:size,output:size,length:size,capacity:1n}),234n);equal(vm.readInt(size),4n);
 equal(query(t,key,true,{output:t.sp}),0n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp),0x78563412n);
 equal(api(t,'RegCreateKeyW',[HKCU,text(t,'Software\\Alias'),t.sp]),0n);equal(vm.ip,vm.entryPoints.returned);
 if(mode===64){
  const high=0x123400000000n;vm.map(high,8192n,7);text(t,'Software\\High',true,high);
  equal(api(t,'RegCreateKeyExW',[HKCU,high,0xabcdef0000000000n,0n,0xabcdef0000000000n,0xabcdef00000f003fn,0n,high+4096n,0n]),0n);
  const h=vm.readInt(high+4096n,8);assert.ok(h>0xffffffffn);equal(api(t,'RegCloseKey',[h&0xffffffffn]),6n);equal(api(t,'RegCloseKey',[h]),0n);
 }
 vm.dispose();
}

// VM ownership and finite handle identities prevent stale resources from crossing lifetimes.
for(const mode of [32,64]){
 const a=await machine(mode),b=await machine(mode),key=create(a);equal(api(b,'RegCloseKey',[key]),6n);equal(api(b,'RegOpenKeyW',[HKCU,text(b,'Software\\Wire'),out]),2n);a.vm.dispose();b.vm.dispose();
 const t=await machine(mode),{vm}=t;
 for(let n=0;n<4096;n++){
  equal(api(t,'RegCreateKeyW',[HKCU,0n,out]),0n);const h=vm.readInt(out,mode/8);equal(h,(mode===32?0xa0010000n:0x7ffd04000000n)+BigInt(n)*16n);equal(api(t,'RegCloseKey',[h]),0n);
 }
 vm.write(out,encode(0xfeedn,mode/8));equal(api(t,'RegCreateKeyW',[HKCU,0n,out]),8n);equal(vm.readInt(out,mode/8),0xfeedn);equal(api(t,'RegOpenKeyW',[HKCU,0n,out]),0n);equal(vm.readInt(out,mode/8),HKCU);vm.dispose();
}

console.log(`${checks} registry ABI/A-W/rights/ownership/capacity/alias/fault checks passed`);
