import assert from 'node:assert/strict';
import {Exit} from '../web/runtime.mjs';
import {core,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';
import {mask,read,rounded,finish} from './x87-reference.mjs';
import {reference,bcdLoad,bcdStore} from './x87-math-reference.mjs';

let checks=0;
const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
const one=0x3fff8000000000000000n,indefinite=0xffffc000000000000000n;
const samples=[0n,1n<<79n,one,one|(1n<<79n),one+1n,0x40008000000000000000n,0x4000c000000000000000n,1n,(1n<<79n)|1n,0x00008000000000000000n,0x3fff0000000000000001n,0x7ffeffffffffffffffffn,0x00018000000000000000n,0x00018000000000000001n,0x7fff8000000000000000n,0xffff8000000000000000n,0x7fffc000000000000123n,0x7fff8000000000000321n];
const controls=[0x37f,0x37e,0x37d,0x37b,0x377,0x36f,0x35f,0x7f,0x27f,0x77f,0xb7f,0xf7f];
const integer80=n=>rounded(BigInt(n),1n,false,0x37f).value;
const seed=()=>({cw:0x37f,sw:0x6700,tags:0,regs:Array.from({length:8},(_,n)=>samples[n+2]|(BigInt(0x12340000+n)<<80n)),ip:0x12345678n,dp:0xabcdef01n,opcode:0x725});
const category=v=>{const x=read(v);return x.zero?1:x.unsupported||x.nan||x.infinity||x.denormal?2:0;};
function put(state,logical,value){const n=((state.sw>>11)+logical)&7;state.regs[n]=value&mask(80);state.tags=(state.tags&~(3<<(n*2)))|(category(value)<<(n*2));}
function pop(state){state.tags|=3<<(((state.sw>>11)&7)*2);state.sw=(state.sw&~0x3800)|((((state.sw>>11)+1)&7)<<11);}
function result(pointer){const v=new DataView(core.memory.buffer);return {value:v.getBigUint64(pointer,true)|(v.getBigUint64(pointer+8,true)<<64n),sw:v.getUint32(pointer+16,true),commit:!!v.getUint32(pointer+20,true)};}
function call(owner,kind,cw,sw,a,b=0n){const p=core[`wire_ext_${kind}`](owner,cw,a&mask(64),a>>64n,b&mask(64),b>>64n,sw),out=result(p);if(['sincos','tan'].includes(kind)){const v=new DataView(core.memory.buffer);out.pushed=v.getBigUint64(p+24,true)|(v.getBigUint64(p+32,true)<<64n);}return out;}
const trig=kind=>['sin','cos','sincos','tan'].includes(kind);
function expected(kind,cw,sw,a,b=0n){const raw=reference(kind,a,b,cw),out=finish(trig(kind)?sw&~0x400:sw,cw,raw);if(raw.c2)out.sw|=raw.c2;return out;}
const abs=n=>n<0n?-n:n;
function exponent(n,d){n=abs(n);let e=n.toString(2).length-d.toString(2).length;if(e>=0?n<(d<<BigInt(e)):(n<<BigInt(-e))<d)e--;return e;}
function close(actual,out,cw,label){
 if(out.exact||out.n===undefined||read(out.value).nan||read(out.value).infinity||out.flags&8&&cw&8){equal(actual,out.value,label);return;}
 let {n,d}=out;if(d<0n){n=-n;d=-d;}
 if(out.flags&8&&!(cw&8))d<<=24576n;
 if(out.flags&16&&!(cw&16))n<<=24576n;
 const a=read(actual),error=abs(a.n*d-n*a.d),denominator=a.d*d,e=Math.max(exponent(n,d)-63,-16445),nearest=((cw>>10)&3)===0;
 const left=error*(nearest?1n:2n),right=denominator*(nearest?1n:3n);
 assert.ok(e<0?(left<<BigInt(-e))<=right:left<=right<<BigInt(e),label+' accuracy target');checks++;
 equal(a.sign,read(out.value).sign,label+' sign');
}
function check(actual,out,cw,label){
 // C1 describes the implementation's final rounding. An accepted approximation
 // can choose a different adjacent result from the ideal rounded reference.
 const different=out.pushedRatio?actual.pushed!==out.pushed:actual.value!==out.value;
 const statusMask=!out.exact&&different?~0x200:0xffff;
 equal({sw:actual.sw&statusMask,commit:actual.commit},{sw:out.sw&statusMask,commit:out.commit},label);
 if(!out.commit)return;
 close(actual.value,out,cw,label);
 if(out.pushed!==undefined){if(out.pushedRatio)close(actual.pushed,{...out,...out.pushedRatio,value:out.pushed,flags:0},cw,label+' pushed');else equal(actual.pushed,out.pushed,label+' pushed');}
}

const more=[0x3ffe8000000000000000n,0xbffe8000000000000000n,0x3ffd8000000000000000n,0xbffd8000000000000000n,0x3fffc90fdaa22168c235n,0x4000c90fdaa22168c235n,0x403dffffffffffffffffn,0x403e8000000000000000n,0x403e8000000000000001n];
for(const e of [-34,-33,-32,-121,-120,-1000,-16382,-16445]){const v=rounded(1n,e<0?1n<<BigInt(-e):1n,false,0x37f).value;more.push(v,v+1n,v|(1n<<79n));}
const values=[...samples,...more],pairs=samples.flatMap(a=>samples.map(b=>[a,b]));
const ordinaryPairs=[];for(const a of [one+1n,...more.slice(0,6)])for(const b of [one,0x4000c000000000000000n,0xbffc8000000000000000n])ordinaryPairs.push([a,b]);
ordinaryPairs.push([samples[11],samples[11]],[1n,1n],[one+1n,1n],[1n,samples[11]],[samples[11],1n]);
const owner=core.wire_create(64,base,0n),state=core.wire_state(owner),before=new Uint8Array(core.memory.buffer,state,stateLayout[0]).slice();
for(const kind of ['exp','sin','cos','sincos','tan'])for(const cw of controls)for(const a of values){const sw=0x6700;check(call(owner,kind,cw,sw,a),expected(kind,cw,sw,a),cw,`${kind}/${cw}/${a.toString(16)}`);}
for(const kind of ['log','log1p','atan']){
 for(const cw of controls.slice(0,4))for(const [a,b] of pairs)check(call(owner,kind,cw,0x6700,a,b),expected(kind,cw,0x6700,a,b),cw,`${kind}/${cw}/${a.toString(16)}/${b.toString(16)}`);
 for(const cw of controls)for(const [a,b] of ordinaryPairs)check(call(owner,kind,cw,0x6700,a,b),expected(kind,cw,0x6700,a,b),cw,`${kind}/${cw}/${a.toString(16)}/${b.toString(16)}`);
}
equal(new Uint8Array(core.memory.buffer,state,stateLayout[0]),before,'all numerical entries preserve CPU backing');
core.wire_destroy(owner);

const bcdLimit=999999999999999999n;
const bcdInputs=[];for(const n of [0n,1n,123456789012345678n,bcdLimit])for(const sign of [0n,1n<<79n])for(const reserved of [0n,0x7fn<<72n])bcdInputs.push(BigInt('0x'+n.toString().padStart(18,'0'))|sign|reserved);
bcdInputs.push(0xabcdef01234567890123n,mask(80));
const bcdValues=[...samples];
for(const n of [bcdLimit,bcdLimit+1n,-bcdLimit,-bcdLimit-1n])for(const fraction of [-1n,0n,1n])bcdValues.push(rounded(n*2n+fraction,2n,false,0x37f).value);
const bcdOwner=core.wire_create(64,base,0n);
for(const cw of controls)for(const raw of bcdInputs){const out=finish(0x6700,cw,bcdLoad(raw)),actual=call(bcdOwner,'bcd_load',cw,0x6700,0n,raw);equal(actual,{value:out.value,sw:out.sw,commit:out.commit});}
for(const cw of controls)for(const value of bcdValues){const out=finish(0x6700,cw,bcdStore(value,cw)),actual=call(bcdOwner,'bcd_store',cw,0x6700,value);equal({sw:actual.sw,commit:actual.commit},{sw:out.sw,commit:out.commit});if(out.commit)equal(actual.value,out.value);}
core.wire_destroy(bcdOwner);

const specs=[['exp',0xf0],['sin',0xfe],['cos',0xff],['sincos',0xfb],['tan',0xf2],['atan',0xf3],['log',0xf1],['log1p',0xf9]].map(([kind,op])=>({kind,op,pair:['sincos','tan'].includes(kind),pop:['atan','log','log1p'].includes(kind)}));
const faultStatus=(before,overflow)=>((before.sw&~0x200)|0x41|(overflow?0x200:0)|(!(before.cw&1)?0x8080:0));
function architecture(test,before,spec,out,pc){
 const actual=fp(test),top=(before.sw>>11)&7,target=(top+(spec.pop?1:0))&7,pushed=(top+7)&7;
 const number={value:actual.regs[target],sw:(actual.sw&~0x3800)|(before.sw&0x3800),commit:out.commit};
 if(spec.pair)number.pushed=actual.regs[pushed];
 check(number,out,before.cw,`architecture/${spec.kind}/${before.cw}/${before.regs[top].toString(16)}`);
 const expectedState={...before,regs:[...before.regs],sw:out.sw,ip:pc,opcode:0x100|spec.op};
 const different=out.pushedRatio?number.pushed!==out.pushed:number.value!==out.value;
 if(!out.exact&&different)expectedState.sw=(expectedState.sw&~0x200)|(actual.sw&0x200);
 if(out.commit){put(expectedState,spec.pop?1:0,number.value);if(spec.pair){expectedState.sw=(expectedState.sw&~0x3800)|(pushed<<11);put(expectedState,0,number.pushed);}if(spec.pop)pop(expectedState);}
 equal(actual,expectedState,'numeric results commit with complete stack and metadata');
 return expectedState;
}
for(const mode of [32,64])for(const spec of specs){
 const test=program([0x04,1,0xd9,spec.op],mode),{vm}=test;
 const cases=spec.pop?[...pairs.filter((_,n)=>n%7===0),...ordinaryPairs]:values.map(a=>[a,0n]);
 for(const cw of [0x37f,0x37e,0x37d,0x35f,0x36f])for(let n=0;n<cases.length;n+=3){const [a,b]=cases[n],before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);before.regs[top]=a;if(spec.pop)before.regs[(top+1)&7]=b;if(spec.pair)before.tags|=3<<(((top+7)&7)*2);fp(test,before);vm.ip=base;vm.register(0,127n);vm.flags=0x602n;const out=expected(spec.kind,cw,before.sw,a,b);assert.equal((await vm.run(2n)).reason,Exit.Budget);architecture(test,before,spec,out,base+2n);equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f80);}
 vm.dispose();
}
for(const mode of [32,64])for(const spec of specs){
 const test=program([0xd9,spec.op],mode),{vm}=test;
 for(const top of [0,3,7])for(const cw of [0x37f,0x37e])for(const pattern of [1,2,3]){
  if(!spec.pop&&!spec.pair&&pattern!==1)continue;
  const before=seed(),next=(top+7)&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);before.regs[top]=one;before.regs[(top+1)&7]=one;if(spec.pair){before.tags|=3<<(next*2);if(pattern&2)before.tags&=~(3<<(next*2));}else if(spec.pop&&pattern&2)before.tags|=3<<(((top+1)&7)*2);if(pattern&1)before.tags|=3<<(top*2);fp(test,before);vm.ip=base;
  const overflow=spec.pair&&!!(pattern&2)&&!(pattern&1),expectedState={...before,regs:[...before.regs],sw:faultStatus(before,overflow)&(trig(spec.kind)?~0x400:0xffff),ip:base,opcode:0x100|spec.op};
  if(cw&1){put(expectedState,spec.pop?1:0,indefinite);if(spec.pair){expectedState.sw=(expectedState.sw&~0x3800)|(next<<11);put(expectedState,0,indefinite);}if(spec.pop)pop(expectedState);}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expectedState);assert.equal(test.calls.numeric,0);
 }vm.dispose();
}

