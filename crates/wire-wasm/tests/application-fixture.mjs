import {moduleFixture} from './module-fixture.mjs';

// Main returns normally after installing FLS; a static DLL also owns two TLS callbacks.
export function applicationFixture(mode) {
  const program=moduleFixture(mode,{dependency:'library.dll',tls:true}),library=moduleFixture(mode,{dll:true,tls:true});
  const f=program,ptr=mode/8,iat=0x2140,names=['FlsAlloc','FlsSetValue','GetFileAttributesW'];
  f.rput(0x2014,0x2100);f.rput(0x2020,0x2500);f.rput(0x2024,iat);f.rtext(0x2500,'kernel32.dll\0');f.put(f.dirs+12,60);
  let at=0x2480;
  for(const [n,name]of names.entries()){f.rput(0x2100+n*ptr,at,ptr);f.rput(iat+n*ptr,at,ptr);f.rtext(at+2,name+'\0');at+=name.length+3;}
  f.bytes.fill(0x90,f.offset(0x1000),f.offset(0x1100));
  f.relocations.splice(0,f.relocations.length,...f.relocations.filter(rva=>rva>=0x1100));
  let pc=0x1000;
  const emit=(...bytes)=>{f.bytes.set(bytes,f.offset(pc));pc+=bytes.length;};
  const absolute=(prefix,rva)=>{emit(...prefix);f.absolute(pc,f.preferred+BigInt(rva),ptr);pc+=ptr;};
  const relative=(prefix,rva)=>{emit(...prefix);f.rput(pc,BigInt.asUintN(32,BigInt(rva-pc-4)));pc+=4;};
  const call=n=>mode===32?absolute([0xff,0x15],iat+n*ptr):relative([0xff,0x15],iat+n*ptr);
  const address=(prefix,rva)=>absolute(prefix,rva);
  if(mode===32){address([0x68],0x1080);call(0);address([0x68],0x3180);emit(0x50);call(1);address([0x68],0x3140);call(2);absolute([0xa3],0x3188);}
  else{emit(0x48,0x83,0xec,40);address([0x48,0xb9],0x1080);call(0);emit(0x89,0xc1);address([0x48,0xba],0x3180);call(1);address([0x48,0xb9],0x3140);call(2);relative([0x89,0x05],0x3188);emit(0x48,0x83,0xc4,40);}
  emit(0xb8,37,0,0,0,0xc3);
  if(pc>0x1080)throw Error('application fixture code overflow');
  f.bytes.set(mode===32?[0x8b,0x44,0x24,4,0xff,0x00,0xc2,4,0]:[0xff,0x01,0xc3],f.offset(0x1080));
  f.bytes.set(Uint8Array.from([...'C:\\empty\\nested\0'].flatMap(c=>[c.charCodeAt(0),0])),f.offset(0x3140));f.rebuildRelocations();
  const manifest={program:{path:'C:\\app.exe',url:'/app',size:program.bytes.length},files:[{path:'C:\\library.dll',url:'/library',size:library.bytes.length}],directories:['C:\\empty','C:\\empty\\nested'],command:'"C:\\app.exe"'};
  return {program,library,manifest,files:new Map([['/app',program.bytes],['/library',library.bytes]])};
}
