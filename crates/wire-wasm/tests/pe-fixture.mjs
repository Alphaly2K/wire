// Deterministic disk images with independent section, import, relocation and TLS records.
export function fixture(mode=64,{tls=true,lookup=true}={}){
 const bytes=new Uint8Array(0x1200),view=new DataView(bytes.buffer),preferred=mode===32?0x400000n:0x140000000n,ptr=mode/8,opt=0x98,optSize=mode===32?0xe0:0xf0,dirs=opt+(mode===32?96:112),sections=opt+optSize;
 const put=(at,value,size=4)=>{value=BigInt(value);for(let n=0;n<size;n++)bytes[at+n]=Number((value>>BigInt(n*8))&255n);};
 const text=(at,value)=>bytes.set(new TextEncoder().encode(value),at);
 const offset=rva=>rva<0x1000?rva:rva<0x2000?0x400+rva-0x1000:rva<0x3000?0x800+rva-0x2000:rva<0x4000?0xe00+rva-0x3000:0x1000+rva-0x4000;
 const rput=(rva,value,size=4)=>put(offset(rva),value,size),rtext=(rva,value)=>text(offset(rva),value),patches=[];
 const absolute=(rva,value,size=ptr)=>{rput(rva,value,size);patches.push(rva);};
 text(0,'MZ');put(0x3c,0x80);text(0x80,'PE\0\0');put(0x84,mode===32?0x14c:0x8664,2);put(0x86,4,2);put(0x94,optSize,2);put(0x96,0x102,2);put(opt,mode===32?0x10b:0x20b,2);put(opt+16,0x1000);put(opt+(mode===32?28:24),preferred,ptr);put(opt+32,0x1000);put(opt+36,0x200);put(opt+56,0x5000);put(opt+60,0x400);put(opt+68,3,2);put(opt+70,0x140,2);put(opt+72,0x100000,ptr);put(opt+72+ptr,0x1000,ptr);put(dirs-4,16);
 for(const [n,name,rva,virtual,raw,rawSize,flags] of [[0,'.text',0x1000,0x1000,0x400,0x400,0x60000020],[1,'.rdata',0x2000,0x1000,0x800,0x600,0x40000040],[2,'.data',0x3000,0x1000,0xe00,0x200,0xc0000040],[3,'.reloc',0x4000,0x1000,0x1000,0x200,0x42000040]]){const at=sections+n*40;text(at,name);put(at+8,virtual);put(at+12,rva);put(at+16,rawSize);put(at+20,raw);put(at+36,flags);}
 put(dirs+8,0x2000);put(dirs+12,40);rput(0x2000,lookup?0x2080:0);rput(0x200c,0x20c0);rput(0x2010,0x20a0);rput(0x2080,0x20e0,ptr);rput(0x20a0,0x20e0,ptr);rtext(0x20c0,'kernel32.dll\0');rtext(0x20e2,'QueryPerformanceCounter\0');
 const emit=(rva,code)=>bytes.set(code,offset(rva));
 const relative=(from,target)=>Number(BigInt.asUintN(32,BigInt(target-from)));
 if(mode===32){
  emit(0x1000,[0x68,0,0,0,0,0xff,0x15,0,0,0,0,0xa1,0,0,0,0,0xc3]);absolute(0x1001,preferred+0x3000n);absolute(0x1007,preferred+0x20a0n);absolute(0x100c,preferred+0x3000n);
  emit(0x1100,[0x64,0xa1,0x2c,0,0,0,0x8b,0,0x8b,0,0xa3,0,0,0,0,0xff,0x05,0,0,0,0,0xc2,12,0]);absolute(0x110b,preferred+0x3010n);absolute(0x1111,preferred+0x3008n);
 }else{
  emit(0x1000,[0x48,0x83,0xec,0x28,0x48,0x8d,0x0d,0,0,0,0,0xff,0x15,0,0,0,0,0x48,0x8b,0x05,0,0,0,0,0x48,0x83,0xc4,0x28,0xc3]);rput(0x1007,relative(0x100b,0x3000));rput(0x100d,relative(0x1011,0x20a0));rput(0x1014,relative(0x1018,0x3000));
  emit(0x1100,[0x65,0x48,0x8b,0x04,0x25,0x58,0,0,0,0x48,0x8b,0,0x8b,0,0x89,0x05,0,0,0,0,0xff,0x05,0,0,0,0,0xc3]);rput(0x1110,relative(0x1114,0x3010));rput(0x1116,relative(0x111a,0x3008));
 }
 if(tls){put(dirs+9*8,0x2200);put(dirs+9*8+4,ptr*4+8);for(const [n,va] of [0x2400n,0x2404n,0x300cn,0x2300n].entries())absolute(0x2200+n*ptr,preferred+va);rput(0x2200+ptr*4,12);rput(0x2200+ptr*4+4,5<<20);absolute(0x2300,preferred+0x1100n);absolute(0x2300+ptr,preferred+0x1100n);rput(0x2400,0x11223344);}
 const rebuildRelocations=()=>{bytes.fill(0,0x1000,0x1200);put(dirs+5*8,0);put(dirs+5*8+4,0);const groups=new Map();for(const rva of patches){const page=rva&~4095;if(!groups.has(page))groups.set(page,[]);groups.get(page).push((mode===32?3:10)<<12|(rva&4095));}
 let cursor=0x4000;for(const [page,entries] of groups){if(entries.length%2)entries.push(0);rput(cursor,page);rput(cursor+4,8+entries.length*2);for(let n=0;n<entries.length;n++)rput(cursor+8+n*2,entries[n],2);cursor+=8+entries.length*2;}
 if(cursor>0x4000){put(dirs+5*8,0x4000);put(dirs+5*8+4,cursor-0x4000);}};rebuildRelocations();
 return {bytes,view,preferred,ptr,opt,dirs,sections,put,rput,rtext,offset,absolute,relocations:patches,rebuildRelocations};
}
