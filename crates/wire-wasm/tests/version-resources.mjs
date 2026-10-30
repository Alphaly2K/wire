import assert from 'node:assert/strict';
import {core,encode,Exit,equal,checks,snapshot,machine,frame,call} from './gdi-harness.mjs';
import {fixture} from './pe-fixture.mjs';

const wide=text=>Array.from({length:text.length},(_,i)=>[text.charCodeAt(i)&255,text.charCodeAt(i)>>8]).flat();
const api=(t,name,args)=>call(t,name,args,'version.dll');
const data=(vm,p,n)=>Uint8Array.from({length:n},(_,i)=>Number(vm.readInt(p+BigInt(i),1)));
const block=(key,text,value,children=[])=>{
 const bytes=[0,0,...encode(BigInt(value.length/(text?2:1)),2),...encode(text?1n:0n,2),...wide(key+'\0')];
 while(bytes.length%4)bytes.push(0);
 bytes.push(...value);
 for(const child of children){while(bytes.length%4)bytes.push(0);bytes.push(...child);}
 bytes.splice(0,2,...encode(BigInt(bytes.length),2));return bytes;
};
const fixed=[0xfeef04bdn,0x10000n,0x10002n,0x30004n,0x50006n,0x70008n,0x3fn,0n,0x40004n,1n,0n,0n,0n].flatMap(n=>Array.from(encode(n,4)));
const blob=block('VS_VERSION_INFO',false,fixed,[
 block('StringFileInfo',true,[],[block('041103a4',true,[],[
  block('CompanyName',true,wide('ソフト\0')),
  block('FileDescription',true,wide('Runtime\0')),
  block('Empty',true,[]),
 ])]),
 block('VarFileInfo',true,[],[block('Translation',false,[0x11,4,0xa4,3])]),
]);
function image(mode,{payload=blob,padding=0}={}){
 const f=fixture(mode,{tls:false}),rawSize=Math.ceil((payload.length+padding)/512)*512,size=Math.ceil(rawSize/4096)*4096;
 f.put(f.dirs+16,0x2500);f.put(f.dirs+20,0x60);f.put(f.dirs+40,0);f.put(f.dirs+44,0);
 f.put(f.opt+56,0x4000+size);f.put(f.sections+3*40+8,size);f.put(f.sections+3*40+16,rawSize);
 for(const at of [0x2500,0x2518,0x2530])f.rput(at+14,1,2);
 for(const [at,value] of [[0x2510,16],[0x2514,0x80000018],[0x2528,1],[0x252c,0x80000030],[0x2540,0x411],[0x2544,0x48],[0x2548,0x4000],[0x254c,payload.length+padding]])f.rput(at,value);
 const bytes=new Uint8Array(0x1000+rawSize);bytes.set(f.bytes.subarray(0,0x1000));bytes.set(payload,0x1000);
 return {bytes,offset:f.offset,put:f.put,fixture:f};
}
function filename(t,text='C:\\versions\\sample.dll',ansi=false){t.vm.write(0x20000n,ansi?new TextEncoder().encode(text+'\0'):wide(text+'\0'));return 0x20000n;}
function query(t,text='\\',ansi=false,base=0x26000n,pointer=0x22000n,length=0x22010n){
 const path=text===null?0n:0x21000n;if(path)t.vm.write(path,ansi?new TextEncoder().encode(text+'\0'):wide(text+'\0'));
 return api(t,ansi?'VerQueryValueA':'VerQueryValueW',[base,path,pointer,length]);
}
async function setup(mode,fileMode=mode,options={}){
 const t=await machine(mode),f=image(fileMode,options);t.vm.supplyFile('C:\\versions\\sample.dll',f.bytes);return {...t,f};
}

