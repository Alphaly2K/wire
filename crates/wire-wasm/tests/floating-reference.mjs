// Independent binary-format rounding from integer ratios. This never calls
// the runtime's numeric helpers, host float arithmetic, or an x86 executor.
export const mask = bits => (1n << BigInt(bits)) - 1n;
export const reported = (csr, flags) => csr | ((flags & ~(csr >> 7) & 7) ? flags & 7 : flags);
export const format = bits => ({ bits, p: bits === 32 ? 23 : 52, bias: bits === 32 ? 127 : 1023, emax: bits === 32 ? 127 : 1023, emin: bits === 32 ? -126 : -1022 });
export function read(value, bits, csr) {
  const f=format(bits), sign=!!(value>>(BigInt(bits)-1n)), exp=Number((value>>BigInt(f.p))&mask(bits===32?8:11)), fraction=value&mask(f.p);
  const special=exp===(bits===32?255:2047), nan=special&&fraction!==0n, infinity=special&&!nan;
  const denormal=exp===0&&fraction!==0n, zero=exp===0&&(fraction===0n||!!(csr&64));
  const mantissa=zero?0n:fraction+(exp===0?0n:1n<<BigInt(f.p)), power=(exp===0?f.emin:exp-f.bias)-f.p;
  const n=(sign?-mantissa:mantissa)<<(BigInt(Math.max(0,power))),d=1n<<BigInt(Math.max(0,-power));
  return {...f,value,sign,nan,infinity,signaling:nan&&!(fraction&(1n<<BigInt(f.p-1))),fraction,zero,n,d,flags:denormal&&!(csr&64)?2:0};
}
function increment(q,r,d,negative,csr) {
  const rc=(csr>>13)&3;
  return r!==0n&&(rc===0?(2n*r>d||(2n*r===d&&!!(q&1n))):rc===1?negative:rc===2?!negative:false);
}
function magnitude(n,d) {
  let exponent=n.toString(2).length-d.toString(2).length;
  if(exponent>=0?n<(d<<BigInt(exponent)):(n<<BigInt(-exponent))<d) exponent--;
  return exponent;
}
function encoded(q,exponent,negative,bits,flags,csr) {
  const f=format(bits),sign=negative?1n<<BigInt(bits-1):0n;
  if(q>=1n<<BigInt(f.p+1)){q>>=1n;exponent++;}
  if(exponent>f.emax){const rc=(csr>>13)&3,toInfinity=rc===0||rc===1&&negative||rc===2&&!negative;
    return {value:sign|(toInfinity?BigInt(f.emax+f.bias+1)<<BigInt(f.p):((BigInt(f.emax+f.bias)<<BigInt(f.p))|mask(f.p))),flags:flags|8|((csr&0x400)?32:0)};}
  const normal=q>=1n<<BigInt(f.p);
  if(!normal&&q!==0n&&(csr&0x8800)===0x8800)return {value:sign,flags:flags|48};
  return {value:sign|(normal?BigInt(exponent+f.bias)<<BigInt(f.p):0n)|(q&mask(f.p)),flags};
}
export function round(n,d,negativeZero,bits,csr) {
  const negative=n<0n, sign=negative?1n<<BigInt(bits-1):0n;
  if(n===0n)return {value:negativeZero?1n<<BigInt(bits-1):0n,flags:0};
  if(negative)n=-n;
  const f=format(bits),unbounded=magnitude(n,d),exponent=Math.max(f.emin,unbounded),unit=exponent-f.p;
  const numerator=unit<0?n<<BigInt(-unit):n,denominator=unit>=0?d<<BigInt(unit):d;
  let q=numerator/denominator;const r=numerator%denominator;
  if(increment(q,r,denominator,negative,csr))q++;
  const wideUnit=unbounded-f.p,wn=wideUnit<0?n<<BigInt(-wideUnit):n,wd=wideUnit>=0?d<<BigInt(wideUnit):d;
  let wq=wn/wd;const wr=wn%wd;if(increment(wq,wr,wd,negative,csr))wq++;
  const wideExponent=unbounded+(wq>=(1n<<BigInt(f.p+1))?1:0),tiny=wideExponent<f.emin;
  const flags=tiny&&!(csr&0x800)?16|(wr!==0n?32:0):(r!==0n?32|(tiny?16:0):0);
  const result=encoded(q,exponent,negative,bits,flags,csr);
  return {value:result.value|sign,flags:result.flags};
}
function quiet(value,bits) {return value|(1n<<BigInt(format(bits).p-1));}
function indefinite(bits) {const f=format(bits);return (1n<<BigInt(bits-1))|(BigInt(f.emax+f.bias+1)<<BigInt(f.p))|(1n<<BigInt(f.p-1));}
function infinity(negative,bits) {const f=format(bits);return (negative?1n<<BigInt(bits-1):0n)|(BigInt(f.emax+f.bias+1)<<BigInt(f.p));}
export function compare(a,b) {
  if(a.infinity||b.infinity){if(a.infinity&&b.infinity)return a.sign===b.sign?0:a.sign?-1:1;return a.infinity?(a.sign?-1:1):(b.sign?1:-1);}
  const difference=a.n*b.d-b.n*a.d;return difference===0n?0:difference<0n?-1:1;
}
export function arithmetic(op,av,bv,bits,csr) {
  const a=read(av,bits,csr),b=read(bv,bits,csr);
  if(op==='min'||op==='max') {
    if(a.nan||b.nan)return {value:b.zero?(b.sign?1n<<BigInt(bits-1):0n):b.value,flags:1};
    const order=compare(a,b),chosen=op==='min'?order<0:order>0;
    const x=chosen?a:b;
    return {value:x.zero?(x.sign?1n<<BigInt(bits-1):0n):x.value,flags:a.flags|b.flags};
  }
  if(a.nan||b.nan){const x=a.nan?a:b;return {value:quiet(x.value,bits),flags:a.signaling||b.signaling?1:0};}
  if(op==='div'&&b.zero&&!a.zero&&!a.infinity)return {value:infinity(a.sign!==b.sign,bits),flags:4};
  const flags=a.flags|b.flags;
  if(flags&&!(csr&256))return {value:av,flags};
  const negative=a.sign!==b.sign;
  if(op==='add'||op==='sub') {
    const bs=op==='sub'?!b.sign:b.sign;
    if(a.infinity||b.infinity)return a.infinity&&b.infinity&&a.sign!==bs?{value:indefinite(bits),flags:1|flags}:{value:infinity(a.infinity?a.sign:bs,bits),flags};
    const n=a.n*b.d+(op==='sub'?-b.n:b.n)*a.d;
    const result=round(n,a.d*b.d,a.zero&&b.zero&&a.sign===bs?a.sign:((csr>>13)&3)===1,bits,csr);
    return {value:result.value,flags:result.flags|flags};
  }
  if(op==='mul') {
    if((a.infinity&&b.zero)||(b.infinity&&a.zero))return {value:indefinite(bits),flags:1|flags};
    if(a.infinity||b.infinity)return {value:infinity(negative,bits),flags};
    const result=round(a.n*b.n,a.d*b.d,negative,bits,csr);return {value:result.value,flags:result.flags|flags};
  }
  if(a.infinity&&b.infinity||a.zero&&b.zero)return {value:indefinite(bits),flags:1|flags};
  if(a.infinity)return {value:infinity(negative,bits),flags};
  if(b.infinity)return {value:negative?1n<<BigInt(bits-1):0n,flags};
  if(b.zero)return {value:infinity(negative,bits),flags:4|flags};
  const result=round(a.n*b.d*(b.n<0n?-1n:1n),a.d*(b.n<0n?-b.n:b.n),negative,bits,csr);
  return {value:result.value,flags:result.flags|flags};
}
function isqrt(value) {if(value===0n)return 0n;let q=1n<<BigInt(Math.ceil(value.toString(2).length/2));for(;;){const next=(q+value/q)>>1n;if(next>=q)return q;q=next;}}
export function sqrt(value,bits,csr) {
  const a=read(value,bits,csr);
  if(a.nan)return {value:quiet(value,bits),flags:a.signaling?1:0};
  if(a.zero)return {value:a.sign?1n<<BigInt(bits-1):0n,flags:0};
  if(a.sign)return {value:indefinite(bits),flags:1};
  if(a.infinity)return {value,flags:0};
  const f=format(bits),exponent=Math.floor(magnitude(a.n,a.d)/2),unit=exponent-f.p;
  const n=unit<0?a.n<<BigInt(-2*unit):a.n,d=unit>0?a.d<<BigInt(2*unit):a.d;
  let q=isqrt(n/d);const exact=q*q*d===n,rc=(csr>>13)&3,mid=(2n*q+1n)**2n*d;
  if(!exact&&(rc===2||rc===0&&(4n*n>mid||4n*n===mid&&!!(q&1n))))q++;
  return encoded(q,exponent,false,bits,a.flags|(exact?0:32),csr);
}
export function predicate(av,bv,bits,p,csr,comi=false) {
  const a=read(av,bits,csr),b=read(bv,bits,csr),unordered=a.nan||b.nan;
  const flags=unordered?(a.signaling||b.signaling||comi||[1,2,5,6].includes(p&7)?1:0):a.flags|b.flags;
  const order=unordered?null:compare(a,b);
  return {value:[order===0,order===-1,order!==null&&order<=0,unordered,order!==0,unordered||order>=0,unordered||order>0,!unordered][p&7],flags,order};
}
export function conversion(value,source,dest,csr) {
  const a=read(value,source,csr),f=format(dest);
  if(a.nan){let payload=a.fraction;if(a.p>f.p)payload>>=BigInt(a.p-f.p);else payload<<=BigInt(f.p-a.p);
    return {value:infinity(a.sign,dest)|payload|(1n<<BigInt(f.p-1)),flags:a.signaling?1:0};}
  if(a.infinity)return {value:infinity(a.sign,dest),flags:0};
  const result=round(a.n,a.d,a.sign,dest,csr);return {value:result.value,flags:result.flags|a.flags};
}
export function integer(value,source,bits,csr,trunc) {
  const a=read(value,source,csr);if(a.nan||a.infinity)return {value:1n<<BigInt(bits-1),flags:1};
  const negative=a.n<0n,n=negative?-a.n:a.n,d=a.d;let q=n/d;const r=n%d;
  if(increment(q,r,d,negative,trunc?(csr&~0x6000)|0x6000:csr))q++;
  if(negative)q=-q;
  if(q<-(1n<<BigInt(bits-1))||q>(1n<<BigInt(bits-1))-1n)return {value:1n<<BigInt(bits-1),flags:1};
  return {value:q&mask(bits),flags:r!==0n?32:0};
}