function bytes(test,address,length){return Uint8Array.from({length},(_,n)=>Number(core.wire_load(test.handle,address+BigInt(n),1,1)));}
for(const mode of [32,64])for(const load of [true,false]){
 const code=[0xdf,load?0x23:0x33],test=program([0x04,1,...code],mode),{vm}=test;vm.map(data,8192n);
 const cases=load?bcdInputs:bcdValues;
 for(const cw of controls)for(let n=0;n<cases.length;n++){
  const before=seed(),top=n&7,address=data+(n&1?4093n:0n),raw=cases[n];before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);if(load)before.tags|=3<<(((top+7)&7)*2);else before.regs[top]=raw;fp(test,before);vm.ip=base;vm.register(0,127n);vm.flags=0x602n;vm.register(3,address);vm.write(address,load?encode(raw,10):new Uint8Array(10).fill(0xa5));const out=finish(before.sw,cw,load?bcdLoad(raw):bcdStore(raw,cw)),expectedState={...before,regs:[...before.regs],sw:out.sw,ip:base+2n,dp:address,opcode:0x700|code[1]};if(out.commit){if(load){expectedState.sw=(expectedState.sw&~0x3800)|(((top+7)&7)<<11);put(expectedState,0,out.value);}else pop(expectedState);}
  assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expectedState,`BCD/${mode}/${load}/${cw}/${n}`);if(!load)equal(bytes(test,address,10),out.commit?encode(out.value,10):new Uint8Array(10).fill(0xa5));assert.equal(vm.flags,0xe92n);
 }vm.dispose();
}
for(const load of [true,false])for(const cross of [false,true]){
 const code=[0xdf,load?0x23:0x33],test=program(code),{vm}=test,before=seed(),address=data+(cross?4093n:0n);if(load)before.tags=0xffff;else{before.cw=0x37e;before.regs[(before.sw>>11)&7]=samples[17];}fp(test,before);vm.map(data,8192n);vm.register(3,address);vm.write(address,new Uint8Array(10).fill(0xa5));const page=data+(cross?4096n:0n);assert.equal(core.wire_protect(test.handle,page,4096n,cross?(load?1:2):0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Fault);equal(fp(test),before);assert.equal(vm.work,0n);assert.equal(test.calls.numeric,0);assert.equal(core.wire_protect(test.handle,page,4096n,4),0);equal(bytes(test,address,10),new Uint8Array(10).fill(0xa5));assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.dispose();
}
for(const load of [true,false]){
 const test=program([0xdf,load?0x23:0x33]),{vm}=test,before=seed();before.tags=load?0xffff:0;fp(test,before);vm.map(data,8192n);vm.register(3,data+4086n);vm.write(data+4086n,encode(123n,10));assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal(core.wire_load(test.handle,data+4096n,1,1),0n);assert.equal(core.wire_fault_kind(test.handle),4,'BCD access is exactly ten bytes');vm.dispose();
}
for(const load of [true,false])for(const cw of [0x37f,0x37e]){
 const test=program([0xdf,load?0x23:0x33]),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.cw=cw;if(!load)before.tags|=3<<(top*2);fp(test,before);vm.map(data,4096n);vm.register(3,data);vm.write(data,new Uint8Array(10).fill(0xa5));const expectedState={...before,regs:[...before.regs],sw:faultStatus(before,load),ip:base,dp:data,opcode:0x700|(load?0x23:0x33)};if(cw&1){if(load){expectedState.sw=(expectedState.sw&~0x3800)|(((top+7)&7)<<11);put(expectedState,0,indefinite);}else pop(expectedState);}assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expectedState);if(!load)equal(bytes(test,data,10),cw&1?encode(indefinite,10):new Uint8Array(10).fill(0xa5));vm.dispose();
}

