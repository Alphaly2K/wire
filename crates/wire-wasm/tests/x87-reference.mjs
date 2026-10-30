// Binary ratios and explicit FP80 encoding provide an independent reference.
export const mask=bits=>(1n<<BigInt(bits))-1n;
export const indefinite=0xffffc000000000000000n;
export function read(bits){
 const sign=!!(bits>>79n),exp=Number(bits>>64n)&0x7fff,sig=bits&mask(64),j=!!(sig>>63n),nan=exp===0x7fff&&j&&(sig&mask(63))!==0n,infinity=exp===0x7fff&&sig===1n<<63n,unsupported=exp!==0&&!j;
 const power=(exp===0?-16382:exp-16383)-63,n=sig<<BigInt(Math.max(0,power)),d=1n<<BigInt(Math.max(0,-power));
 return {bits,sign,exp,sig,n:sign?-n:n,d,nan,infinity,unsupported,signaling:nan&&!(sig&(1n<<62n)),denormal:exp===0&&sig!==0n,zero:exp===0&&sig===0n};
}
const precision=cw=>((cw>>8)&3)===0?24:((cw>>8)&3)===2?53:64;
function magnitude(n,d){let e=n.toString(2).length-d.toString(2).length;if(e>=0?n<(d<<BigInt(e)):(n<<BigInt(-e))<d)e--;return e;}
function integral(n,d,negative,rc){let q=n/d;const r=n%d,up=r!==0n&&(rc===0?(r*2n>d||r*2n===d&&!!(q&1n)):rc===1?negative:rc===2?!negative:false);if(up)q++;return {q,r,up};}
function quantize(n,d,e,p,negative,rc){const unit=e-p+1;return integral(unit<0?n<<BigInt(-unit):n,unit>0?d<<BigInt(unit):d,negative,rc);}
function encoded(q,e,p,negative,bits){
 const bias=bits===80?16383:bits===32?127:1023,sign=negative?1n<<BigInt(bits-1):0n;
 if(q>=1n<<BigInt(p)){q>>=1n;e++;}
 const normal=q>=1n<<BigInt(p-1),exp=normal?BigInt(e+bias):0n;
 return sign|(exp<<BigInt(bits===80?64:p-1))|(bits===80?q<<BigInt(64-p):q&mask(p-1));
}
const inf=(sign,bits=80)=>(sign?1n<<BigInt(bits-1):0n)|(bits===80?0x7fff8000000000000000n:BigInt(bits===32?255:2047)<<BigInt(bits===32?23:52));
export function rounded(n,d,negativeZero,cw,bits=80){
 const p=bits===80?precision(cw):bits===32?24:53,emin=bits===80?-16382:bits===32?-126:-1022,emax=bits===80?16383:bits===32?127:1023,negative=n<0n,rc=(cw>>10)&3;
 if(n===0n)return {value:negativeZero?1n<<BigInt(bits-1):0n,flags:0,c1:0,commit:true};
 if(negative)n=-n;
 const e=magnitude(n,d),wide=quantize(n,d,e,p,negative,rc),we=e+(wide.q>=1n<<BigInt(p)?1:0),over=we>emax,tiny=we<emin,unmasked=over&&!(cw&8)||tiny&&!(cw&16);
 if(unmasked){const flags=(over?8:16)|(bits===80&&wide.r!==0n?32:0);return {value:bits===80?encoded(wide.q,e+(over?-24576:24576),p,negative,80):0n,flags,c1:bits===80&&wide.up?0x200:0,commit:bits===80};}
 if(over){const toInf=rc===0||rc===1&&negative||rc===2&&!negative,bias=bits===80?16383:bits===32?127:1023;
  const max=(negative?1n<<BigInt(bits-1):0n)|(BigInt(emax+bias)<<BigInt(bits===80?64:p-1))|(bits===80?mask(p)<<BigInt(64-p):mask(p-1));return {value:toInf?inf(negative,bits):max,flags:40,c1:toInf?0x200:0,commit:true};}
 const normalE=Math.max(e,emin),out=quantize(n,d,normalE,p,negative,rc),flags=out.r!==0n?32|(tiny?16:0):0;
 return {value:encoded(out.q,normalE,p,negative,bits),flags,c1:out.up?0x200:0,commit:true};
}
export function finish(old,cw,result){const blocked=result.flags&~cw&0x3f;return {...result,sw:(old&~0x200)|result.flags|result.c1|(blocked?0x8080:0)};}
const pre=(value,flags,cw)=>({value,flags,c1:0,commit:!(flags&~cw&7)});
export function binary(kind,av,bv,cw,memoryDenormal=false){
 const a=read(av),b=read(bv);
 if(a.unsupported||b.unsupported)return pre(indefinite,1,cw);
 if(a.nan||b.nan){let value;if(!a.nan)value=b;else if(!b.nan)value=a;else if(a.signaling!==b.signaling)value=a.signaling?b:a;else value=a.sig>=b.sig?a:b;
  return pre(value.bits|(1n<<62n),a.signaling||b.signaling?1:0,cw);}
 const negative=a.sign!==b.sign;
 if(kind==='div'&&a.zero&&b.zero||kind==='div'&&a.infinity&&b.infinity||kind==='mul'&&(a.infinity&&b.zero||b.infinity&&a.zero)||kind==='add'&&a.infinity&&b.infinity&&a.sign!==b.sign||kind==='sub'&&a.infinity&&b.infinity&&a.sign===b.sign)return pre(indefinite,1,cw);
 if(kind==='div'&&b.zero&&!a.zero&&!a.infinity)return pre(inf(negative),4,cw);
 const flags=a.denormal||b.denormal||memoryDenormal?2:0;if(flags&~cw)return pre(av,flags,cw);
 if(kind==='add'||kind==='sub'){
  const bs=kind==='sub'?!b.sign:b.sign;if(a.infinity||b.infinity)return pre(inf(a.infinity?a.sign:bs),flags,cw);
  const out=rounded(a.n*b.d+(kind==='sub'?-b.n:b.n)*a.d,a.d*b.d,a.zero&&b.zero&&a.sign===bs?a.sign:((cw>>10)&3)===1,cw);return {...out,flags:out.flags|flags};
 }
 if(kind==='mul'){if(a.infinity||b.infinity)return pre(inf(negative),flags,cw);const out=rounded(a.n*b.n,a.d*b.d,negative,cw);return {...out,flags:out.flags|flags};}
 if(a.infinity)return pre(inf(negative),flags,cw);if(b.infinity)return pre(negative?1n<<79n:0n,flags,cw);
 const out=rounded(a.n*b.d*(b.n<0n?-1n:1n),a.d*(b.n<0n?-b.n:b.n),negative,cw);return {...out,flags:out.flags|flags};
}
export function store(value,cw,bits){const x=read(value);if(x.unsupported)return pre(bits===32?0xffc00000n:0xfff8000000000000n,1,cw);
 if(x.nan){const shift=bits===32?40n:11n,p=bits===32?23:52;return pre(inf(x.sign,bits)|((x.sig&mask(63))>>shift)|(1n<<BigInt(p-1)),x.signaling?1:0,cw);}
 if(x.infinity)return pre(inf(x.sign,bits),0,cw);return rounded(x.n,x.d,x.sign,cw,bits);}