export function integral(value,bits,csr,control) {
  const a=read(value,bits,csr);
  if(a.nan)return {value:quiet(value,bits),flags:a.signaling?1:0};
  if(a.infinity)return {value,flags:0};
  if(a.zero)return {value:a.sign?1n<<BigInt(bits-1):0n,flags:0};
  const negative=a.n<0n,n=negative?-a.n:a.n;
  const rounding=control&4?csr:(csr&~0x6000)|((control&3)<<13);
  let q=n/a.d;const r=n%a.d;if(increment(q,r,a.d,negative,rounding))q++;
  const result=round(negative?-q:q,1n,a.sign,bits,csr);
  return {value:result.value,flags:r!==0n&&!(control&8)?32:0};
}

export function dot(a,b,bits,csr,control) {
  const products=a.map((value,lane)=>control&(1<<(lane+4))?arithmetic('mul',value,b[lane],bits,csr):{value:0n,flags:0});
  let flags=products.reduce((flags,result)=>flags|result.flags,0),mxcsr=reported(csr,flags),exception=!!(flags&~(csr>>7));
  const finish=value=>({values:Array.from({length:a.length},(_,lane)=>control&(1<<lane)?value:0n),mxcsr,exception});
  if(exception)return finish(0n);
  const add=(a,b)=>{const result=arithmetic('add',a,b,bits,mxcsr);flags=result.flags;exception=!!(flags&~(mxcsr>>7));mxcsr=reported(mxcsr,flags);return result.value;};
  let sum=add(products[0].value,products[1].value);
  if(exception)return finish(0n);
  if(bits===32){const upper=add(products[2].value,products[3].value);if(exception)return finish(0n);sum=add(sum,upper);}
  return finish(sum);
}