for(const spec of specs){
 const test=program([0x04,1,0xd9,spec.op]),{vm}=test,before=seed();before.cw=0x37e;before.sw|=1;fp(test,before);vm.register(0,127n);vm.flags=0x602n;assert.equal((await vm.run(3n)).reason,Exit.Exception);equal(fp(test),before);assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.numeric,0);vm.dispose();
}
for(const mode of [32,64])for(const load of [true,false]){
 const test=program([0xdf,load?0x23:0x33],mode),{vm}=test,before=seed();before.cw=0x37e;before.sw|=1;fp(test,before);vm.map(data,4096n);vm.register(3,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Exception);equal(fp(test),before,'BCD pending #MF preserves FP state');equal(vm.work,0n);equal(vm.ip,base);equal(test.calls.fp_area,0,'BCD pending #MF precedes memory preparation');equal(test.calls.numeric,0);core.wire_load(test.handle,data,1,1);equal(core.wire_fault_kind(test.handle),4,'BCD pending #MF preserves guard');vm.dispose();
}
for(const [kind,clear,a,b] of [['log',1,samples[3],one],['log',4,0n,one],['atan',32,one,one],['sincos',32,more[0],0n],['tan',1,samples[14],0n],['sin',2,1n,0n],['exp',16,1n,0n]]){
 const spec=specs.find(s=>s.kind===kind),test=program([0xd9,spec.op,0x04,1,0x9b]),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.cw=0x37f&~clear;before.regs[top]=a;if(spec.pop)before.regs[(top+1)&7]=b;if(spec.pair)before.tags|=3<<(((top+7)&7)*2);fp(test,before);vm.register(0,127n);const out=expected(kind,before.cw,before.sw,a,b);assert.equal((await vm.run(2n)).reason,Exit.Budget);const after=architecture(test,before,spec,out,base);assert.equal(vm.register(0),128n);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(vm.ip,base+4n);assert.equal(vm.work,2n);equal(fp(test),after);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xd9,0xee,0xd9,0xfb,0xff,0xe2],mode),{vm}=test,target=0x30000n;vm.map(data,4096n,7);vm.map(target,4096n);vm.write(data,[0xd9,0xf1,0xdf,0x33,0x83,0xe9,1,0x74,2,0xff,0xe6,0xcc]);vm.register(2,data);vm.register(6,base);vm.register(3,target);vm.register(1,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){const before=seed(),top=count&7;before.sw=(before.sw&~0x3800)|(top<<11);before.tags|=3<<(((top+7)&7)*2)|3<<(((top+6)&7)*2);fp(test,before);vm.ip=base;vm.register(1,BigInt(count));vm.flags=0x602n;test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>equal(fp(test),before,'math/BCD ordinary links preserve entry backing');assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const after={...before,regs:[...before.regs],sw:before.sw&~0x600,ip:data+2n,dp:target,opcode:0x733};put(after,7,0n);put(after,6,one);after.tags|=3<<(((top+7)&7)*2)|3<<(((top+6)&7)*2);equal(fp(test),after);equal(bytes(test,target,10),new Uint8Array(10));assert.equal(vm.flags,0x646n);assert.equal(test.calls.begin,1);assert.equal(test.calls.link,2);assert.deepEqual(vm.stats(),stats);}vm.dispose();
}

console.log(`${checks} x87 transcendental/BCD checks passed`);