// Both guest ABIs read either PE format, preserve raw resource size and expose caller-owned pointers.
for(const mode of [32,64])for(const fileMode of [32,64])for(const ansi of [false,true]){
 const t=await setup(mode,fileMode,{padding:16}),{vm}=t,suffix=ansi?'A':'W',name=filename(t,undefined,ansi),expected=(blob.length+16)*2+4;
 vm.write(t.error,encode(123n,4));vm.write(0x22000n,new Uint8Array(8).fill(0xa5));
 equal(api(t,'GetFileVersionInfoSize'+suffix,[name,0x22000n]),BigInt(expected));equal(vm.readInt(0x22000n),0n);equal(vm.readInt(0x22004n),0xa5a5a5a5n);equal(vm.readInt(t.error),0n);
 vm.write(0x26000n,new Uint8Array(expected+16).fill(0xa5));
 equal(api(t,'GetFileVersionInfo'+suffix,[name,0xabcdef0012345678n,BigInt(expected),0x26000n]),1n);
 equal(data(vm,0x26000n,blob.length),Uint8Array.from(blob));equal(data(vm,0x26000n+BigInt(blob.length),4),new TextEncoder().encode('FE2X'));
 equal(data(vm,0x26000n+BigInt(blob.length+16),expected-blob.length-16+16),new Uint8Array(expected-blob.length).fill(0xa5));
 for(const path of ['\\','','\\\\',null]){
  vm.write(t.error,encode(123n,4));equal(query(t,path,ansi),1n);equal(vm.readInt(0x22000n,mode/8),0x26028n);equal(vm.readInt(0x22010n),52n);equal(vm.readInt(t.error),123n);equal(vm.readInt(0x26028n),0xfeef04bdn);
 }
 equal(query(t,'\\varfileinfo\\TRANSLATION',ansi),1n);equal(vm.readInt(vm.readInt(0x22000n,mode/8)),0x03a40411n);equal(vm.readInt(0x22010n),4n);
 equal(query(t,'\\StringFileInfo\\041103a4\\CompanyName',false),1n);const widePointer=vm.readInt(0x22000n,mode/8);equal(vm.readInt(0x22010n),4n);equal(data(vm,widePointer,8),Uint8Array.from(wide('ソフト\0')));
 equal(query(t,'\\StringFileInfo\\041103a4\\CompanyName',true),1n);const narrowPointer=vm.readInt(0x22000n,mode/8);equal(narrowPointer,widePointer+BigInt(blob.length+4));equal(vm.readInt(0x22010n),7n);equal(data(vm,narrowPointer,7),Uint8Array.from([0x83,0x5c,0x83,0x74,0x83,0x67,0]));equal(data(vm,widePointer,8),Uint8Array.from(wide('ソフト\0')));
 equal(query(t,'\\StringFileInfo\\041103a4\\Empty',ansi),1n);equal(vm.readInt(0x22010n),0n);
 const pointer=vm.readInt(0x22000n,mode/8);vm.write(0x22010n,encode(0x12345678n,4));equal(query(t,'\\missing',ansi),0n);equal(vm.readInt(0x22010n),0n);equal(vm.readInt(0x22000n,mode/8),pointer);equal(vm.readInt(t.error),1813n);
 equal(query(t,'\\',ansi,0x26000n,0x22000n,0n),1n);
 equal(t.time.calls,0);equal(t.time.wallCalls,0);vm.dispose();
}

// Small buffers copy only their declared prefix; scratch bytes are not gratuitously cleared.
for(const mode of [32,64])for(const capacity of [0,1,7,blob.length-1,blob.length,blob.length+3,blob.length+4]){
 const t=await setup(mode),{vm}=t,name=filename(t),output=0x24000n;vm.write(output,new Uint8Array(blob.length+16).fill(0xa5));
 equal(api(t,'GetFileVersionInfoW',[name,0n,BigInt(capacity),output]),1n);
 const expected=new Uint8Array(blob.length+16).fill(0xa5);expected.set(blob.slice(0,capacity));if(capacity>=blob.length+4)expected.set(new TextEncoder().encode('FE2X'),blob.length);
 equal(data(vm,output,expected.length),expected);equal(vm.readInt(t.error),0n);vm.dispose();
}