export function integer(value,cw,bits,trunc=false){const x=read(value);if(x.unsupported||x.nan||x.infinity)return pre(1n<<BigInt(bits-1),1,cw);const negative=x.n<0n,n=negative?-x.n:x.n,out=integral(n,x.d,negative,trunc?3:(cw>>10)&3);let q=negative?-out.q:out.q;if(q<-(1n<<BigInt(bits-1))||q>(1n<<BigInt(bits-1))-1n)return pre(1n<<BigInt(bits-1),1,cw);return {value:q&mask(bits),flags:out.r!==0n?32:0,c1:!trunc&&out.up?0x200:0,commit:true};}
export function fromFloat(value,bits){const p=bits===32?23:52,bias=bits===32?127:1023,exp=Number(value>>BigInt(p))&(bits===32?255:2047),fraction=value&mask(p),sign=!!(value>>BigInt(bits-1));if(exp===(bits===32?255:2047))return {value:inf(sign)|(fraction<<BigInt(63-p)),flags:fraction&&!(fraction&(1n<<BigInt(p-1)))?1:0};const power=(exp?exp-bias:1-bias)-p,sig=fraction+(exp?1n<<BigInt(p):0n),n=(sign?-sig:sig)<<BigInt(Math.max(0,power)),d=1n<<BigInt(Math.max(0,-power)),out=rounded(n,d,sign,0x37f);return {...out,flags:exp===0&&fraction?2:0};}
function isqrt(n){if(!n)return 0n;let q=1n<<BigInt(Math.ceil(n.toString(2).length/2));for(;;){const next=(q+n/q)>>1n;if(next>=q)return q;q=next;}}
export function sqrt(value,cw){const x=read(value);if(x.unsupported||x.sign&&!x.zero&&!x.nan)return pre(indefinite,1,cw);if(x.nan)return pre(value|(1n<<62n),x.signaling?1:0,cw);if(x.zero||x.infinity)return pre(value,0,cw);const flags=x.denormal?2:0;if(flags&~cw)return pre(value,flags,cw);const p=precision(cw),e=Math.floor(magnitude(x.n,x.d)/2),unit=e-p+1,n=unit<0?x.n<<BigInt(-2*unit):x.n,d=unit>0?x.d<<BigInt(2*unit):x.d;let q=isqrt(n/d);const exact=q*q*d===n,rc=(cw>>10)&3,mid=(2n*q+1n)**2n*d,up=!exact&&(rc===2||rc===0&&(4n*n>mid||4n*n===mid&&!!(q&1n)));if(up)q++;return {value:encoded(q,e,p,false,80),flags:flags|(exact?0:32),c1:up?0x200:0,commit:true};}
export function comparison(aBits,bBits,cw,quiet=false,extra=false,reportDenormal=true){const a=read(aBits),b=read(bBits),unordered=a.nan||b.nan||a.unsupported||b.unsupported,flags=a.unsupported||b.unsupported||a.signaling||b.signaling||!quiet&&(a.nan||b.nan)?1:reportDenormal&&!unordered&&(a.denormal||b.denormal||extra)?2:0;let order=0;if(unordered)order=null;else if(a.infinity||b.infinity){order=a.infinity&&b.infinity?(a.sign===b.sign?0:a.sign?-1:1):a.infinity?(a.sign?-1:1):b.sign?1:-1;}else{const diff=a.n*b.d-b.n*a.d;order=diff===0n?0:diff<0n?-1:1;}return {value:0n,flags,c1:0,commit:!(flags&~cw&3),cc:order===null?0x4500:order<0?0x100:order===0?0x4000:0};}
export function roundIntegral(value,cw){const x=read(value);if(x.unsupported)return pre(indefinite,1,cw);if(x.nan)return pre(value|(1n<<62n),x.signaling?1:0,cw);if(x.zero||x.infinity)return pre(value,0,cw);const flags=x.denormal?2:0;if(flags&~cw)return pre(value,flags,cw);const r=integral(x.n<0n?-x.n:x.n,x.d,x.sign,(cw>>10)&3),q=x.sign?-r.q:r.q;return {value:rounded(q,1n,x.sign,cw|0x300).value,flags:flags|(r.r===0n?0:32),c1:r.up?0x200:0,commit:true};}
export function remainder(av,bv,cw,nearest=false){
 const a=read(av),b=read(bv);
 if(a.unsupported||b.unsupported)return pre(indefinite,1,cw);
 if(a.nan||b.nan)return binary('add',av,bv,cw);
 if(a.infinity||b.zero)return pre(indefinite,1,cw);
 const flags=a.denormal||b.denormal?2:0;
 if(flags&~cw)return pre(av,flags,cw);
 let n=a.n,d=a.d,q=0n,partial=false;
 if(!a.zero&&!b.infinity){
  const delta=magnitude(a.n<0n?-a.n:a.n,a.d)-magnitude(b.n<0n?-b.n:b.n,b.d);partial=delta>=64;
  const divisor=b.n<<BigInt(partial?delta-32:0),dn=a.n*b.d,dd=a.d*divisor,negative=(dn<0n)!==(dd<0n);
  q=integral(dn<0n?-dn:dn,dd<0n?-dd:dd,negative,nearest&&!partial?0:3).q;
  n=a.n*b.d-(negative?-q:q)*divisor*a.d;d=a.d*b.d;
 }
 let value=rounded(n,d,a.sign,0x37f).value,status=flags;
 if(read(value).denormal&&!(cw&16)){status|=16;value=rounded(n<<24576n,d,a.sign,0x37f).value;}
 const bits=Number(q&7n),cc=partial?0x400:((bits&4)<<6)|((bits&2)<<13)|((bits&1)<<9);
 return {value,flags:status,c1:0,commit:true,cc,partial};
}
export function scale(av,bv,cw){
 const a=read(av),b=read(bv);
 if(a.unsupported||b.unsupported)return pre(indefinite,1,cw);
 if(a.nan||b.nan)return binary('add',av,bv,cw);
 if(b.infinity&&(a.zero&&!b.sign||a.infinity&&b.sign))return pre(indefinite,1,cw);
 const flags=a.denormal||b.denormal?2:0;
 if(flags&~cw)return pre(av,flags,cw);
 if(b.infinity)return pre(b.sign?(a.sign?1n<<79n:0n):inf(a.sign),flags,cw);
 if(a.zero||a.infinity)return pre(av,flags,cw);
 let exponent=b.n/b.d;exponent=Number(exponent>100000n?100000n:exponent< -100000n?-100000n:exponent);
 const n=exponent>0?a.n<<BigInt(exponent):a.n,d=exponent<0?a.d<<BigInt(-exponent):a.d,out=rounded(n,d,a.sign,cw|0x300);
 if(out.flags&24&&(!(cw&8)&&out.flags&8||!(cw&16)&&out.flags&16)){
  const e=magnitude(n<0n?-n:n,d)+(out.flags&8?-24576:24576);
  if(e>16383)out.value=inf(a.sign);
  if(e< -16382)out.value=a.sign?1n<<79n:0n;
 }
 return {...out,flags:out.flags|flags};
}
export function extract(value,cw){
 const x=read(value);
 if(x.unsupported||x.nan){const v=x.unsupported?indefinite:value|(1n<<62n);return {...pre(v,x.unsupported||x.signaling?1:0,cw),pushed:v};}
 if(x.infinity)return {...pre(inf(false),0,cw),pushed:value};
 if(x.zero)return {...pre(inf(true),4,cw),pushed:value};
 const e=magnitude(x.n<0n?-x.n:x.n,x.d),n=e<0?x.n<<BigInt(-e):x.n,d=e>0?x.d<<BigInt(e):x.d;
 return {...pre(rounded(BigInt(e),1n,false,0x37f).value,x.denormal?2:0,cw),pushed:rounded(n,d,x.sign,0x37f).value};
}
