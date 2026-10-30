import {moduleFixture} from './module-fixture.mjs';

export function delayFixture(mode,{dll=false,library='leaf.dll',symbols=['GetValue',17],va=false,unload=true,...options}={}){
 const f=moduleFixture(mode,{dll,...options}),{ptr,preferred,rput,rtext,absolute}=f;
 // Legacy VA descriptors use DWORDs; keep their preferred image below 4 GiB.
 if(va&&mode===64){const delta=preferred-0x400000n;for(const at of f.relocations)rput(at,f.view.getBigUint64(f.offset(at),true)-delta,8);f.preferred=0x400000n;f.put(f.opt+24,f.preferred,8);}
 const base=f.preferred,address=(at,rva,width=4)=>va?absolute(at,base+BigInt(rva),width):rput(at,rva,width);
 f.put(f.dirs+13*8,0x2100);f.put(f.dirs+13*8+4,64);rput(0x2100,va?0:1);address(0x2104,0x2180);address(0x2108,0x3100);address(0x210c,0x3120);address(0x2110,0x2200);if(unload)address(0x2118,0x3180);rtext(0x2180,library+'\0');
 let name=0x2400;
 symbols.forEach((symbol,index)=>{const at=0x2200+index*ptr;if(typeof symbol==='number')rput(at,(1n<<BigInt(mode-1))|BigInt(symbol),ptr);else{address(at,name,ptr);rput(name,0,2);rtext(name+2,symbol+'\0');name+=symbol.length+3;}rput(0x3120+index*ptr,base+0x1300n,ptr);if(unload)rput(0x3180+index*ptr,base+0x1300n,ptr);});
 f.bytes.set([0x0f,0x0b],f.offset(0x1300));
 if(!dll){f.bytes.set([0xb8,42,0,0,0,0xc3],f.offset(0x1000));const kept=f.relocations.filter(rva=>rva<0x1000||rva>=0x1100);f.relocations.splice(0,f.relocations.length,...kept);}
 f.rebuildRelocations();
 if(va&&mode===64){let at=0x4000,end=at+f.view.getUint32(f.dirs+5*8+4,true);while(at<end){const page=f.view.getUint32(f.offset(at),true),size=f.view.getUint32(f.offset(at+4),true);for(let n=8;n<size;n+=2){const token=f.view.getUint16(f.offset(at+n),true),rva=page+(token&4095);if([0x2104,0x2108,0x210c,0x2110,0x2118].includes(rva))rput(at+n,3<<12|(rva&4095),2);}at+=size;}}
 return f;
}