// Null, missing, malformed and inaccessible VFS inputs have stable errors and handle outputs.
for(const mode of [32,64]){
 const t=await setup(mode),{vm}=t;
 for(const [path,error] of [['',161n],['C:\\versions\\missing.dll',2n],['C:\\absent\\file.dll',3n],['D:\\file.dll',15n]]){
  vm.write(0x22000n,encode(123n,4));equal(api(t,'GetFileVersionInfoSizeW',[filename(t,path),0x22000n]),0n);equal(vm.readInt(t.error),error);equal(vm.readInt(0x22000n),0n);
 }
 equal(api(t,'GetFileVersionInfoSizeW',[0n,0x22000n]),0n);equal(vm.readInt(t.error),87n);
 equal(api(t,'GetFileVersionInfoW',[filename(t),0n,100n,0n]),0n);equal(vm.readInt(t.error),13n);
 vm.supplyFile('C:\\versions\\invalid.dll',[1,2,3]);equal(api(t,'GetFileVersionInfoSizeW',[filename(t,'C:\\versions\\invalid.dll'),0n]),0n);equal(vm.readInt(t.error),1812n);
 const f=image(mode);f.bytes[f.offset(0x2510)]=15;vm.supplyFile('C:\\versions\\no-version.dll',f.bytes);equal(api(t,'GetFileVersionInfoSizeW',[filename(t,'C:\\versions\\no-version.dll'),0n]),0n);equal(vm.readInt(t.error),1813n);
 vm.write(0x26000n,[6,0,52,0,0,0]);equal(query(t),0n);equal(vm.readInt(t.error),13n);
 vm.write(t.error,encode(123n,4));vm.write(0x22010n,encode(99n,4));equal(query(t,'\\',false,0n),0n);equal(vm.readInt(t.error),123n);equal(vm.readInt(0x22010n),99n);
 for(const name of ['GetFileVersionInfoA','GetFileVersionInfoW','GetFileVersionInfoSizeA','GetFileVersionInfoSizeW','VerQueryValueA','VerQueryValueW']){assert.ok(vm.bind('version.dll',name));assert.throws(()=>vm.bind('kernel32.dll',name),/unknown/);}
 vm.dispose();
}

// Neutral/thread/user language selection is independent of on-disk entry order.
for(const mode of [32,64])for(const neutral of [false,true])for(const lcid of [0x409n,0x411n,0x7fn]){
 const t=await machine(mode),{vm}=t,languages=neutral?[0,0x409,0x411]:[0x409,0x411];
 const payload=languages.flatMap(language=>{const copy=blob.slice();copy.splice(48,4,...encode(BigInt(language),4));return copy;});
 const f=image(mode,{payload}),v=new DataView(f.bytes.buffer),put=(rva,value,size=4)=>{const at=f.offset(rva);for(let n=0;n<size;n++)v.setUint8(at+n,Number(BigInt(value)>>BigInt(n*8)&255n));};
 put(0x253e,languages.length,2);put(0x253c,0,2);
 languages.forEach((language,index)=>{put(0x2540+index*8,language);put(0x2544+index*8,0x2558-0x2500+index*16);put(0x2558+index*16,0x4000+index*blob.length);put(0x255c+index*16,blob.length);});
 v.setUint32(f.fixture.dirs+20,0x90,true);vm.supplyFile('C:\\versions\\sample.dll',f.bytes);
 equal(call(t,'SetThreadLocale',[lcid],'kernel32.dll'),1n);
 equal(api(t,'GetFileVersionInfoW',[filename(t),0n,BigInt(blob.length*2+4),0x26000n]),1n);equal(vm.readInt(0x26030n),neutral?0n:lcid===0x409n?0x409n:0x411n);vm.dispose();
}

// Resource directories/data remain bounded; files without resource ID 1 are rejected.
for(const mode of [32,64])for(const [at,value] of [[0x2514,0x7fffffffn],[0x2528,2n],[0x252c,0xffffffffn],[0x2544,0x80000048n],[0x2548,0xfffffff0n],[0x254c,0x100001n]]){
 const t=await machine(mode),{vm}=t,f=image(mode);f.bytes.set(encode(value,4),f.offset(at));vm.supplyFile('C:\\versions\\sample.dll',f.bytes);
 equal(api(t,'GetFileVersionInfoSizeW',[filename(t),0n]),0n);equal(vm.readInt(t.error),1813n);vm.dispose();
}

