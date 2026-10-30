import {read,mask,rounded,binary} from './x87-reference.mjs';

const bits=320n,unit=1n<<bits,indefinite=0xffffc000000000000000n;
const decimal=s=>{const [a,b='']=s.split('.');return BigInt(a+b)*unit/(10n**BigInt(b.length));};
const pi=decimal('3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745028410270193852110555964462294895493038196');
const ln2=decimal('0.69314718055994530941723212145817656807550013436025525412068000949339362196969471560586332699641868754200148102057068573368552023575813055703267075163507596193072757082837143519030703862389167347112335');
const absolute=n=>n<0n?-n:n;
const inf=negative=>(negative?1n<<79n:0n)|0x7fff8000000000000000n;
const pre=(value,flags,cw)=>({value,flags,c1:0,commit:!(flags&~cw&7),exact:true});
function exponent(n,d){n=absolute(n);let e=n.toString(2).length-d.toString(2).length;if(e>=0?n<(d<<BigInt(e)):(n<<BigInt(-e))<d)e--;return e;}
function logarithm(n,d){
 const e=exponent(n,d);if(e>=0)d<<=BigInt(e);else n<<=BigInt(-e);
 const z=(n-d)*unit/(n+d),z2=z*z/unit;let power=z,sum=z;
 for(let k=1n;k<256n;k++){power=power*z2/unit;const term=power/(2n*k+1n);if(!term)break;sum+=term;}
 return {value:BigInt(e)*unit+2n*sum*unit/ln2,exact:n===d};
}
function finish(n,d,negativeZero,cw,force=false){
 if(d<0n){n=-n;d=-d;}
 const result=rounded(n,d,negativeZero,cw|0x300);
 if(force){result.flags|=32;if(n&&(cw&16)&&read(result.value).denormal)result.flags|=16;}
 return {...result,n,d,exact:!force};
}
function sineCosine(x){
 const value=x.n*unit/x.d,half=pi/2n;let q=value/half;
 const residual=value%half;if(absolute(residual)*2n>half||absolute(residual)*2n===half&&(q&1n))q+=value<0n?-1n:1n;
 const r=value-q*half,r2=r*r;let s=r,c=unit,st=r,ct=unit;
 for(let k=1n;k<100n;k++){
  st=-st*r2/(unit*unit*(2n*k)*(2n*k+1n));ct=-ct*r2/(unit*unit*(2n*k-1n)*(2n*k));
  if(!st&&!ct)break;s+=st;c+=ct;
 }
 switch(Number((q%4n+4n)%4n)){case 1:return [c,-s];case 2:return [-s,-c];case 3:return [-c,s];default:return [s,c];}
}
function atanSmall(n,d){
 if(exponent(n,d)<-80)return {n:n*(3n*d*d-n*n),d:3n*d*d*d};
 let z=n*unit/d,base=0n;if(z>unit/2n){base=pi/4n;z=(z-unit)*unit/(z+unit);}
 const squared=z*z/unit;let power=z,sum=z;
 for(let k=1n;k<256n;k++){power=-power*squared/unit;const term=power/(2n*k+1n);if(!term)break;sum+=term;}
 return {n:base+sum,d:unit};
}
export function reference(kind,av,bv,cw){
 const a=read(av),b=read(bv),unary=['exp','sin','cos','sincos','tan'].includes(kind),pair=kind==='sincos'||kind==='tan';
 const attach=r=>pair?{...r,pushed:kind==='tan'?0x3fff8000000000000000n:r.value}:r;
 if(a.unsupported||!unary&&b.unsupported)return attach(pre(indefinite,1,cw));
 if(a.nan||!unary&&b.nan)return attach({...binary('add',av,unary?0n:bv,cw),exact:true});
 let flags=a.denormal||!unary&&b.denormal?2:0;
 if(kind==='exp'){
  if(flags&~cw)return pre(av,flags,cw);
  if(a.infinity||a.n===0n||absolute(a.n)>a.d)return pre(av,flags,cw);
  if(a.n===a.d)return pre(0x3fff8000000000000000n,flags,cw);
  if(a.n===-a.d)return pre(0xbffe8000000000000000n,flags,cw);
  const z=a.n*ln2/a.d;let term=unit,sum=unit;
  for(let k=2n;k<200n;k++){term=term*z/(unit*k);if(!term)break;sum+=term;}
  const out=finish(a.n*ln2*sum,a.d*unit*unit,a.sign,cw,true);return {...out,flags:out.flags|flags};
 }
 if(['sin','cos','sincos','tan'].includes(kind)){
  if(a.infinity)return attach(pre(indefinite,1,cw));
  if(flags&~cw)return attach(pre(av,flags,cw));
  if(a.n&&exponent(a.n,a.d)>=63)return {value:av,flags:0,c1:0,commit:false,c2:0x400,exact:true};
  if(!a.n)return { ...pre(kind==='cos'?0x3fff8000000000000000n:av,flags,cw),...(pair?{pushed:0x3fff8000000000000000n}:{}) };
  let s,c,t;
  if(exponent(a.n,a.d)<-33){
   s={n:a.n*(6n*a.d*a.d-a.n*a.n),d:6n*a.d**3n};
   c={n:2n*a.d*a.d-a.n*a.n,d:2n*a.d*a.d};
   t={n:a.n*(3n*a.d*a.d+a.n*a.n),d:3n*a.d**3n};
  }else{const [sn,cn]=sineCosine(a);s={n:sn,d:unit};c={n:cn,d:unit};t={n:sn,d:cn};}
  const primary=kind==='cos'?c:kind==='tan'?t:s,result=finish(primary.n,primary.d,a.sign,cw,true);
  if(kind==='sincos'){
   const pushed=finish(c.n,c.d,false,cw,true);
   return {...result,flags:result.flags|pushed.flags|flags,c1:pushed.c1,pushed:pushed.value,pushedRatio:c};
  }
  return {...result,flags:result.flags|flags,...(kind==='tan'?{pushed:0x3fff8000000000000000n}:{})};
 }
 if(kind==='atan'){
  if(flags&~cw)return pre(bv,flags,cw);
  if((b.zero||a.infinity&&!b.infinity)&&!a.sign)return pre(b.sign?1n<<79n:0n,flags,cw);
  let angle;
  if(b.zero||a.infinity&&!b.infinity)angle={n:pi,d:unit};
  else if(a.zero||b.infinity&&!a.infinity)angle={n:pi,d:2n*unit};
  else if(a.infinity&&b.infinity)angle={n:pi*(a.sign?3n:1n),d:4n*unit};
  else{
   const swapped=absolute(b.n)*a.d>absolute(a.n)*b.d;
   const rn=swapped?absolute(a.n)*b.d:absolute(b.n)*a.d,rd=swapped?absolute(b.n)*a.d:absolute(a.n)*b.d;
   angle=atanSmall(rn,rd);
   if(swapped)angle={n:pi*angle.d-2n*unit*angle.n,d:2n*unit*angle.d};
   if(a.sign)angle={n:pi*angle.d-unit*angle.n,d:unit*angle.d};
  }
  const out=finish(b.sign?-angle.n:angle.n,angle.d,b.sign,cw,true);return {...out,flags:out.flags|flags};
 }
 const plus=kind==='log1p',domain=plus?a.n< -a.d:a.sign&&!a.zero,pole=plus?a.n===-a.d:a.zero,logZero=plus?a.zero:a.n===a.d;
 if(domain||pole&&b.zero||logZero&&b.infinity||a.infinity&&b.zero)return pre(indefinite,1,cw);
 const negativeLog=plus?a.sign:a.n<a.d,negative=b.sign!==negativeLog;
 if(pole&&!b.infinity)return pre(inf(negative),4,cw);
 if(flags&~cw)return pre(bv,flags,cw);
 if(a.infinity||b.infinity||pole)return pre(inf(negative),flags,cw);
 if(b.zero||logZero)return pre(negative?1n<<79n:0n,flags,cw);
 let out;
 if(plus&&absolute(a.n)*2n<=a.d){
  const z=a.n*unit/(2n*a.d+a.n),squared=z*z/unit;let power=unit,sum=unit;
  for(let k=1n;k<256n;k++){power=power*squared/unit;const term=power/(2n*k+1n);if(!term)break;sum+=term;}
  const factor=2n*a.d*sum/(2n*a.d+a.n);
  out=finish(a.n*factor*b.n,a.d*ln2*b.d,negative,cw,true);
 }else{
  const log=logarithm(a.n+(plus?a.d:0n),a.d);
  out=finish(log.value*b.n,unit*b.d,negative,cw,!log.exact);
 }
 return {...out,flags:out.flags|flags};
}
export function bcdLoad(bits){
 let n=0n;for(let k=17;k>=0;k--)n=n*10n+((bits>>BigInt(k*4))&15n);
 const negative=!!(bits&(1n<<79n));return {value:rounded(negative?-n:n,1n,negative,0x37f).value,flags:0,c1:0,commit:true};
}
export function bcdStore(bits,cw){
 const x=read(bits);if(x.unsupported||x.nan||x.infinity)return pre(indefinite,1,cw);
 let q=absolute(x.n)/x.d;const tail=absolute(x.n)%x.d,rc=(cw>>10)&3;
 const up=!!tail&&(rc===0?(2n*tail>x.d||2n*tail===x.d&&!!(q&1n)):rc===1?x.sign:rc===2?!x.sign:false);if(up)q++;
 if(q>999999999999999999n)return pre(indefinite,1,cw);
 const value=BigInt('0x'+q.toString().padStart(18,'0'))|(x.sign?1n<<79n:0n);
 return {value,flags:tail?32:0,c1:up?0x200:0,commit:true};
}
