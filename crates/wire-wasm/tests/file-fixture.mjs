import {fixture} from './pe-fixture.mjs';

export const filePosition=(1n<<36n)+19n;
export const fileMapOffset=(1n<<36n)+7n*65536n;

// The guest reads a high file offset, writes privately, then copies a mapped view.
export function fileFixture(mode,{mapping=true,readLength=33}={}){
 const f=fixture(mode,{tls:false}),names=['CreateFileA','GetFileSizeEx','SetFilePointerEx','ReadFile','WriteFile','CreateFileMappingA','MapViewOfFile','UnmapViewOfFile','CloseHandle'];
 f.relocations.splice(0);f.bytes.fill(0,f.offset(0x2040),f.offset(0x2400));f.rput(0x2000,0x2040);f.rput(0x2010,0x20a0);f.rput(0x200c,0x2160);f.rtext(0x2160,'kernel32.dll\0');
 let name=0x2180;for(const[n,value]of names.entries()){f.rput(0x2040+n*f.ptr,name,f.ptr);f.rput(0x20a0+n*f.ptr,name,f.ptr);f.rtext(name+2,value+'\0');name+=value.length+3;}
 f.rtext(0x2360,'C:\\large.dat\0');f.bytes.set([9,7,5],f.offset(0x3100));
 let pc=0x1000;
 const encode=(value,width)=>Array.from({length:width},(_,n)=>Number(BigInt.asUintN(width*8,BigInt(value))>>BigInt(n*8)&255n));
 const emit=(...code)=>{f.bytes.set(code,f.offset(pc));pc+=code.length;};
 const absolute=(prefix,rva)=>{emit(...prefix);f.absolute(pc,f.preferred+BigInt(rva));pc+=f.ptr;};
 const relative=(prefix,rva)=>{emit(...prefix);emit(...encode(rva-pc-4,4));};
 const pointer=rva=>({rva}),slot=rva=>({slot:rva});
 const save=rva=>mode===32?absolute([0xa3],rva):relative([0x48,0x89,0x05],rva);
 const invoke=(name,args)=>{
  if(mode===32){
   for(const a of args.toReversed())if(a.rva!==undefined)absolute([0x68],a.rva);else if(a.slot!==undefined)absolute([0xff,0x35],a.slot);else emit(0x68,...encode(a,4));
   absolute([0xff,0x15],0x20a0+names.indexOf(name)*f.ptr);
  }else{
   for(const[n,a]of args.entries()){
    const reg=n<4?[1,2,8,9][n]:0,prefix=reg<8?0x48:0x49;
    if(a.rva!==undefined)absolute([prefix,0xb8+(reg&7)],a.rva);
    else if(a.slot!==undefined)relative([reg<8?0x48:0x4c,0x8b,0x05+(reg&7)*8],a.slot);
    else emit(prefix,0xb8+(reg&7),...encode(a,8));
    if(n>=4)emit(0x48,0x89,0x44,0x24,32+(n-4)*8);
   }
   relative([0xff,0x15],0x20a0+names.indexOf(name)*f.ptr);
  }
 };
 const seek=()=>invoke('SetFilePointerEx',mode===32?[slot(0x30e0),filePosition&0xffffffffn,filePosition>>32n,0n,0n]:[slot(0x30e0),filePosition,0n,0n]);
 emit(0x56,0x57);if(mode===64)emit(0x48,0x83,0xec,72);
 invoke('CreateFileA',[pointer(0x2360),0xc0000000n,7n,0n,3n,0n,0n]);save(0x30e0);
 invoke('GetFileSizeEx',[slot(0x30e0),pointer(0x3000)]);seek();
 const readRva=readLength>4096?0x4000:0x3020;
 invoke('ReadFile',[slot(0x30e0),pointer(readRva),BigInt(readLength),pointer(0x3008),0n]);seek();
 invoke('WriteFile',[slot(0x30e0),pointer(0x3100),3n,pointer(0x300c),0n]);seek();
 invoke('ReadFile',[slot(0x30e0),pointer(0x3050),4n,pointer(0x3010),0n]);
 if(mapping){
  invoke('CreateFileMappingA',[slot(0x30e0),0n,2n,0n,0n,0n]);save(0x30e8);
  invoke('MapViewOfFile',[slot(0x30e8),4n,fileMapOffset>>32n,fileMapOffset&0xffffffffn,64n]);save(0x30f0);
  if(mode===32){absolute([0x8b,0x35],0x30f0);absolute([0xbf],0x3080);}else{relative([0x48,0x8b,0x35],0x30f0);absolute([0x48,0xbf],0x3080);}
  emit(0xb9,64,0,0,0,0xf3,0xa4);
  invoke('UnmapViewOfFile',[slot(0x30f0)]);invoke('CloseHandle',[slot(0x30e8)]);
 }
 invoke('CloseHandle',[slot(0x30e0)]);
 if(mode===64)emit(0x48,0x83,0xc4,72);emit(0x5f,0x5e,0xb8,9,0,0,0,0xc3);
 if(pc>0x1400)throw Error('file fixture code capacity exceeded');f.rebuildRelocations();
 if(readLength>4096){
  const end=Math.ceil((readRva+readLength)/4096)*4096;
  f.put(f.sections+2*40+8,end-0x3000);f.put(f.sections+3*40+12,end);f.put(f.dirs+5*8,end);f.put(f.opt+56,end+4096);
 }
 return f;
}