// Filename conversion uses the same process ACP as other ANSI file interfaces.
for(const mode of [32,64]){
 const t=await setup(mode),{vm}=t;
 vm.supplyFile('C:\\versions\\ソフト.dll',t.f.bytes);
 vm.write(0x20000n,[...new TextEncoder().encode('C:\\versions\\'),0x83,0x5c,0x83,0x74,0x83,0x67,...new TextEncoder().encode('.dll\0')]);
 equal(api(t,'GetFileVersionInfoSizeA',[0x20000n,0n]),BigInt(blob.length*2+4));
 const file=call(t,'CreateFileW',[filename(t),0x40000000n,0n,0n,3n,0n,0n],'kernel32.dll');assert.notEqual(file,(1n<<BigInt(mode))-1n);
 equal(api(t,'GetFileVersionInfoSizeW',[filename(t),0x22000n]),0n);equal(vm.readInt(t.error),32n);equal(vm.readInt(0x22000n),0n);equal(call(t,'CloseHandle',[file],'kernel32.dll'),1n);vm.dispose();
}

// Outputs are all preflighted, including cross-page handles, ANSI scratch and LastError.
for(const mode of [32,64])for(const kind of ['size','info','query','scratch','error','return']){
 const t=await setup(mode),{vm}=t,name=filename(t);vm.write(0x26000n,[...blob,...new Uint8Array(blob.length+4).fill(0xa5)]);
 let page=0x25000n,protection=2;
 if(kind==='size')frame(t,'GetFileVersionInfoSizeW',[name,0x24ffen],'version.dll');
 if(kind==='info')frame(t,'GetFileVersionInfoW',[name,0n,BigInt(blob.length*2+4),0x24ff0n],'version.dll');
 if(kind==='query'||kind==='return'){
  vm.write(0x21000n,wide('\\\0'));frame(t,'VerQueryValueW',[0x26000n,0x21000n,kind==='query'?0x24ffcn:0x22000n,kind==='query'?0x25000n:0x22010n],'version.dll');
  if(kind==='return'){page=t.sp&~4095n;protection=1;}
 }
 if(kind==='scratch'){
  vm.write(0x21000n,new TextEncoder().encode('\\StringFileInfo\\041103a4\\CompanyName\0'));frame(t,'VerQueryValueA',[0x26000n,0x21000n,0x22000n,0x22010n],'version.dll');page=0x26000n;
 }
 if(kind==='error'){frame(t,'GetFileVersionInfoSizeW',[name,0x22000n],'version.dll');page=t.image.teb;}
 equal(core.wire_protect(t.handle,page,4096n,protection),0);
 const before=snapshot(t),pointer=data(vm,0x22000n,24),prefix=data(vm,0x24ff0n,16);
 equal(vm.service().reason,Exit.Fault);equal(snapshot(t),before);equal(data(vm,0x22000n,24),pointer);equal(data(vm,0x24ff0n,16),prefix);
 equal(core.wire_protect(t.handle,page,4096n,4),0);equal(vm.service().reason,Exit.Dispatch);equal(vm.register(0),kind==='size'||kind==='error'?BigInt(blob.length*2+4):1n);vm.dispose();
}

// Saved returns survive output aliases, and Win64 retains complete guest pointers.
for(const mode of [32,64]){
 const t=await setup(mode),{vm}=t,name=filename(t);
 equal(api(t,'GetFileVersionInfoW',[name,0n,BigInt(blob.length*2+4),0x26000n]),1n);
 equal(query(t,'\\',false,0x26000n,t.sp,0x22010n),1n);equal(vm.ip,vm.entryPoints.returned);equal(vm.readInt(t.sp,mode/8),0x26028n);
 if(mode===64){
  const high=0x123400000000n;vm.map(high,8192n,7);vm.write(high,wide('C:\\versions\\sample.dll\0'));
  equal(api(t,'GetFileVersionInfoW',[high,0n,BigInt(blob.length*2+4),high+4096n]),1n);equal(query(t,'\\',false,high+4096n,high+512n,high+528n),1n);equal(vm.readInt(high+512n,8),high+4096n+40n);
 }
 vm.dispose();
}

console.log(`${checks} version resource/PE32-PE32+/ABI/VFS/ANSI/truncation/alias/fault checks passed`);
