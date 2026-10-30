const bytes=values=>Uint8Array.from(values);
const text=value=>{const out=new Uint8Array(16);out.set(Buffer.from(value).subarray(0,16));return out;};
export const cases=[
 {a:new Uint8Array(16),b:new Uint8Array(16),la:0n,lb:0n},
 {a:text('abc'),b:text('abc'),la:3n,lb:3n},
 {a:text('abc'),b:text('zabc'),la:-3n,lb:-4n},
 {a:text('abc'),b:text('xxxxxxxxxxxxxxab'),la:3n,lb:16n},
 {a:bytes([0x80,0xff,0,0x7f,0xff,0,0x80,0,0x7f,0xff,1,2,3,4,5,6]),b:bytes([0xff,0x80,0x7f,0,1,0x81,0xfe,0xff,0,2,6,5,4,3,2,1]),la:15n,lb:17n},
 {a:bytes([0,0x80,0xff,0xff,0,0,0xff,0x7f,0,0x80,1,0,0xfe,0xff,0xff,0x7f]),b:bytes([0xff,0xff,0,0x80,0xff,0x7f,0,0,1,0,0xfe,0xff,0xff,0x7f,0,0x80]),la:-9n,lb:7n},
 {a:bytes(Array.from({length:16},(_,n)=>n+1)),b:bytes(Array.from({length:16},(_,n)=>16-n)),la:0x80000000n,lb:0x7fffffffn},
 {a:new Uint8Array(16).fill(0x80),b:new Uint8Array(16).fill(0x80),la:0x8000000000000000n,lb:0xffffffffffffffffn},
 {a:text('abc\0tail'),b:text('cab\0tail'),la:0x1234000000000001n,lb:0xffffffff00000002n},
];

export function reference({explicit},control,aBytes,bBytes,la=0n,lb=0n,wide=false){
 const width=control&1?2:1,count=16/width,signed=!!(control&2),read=raw=>Array.from({length:count},(_,n)=>{let x=raw[n*width]+(width===2?raw[n*width+1]*256:0);if(signed&&x>=2**(width*8-1))x-=2**(width*8);return x;}),a=read(aBytes),b=read(bBytes);
 const limit=n=>{n=BigInt.asIntN(wide?64:32,n);if(n<0n)n=-n;return Number(n>BigInt(count)?BigInt(count):n);},implicit=values=>{const at=values.indexOf(0);return at<0?count:at;},alen=explicit?limit(la):implicit(a),blen=explicit?limit(lb):implicit(b),kind=(control>>2)&3;
 const set=new Set(a.slice(0,alen)),ranges=[];for(let n=0;n+1<alen;n+=2)ranges.push([a[n],a[n+1]]);
 let bits=b.map((x,j)=>{
  if(kind===0)return j<blen&&set.has(x);
  if(kind===1)return j<blen&&ranges.some(([lo,hi])=>lo<=x&&x<=hi);
  if(kind===2)return j>=alen&&j>=blen||j<alen&&j<blen&&a[j]===x;
  return a.slice(0,Math.min(alen,count-j)).every((x,i)=>j+i<blen&&b[j+i]===x);
 });
 if(control&16)bits=bits.map((bit,n)=>(control&32)&&n>=blen?bit:!bit);
 const selected=bits.flatMap((bit,n)=>bit?[n]:[]),index=selected.length?(control&64?selected.at(-1):selected[0]):count,mask=new Uint8Array(16);
 if(control&64){for(const n of selected)mask.fill(255,n*width,(n+1)*width);}else for(const n of selected)mask[n>>3]|=1<<(n&7);
 const flags=(selected.length?1:0)|(blen<count?64:0)|(alen<count?128:0)|(bits[0]?2048:0);
 return {index,mask,flags,bits,alen,blen};
}

export const specs=[{opcode:0x60,explicit:true,masked:true},{opcode:0x61,explicit:true,masked:false},{opcode:0x62,explicit:false,masked:true},{opcode:0x63,explicit:false,masked:false}];
