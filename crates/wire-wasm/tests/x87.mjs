import assert from 'node:assert/strict';
import { Exit } from '../web/runtime.mjs';
import {core,layout,stateLayout,base,data,encode,program,fp} from './x87-harness.mjs';
import { mask,indefinite,read,rounded,binary,store,integer,fromFloat,finish,sqrt,comparison,roundIntegral,remainder,scale,extract } from './x87-reference.mjs';
let checks=0;const equal=(a,b,label)=>{assert.deepEqual(a,b,label);checks++;};
function result(pointer){const view=new DataView(core.memory.buffer);return {value:view.getBigUint64(pointer,true)|(view.getBigUint64(pointer+8,true)<<64n),sw:view.getUint32(pointer+16,true),commit:!!view.getUint32(pointer+20,true)};}
function number(owner,name,cw,sw,a=0n,b=0n){return result(core[name](owner,cw,a&mask(64),a>>64n,b&mask(64),b>>64n,sw));}
const owner=core.wire_create(64,base,0n),one=0x3fff8000000000000000n;
const samples=[0n,1n<<79n,one,one|(1n<<79n),one+1n,0x40008000000000000000n,0x4000c000000000000000n,1n,(1n<<79n)|1n,0x00008000000000000000n,0x3fff0000000000000001n,0x7ffeffffffffffffffffn,0x00018000000000000000n,0x00018000000000000001n,0x7fff8000000000000000n,0xffff8000000000000000n,0x7fffc000000000000123n,0x7fff8000000000000321n];
const controls=[];for(const pc of [0,1,2,3])for(let rc=0;rc<4;rc++)for(const clear of [0,1,2,4,8,16,32])controls.push(((0x7f|(pc<<8)|(rc<<10))&~clear));
const pairs=samples.map((v,n)=>[v,samples[(n*5+3)%samples.length]]);pairs.push([0n,0n],[samples[11],samples[5]],[samples[12],0x3ffe8000000000000000n],[samples[11],samples[11]],[samples[7],samples[11]],[one,one+1n]);
for(const kind of ['add','mul','sub','div'])for(const cw of controls)for(const [a,b] of pairs){
 const sw=0x6520,expected=finish(sw,cw,binary(kind,a,b,cw)),actual=number(owner,`wire_ext_${kind}`,cw,sw,a,b),label=`${kind}/${cw.toString(16)}/${a.toString(16)}/${b.toString(16)}`;
 equal({sw:actual.sw,commit:actual.commit},{sw:expected.sw,commit:expected.commit},label);if(expected.commit)equal(actual.value,expected.value,label);
}
for(const bits of [32,64])for(const cw of controls)for(const value of samples){const sw=0x6520,expected=finish(sw,cw,store(value,cw,bits)),actual=number(owner,`wire_ext_store${bits}`,cw,sw,value),label=`store${bits}/${cw.toString(16)}/${value.toString(16)}`;equal({sw:actual.sw,commit:actual.commit},{sw:expected.sw,commit:expected.commit},label);if(expected.commit)equal(actual.value,expected.value,label);}
for(const bits of [16,32,64])for(const trunc of [false,true])for(const cw of controls.filter((_,n)=>n%7===0||n%7===1||n%7===6))for(const value of samples){const sw=0x6520,expected=finish(sw,cw,integer(value,cw,bits,trunc)),actual=number(owner,`wire_ext_${trunc?'t':'i'}${bits}`,cw,sw,value),label=`int${bits}/${trunc}/${cw.toString(16)}/${value.toString(16)}`;equal({sw:actual.sw,commit:actual.commit},{sw:expected.sw,commit:expected.commit},label);if(expected.commit)equal(actual.value&mask(bits),expected.value,label);}
for(const cw of controls)for(const value of samples){const sw=0x6520,expected=finish(sw,cw,sqrt(value,cw)),actual=number(owner,'wire_ext_sqrt',cw,sw,value);equal({sw:actual.sw,commit:actual.commit},{sw:expected.sw,commit:expected.commit},`sqrt/${cw.toString(16)}/${value.toString(16)}`);if(expected.commit)equal(actual.value,expected.value);}
const newControls=[0x37f,0x37e,0x37d,0x37b,0x377,0x36f,0x35f,0x7f,0x27f,0x77f,0xb7f,0xf7f],integer80=n=>rounded(BigInt(n),1n,false,0x37f).value;
function remainderResult(sw,cw,a,b,nearest){const r=remainder(a,b,cw,nearest),out=finish(sw&~0x400,cw,r);if(out.commit&&r.cc!==undefined)out.sw=(out.sw&~(r.partial?0x400:0x4700))|r.cc;return out;}
const remainderPairs=samples.flatMap(a=>samples.map(b=>[a,b])),scalePairs=[...remainderPairs];
for(const a of [one,one+1n,samples[11],samples[12],1n,samples[13],samples[3],samples[11]|(1n<<79n)])for(const exponent of [0,1,-1,16383,16384,-16382,-16383,-16445,-16446,40959,40960,-40958,-40959,65536,-65536])scalePairs.push([a,integer80(exponent)]);
for(let q=0;q<16;q++)for(const signA of [0n,1n<<79n])for(const signB of [0n,1n<<79n])remainderPairs.push([integer80(q*2+1)|signA,samples[5]|signB]);
let residueSeed=0x3141592653589793n;
const significand=()=>{residueSeed=(residueSeed*6364136223846793005n+1442695040888963407n)&mask(64);return residueSeed|(1n<<63n);};
for(const delta of [-65,-2,-1,0,1,31,32,63,64,65,127,2048,16384,32700])for(const expB of [1,64,16383,32600]){const expA=expB+delta;if(expA<1||expA>32766)continue;for(const sign of [0n,1n<<79n])remainderPairs.push([(BigInt(expA)<<64n)|significand()|sign,(BigInt(expB)<<64n)|significand()]);}
for(const nearest of [false,true])for(const cw of newControls)for(const [a,b] of remainderPairs){const sw=0x6720,out=remainderResult(sw,cw,a,b,nearest),actual=number(owner,nearest?'wire_ext_rem1':'wire_ext_rem',cw,sw,a,b),label=`remainder/${nearest}/${cw}/${a.toString(16)}/${b.toString(16)}`;equal({sw:actual.sw,commit:actual.commit},{sw:out.sw,commit:out.commit},label);if(out.commit)equal(actual.value,out.value,label);}
for(const cw of newControls)for(const [a,b] of scalePairs){const sw=0x6720,out=finish(sw,cw,scale(a,b,cw)),actual=number(owner,'wire_ext_scale',cw,sw,a,b),label=`scale/${cw}/${a.toString(16)}/${b.toString(16)}`;equal({sw:actual.sw,commit:actual.commit},{sw:out.sw,commit:out.commit},label);if(out.commit)equal(actual.value,out.value,label);}
core.wire_destroy(owner);

const seed=()=>({cw:0x37f,sw:0x6500,tags:0,regs:Array.from({length:8},(_,n)=>samples[n+2]|(BigInt(0x12340000+n)<<80n)),ip:0x12345678n,dp:0xabcdef01n,opcode:0x725});
const category=value=>{const x=read(value);return x.zero?1:x.unsupported||x.nan||x.infinity||x.denormal?2:0;};
function put(state,logical,value){const physical=((state.sw>>11)+logical)&7;state.regs[physical]=value&mask(80);state.tags=(state.tags&~(3<<(physical*2)))|(category(value)<<(physical*2));}
function pop(state){state.tags|=3<<(((state.sw>>11)&7)*2);state.sw=(state.sw&~0x3800)|((((state.sw>>11)+1)&7)<<11);}
function imageRead(test,address,size){return Uint8Array.from({length:size},(_,n)=>Number(core.wire_load(test.handle,address+BigInt(n),1,1)));}

for(const mode of [32,64])for(const [kind,op,reverse] of [['add',0xc1],['mul',0xc9],['sub',0xe1],['div',0xf1],['sub',0xe9,true],['div',0xf9,true]])for(const dest of [false,true])for(const popping of [false,true]){
 if(!dest&&popping)continue;const bytes=[dest?(popping?0xde:0xdc):0xd8,dest&&kind==='sub'?(reverse?0xe1:0xe9):dest&&kind==='div'?(reverse?0xf1:0xf9):op],test=program([0x04,1,...bytes],mode),{vm}=test;
 for(let top=0;top<8;top++)for(const cw of [0x7f,0x27f,0x37f,0xf7f]){const before=seed();before.sw=(before.sw&~0x3800)|(top<<11);before.cw=cw;before.regs[top]=one+1n;before.regs[(top+1)&7]=0x4000c000000000000000n;fp(test,before);vm.ip=base;vm.register(0,127n);vm.flags=0x602n;vm.mxcsr=0x1f01;const a=before.regs[dest?(top+1)&7:top],b=before.regs[dest?top:(top+1)&7],out=finish(before.sw,cw,binary(kind,reverse?b:a,reverse?a:b,cw)),expected={...before,regs:[...before.regs],sw:out.sw,ip:base+2n,opcode:((bytes[0]&7)<<8)|bytes[1]};if(out.commit){put(expected,dest?1:0,out.value);if(popping)pop(expected);}
 assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expected,`${mode}/${kind}/${reverse}/${dest}/${popping}/${top}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f01);}
 assert.equal(test.calls.numeric,32);vm.dispose();
}

const floatInputs=bits=>bits===32?[0n,0x80000000n,0x3fc00000n,0xc0200000n,1n,0x7f800000n,0x7fc00123n,0x7f800123n]:[0n,0x8000000000000000n,0x3ff8000000000000n,0xc004000000000000n,1n,0x7ff0000000000000n,0x7ff8000000000123n,0x7ff0000000000123n];
function stackFault(before,overflow,cw=before.cw){return (before.sw&~0x200)|0x41|(overflow?0x200:0)|(!(cw&1)?0x8080:0);}
for(const mode of [32,64])for(const size of [4,8,10]){
 const code=[size===4?0xd9:size===8?0xdd:0xdb,size===10?0x2b:0x03],test=program(code,mode),{vm}=test;vm.map(data,8192n);
 for(const cw of [0x37f,0x37e,0x37d])for(let n=0;n<(size===10?samples.length:8);n++){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);before.tags=0xffff;fp(test,before);vm.ip=base;const address=data+BigInt(n%2?4093:0),input=size===10?samples[n]:floatInputs(size*8)[n];vm.register(3,address);vm.write(address,encode(input,size));const expected={...before,regs:[...before.regs],ip:base,dp:address,opcode:((code[0]&7)<<8)|code[1]};let value=input,flags=0;if(size!==10){const converted=fromFloat(input,size*8);value=converted.value;if(converted.flags&1)value|=1n<<62n;flags=converted.flags;}const out=finish(before.sw,cw,{value,flags,c1:0,commit:!(flags&~cw&1)});expected.sw=out.sw;if(out.commit){expected.sw=(expected.sw&~0x3800)|(((top+7)&7)<<11);put(expected,0,value);}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`fld/${mode}/${size}/${cw}/${n}`);assert.equal(vm.mxcsr,0x1f80);
 }vm.dispose();
}
for(const mode of [32,64])for(const size of [2,4,8]){
 const code=[size===4?0xdb:0xdf,size===8?0x2b:0x03],test=program(code,mode),{vm}=test;vm.map(data,8192n);for(const value of [0n,-1n,1n,-(1n<<BigInt(size*8-1)),(1n<<BigInt(size*8-1))-1n]){const before=seed();before.tags=0xffff;before.cw=0x7f;fp(test,before);vm.ip=base;vm.register(3,data+4095n);vm.write(data+4095n,encode(value,size));const expected={...before,regs:[...before.regs],sw:(before.sw&~0x3a00)|((((before.sw>>11)+7)&7)<<11),ip:base,dp:data+4095n,opcode:((code[0]&7)<<8)|code[1]};put(expected,0,rounded(value,1n,false,0x37f).value);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected);}vm.dispose();
}
for(const mode of [32,64])for(const [code,size,int,popping,trunc] of [[[0xd9,0x13],4,false,false],[[0xdd,0x13],8,false,false],[[0xd9,0x1b],4,false,true],[[0xdd,0x1b],8,false,true],[[0xdb,0x3b],10,false,true],[[0xdf,0x13],2,true,false],[[0xdb,0x13],4,true,false],[[0xdf,0x1b],2,true,true],[[0xdb,0x1b],4,true,true],[[0xdf,0x3b],8,true,true],[[0xdf,0x0b],2,true,true,true],[[0xdb,0x0b],4,true,true,true],[[0xdd,0x0b],8,true,true,true]]){
 const test=program(code,mode),{vm}=test;vm.map(data,8192n);
 for(const cw of [0x37f,0x77f,0x37e,0x377,0x36f,0x35f])for(let n=0;n<samples.length;n++){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);before.regs[top]=samples[n];fp(test,before);vm.ip=base;const address=data+BigInt(n%2?4093:0);vm.register(3,address);vm.write(address,new Uint8Array(size).fill(0xa5));const expected={...before,regs:[...before.regs],ip:base,dp:address,opcode:((code[0]&7)<<8)|code[1]},out=finish(before.sw,cw,size===10?{value:samples[n],flags:0,c1:0,commit:true}:int?integer(samples[n],cw,size*8,!!trunc):store(samples[n],cw,size*8));expected.sw=out.sw;if(out.commit&&popping)pop(expected);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`store/${mode}/${size}/${int}/${cw}/${n}`);equal(imageRead(test,address,size),out.commit?encode(out.value,size):new Uint8Array(size).fill(0xa5));
 }vm.dispose();
}

for(const mode of [32,64])for(let index=0;index<8;index++)for(const load of [false,true]){
 const test=program([0xd9,(load?0xc0:0xc8)+index],mode),{vm}=test;
 for(let top=0;top<8;top++)for(const pattern of [0,1,2,3])for(const cw of [0x37f,0x37e]){
  const before=seed();before.sw=(before.sw&~0x3800)|(top<<11);before.cw=cw;const other=(top+index)&7,dest=(top+7)&7;before.tags=0;if(load)before.tags|=3<<(dest*2);if(pattern&1)before.tags|=3<<(top*2);if(pattern&2)before.tags|=3<<(other*2);if(load&&pattern===3)before.tags&=~(3<<(dest*2));fp(test,before);vm.ip=base;const full=load&&(before.tags>>(dest*2)&3)!==3,empty=(before.tags>>(other*2)&3)===3||!load&&(before.tags>>(top*2)&3)===3,fault=full||empty,expected={...before,regs:[...before.regs],sw:fault?stackFault(before,full):before.sw&~0x200,ip:base,opcode:0x100|((load?0xc0:0xc8)+index)};
  if(!fault||cw&1){if(load){expected.sw=(expected.sw&~0x3800)|(dest<<11);put(expected,0,fault?indefinite:before.regs[other]);}else{const a=(before.tags>>(top*2)&3)===3?indefinite:before.regs[top],b=(before.tags>>(other*2)&3)===3?indefinite:before.regs[other];put(expected,0,b);put(expected,index,a);}}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`${load?'load':'exchange'}/${mode}/${index}/${top}/${pattern}/${cw}`);
 }assert.equal(test.calls.numeric,0);vm.dispose();
}
for(const [code,clear] of [[[0xd9,0xe0],false],[[0xd9,0xe1],true]]){const test=program(code),{vm}=test;for(const value of samples){const before=seed();before.regs[(before.sw>>11)&7]=value;fp(test,before);vm.ip=base;const expected={...before,regs:[...before.regs],sw:before.sw&~0x200,ip:base,opcode:0x100|code[1]};put(expected,0,clear?value&~(1n<<79n):value^(1n<<79n));assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected);}vm.dispose();}

for(const mode of [32,64]){const test=program([0xd9,0xfa],mode),{vm}=test;for(const cw of [0x7f,0x27f,0x37f,0xf7f,0x37e,0x37d,0x35f])for(let n=0;n<samples.length;n++){const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11);before.regs[top]=samples[n];fp(test,before);vm.ip=base;const out=finish(before.sw,cw,sqrt(samples[n],cw)),expected={...before,regs:[...before.regs],sw:out.sw,ip:base,opcode:0x1fa};if(out.commit)put(expected,0,out.value);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`sqrt instruction/${mode}/${cw}/${n}`);}vm.dispose();}

function changed(before,code,out,address){const expected={...before,regs:[...before.regs],sw:out.sw,ip:base,opcode:((code[0]&7)<<8)|code[1]};if(address!==undefined)expected.dp=address;return expected;}
function memoryNumber(raw,size,int){if(!int)return fromFloat(raw,size*8);const value=BigInt.asIntN(size*8,raw);return {value:rounded(value,1n,false,0x37f).value,flags:0};}
const arithmetic=[['add',0],['mul',1],['sub',4],['sub',5],['div',6],['div',7]];
for(const mode of [32,64])for(const [size,int,opcode] of [[4,false,0xd8],[8,false,0xdc],[2,true,0xde],[4,true,0xda]])for(const [kind,selector] of arithmetic){
 const code=[opcode,(selector<<3)|3],test=program(code,mode),{vm}=test;vm.map(data,8192n);
 const inputs=int?[0n,1n,mask(size*8),1n<<BigInt(size*8-1),12345n]:floatInputs(size*8);
 for(const cw of [0x7f,0x27f,0x37f,0x37e,0x37d,0x37b])for(let n=0;n<inputs.length;n++){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11);before.regs[top]=samples[(n+3)%samples.length];fp(test,before);vm.ip=base;const address=data+(n&1?4095n:0n);vm.register(3,address);vm.write(address,encode(inputs[n],size));const source=memoryNumber(inputs[n],size,int),reverse=selector===5||selector===7,a=reverse?source.value:before.regs[top],b=reverse?before.regs[top]:source.value,out=finish(before.sw,cw,binary(kind,a,b,cw,!!(source.flags&2))),expected=changed(before,code,out,address);if(out.commit)put(expected,0,out.value);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`memory ${kind}/${selector}/${mode}/${size}/${int}/${cw}/${n}`);assert.equal(vm.work,vm.retired);
 }vm.dispose();
}
for(const mode of [32,64])for(const [code,quiet,popping,zero] of [[[0xd8,0xd1],false,0],[[0xd8,0xd9],false,1],[[0xde,0xd9],false,2],[[0xdd,0xe1],true,0],[[0xdd,0xe9],true,1],[[0xda,0xe9],true,2],[[0xd9,0xe4],false,0,true]]){
 const test=program(code,mode),{vm}=test;
 for(const cw of [0x37f,0x37e,0x37d])for(let n=0;n<samples.length;n++)for(const empty of [false,true]){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11);before.regs[top]=samples[n];before.regs[(top+1)&7]=samples[(n*7+3)%samples.length];if(empty)before.tags|=3<<(top*2);fp(test,before);vm.ip=base;vm.flags=0x602n;
  const out=finish(before.sw,cw,comparison(before.regs[top],zero?0n:before.regs[(top+1)&7],cw,quiet)),expected=changed(before,code,out);let commit=out.commit;if(empty){expected.sw=stackFault(before,false)|(cw&1?0x4500:0);commit=!!(cw&1);}else if(commit)expected.sw=(expected.sw&~0x4500)|out.cc;if(commit)for(let p=0;p<popping;p++)pop(expected);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`compare/${mode}/${code}/${cw}/${n}/${empty}`);assert.equal(vm.flags,0x602n);
 }vm.dispose();
}
for(const mode of [32,64])for(const [size,int,opcode] of [[4,false,0xd8],[8,false,0xdc],[2,true,0xde],[4,true,0xda]])for(const popping of [false,true]){
 const code=[opcode,popping?0x1b:0x13],test=program(code,mode),{vm}=test;vm.map(data,8192n);
 for(const cw of [0x37f,0x37e,0x37d])for(let n=0;n<8;n++){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11);before.regs[top]=samples[n*2];fp(test,before);vm.ip=base;const raw=int?[0n,1n,mask(size*8),1n<<BigInt(size*8-1)][n%4]:floatInputs(size*8)[n],address=data+4095n;vm.register(3,address);vm.write(address,encode(raw,size));const source=memoryNumber(raw,size,int),out=finish(before.sw,cw,comparison(before.regs[top],source.value,cw,false,!!(source.flags&2))),expected=changed(before,code,out,address);if(out.commit){expected.sw=(expected.sw&~0x4500)|out.cc;if(popping)pop(expected);}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`memory compare/${mode}/${size}/${int}/${popping}/${cw}/${n}`);
 }vm.dispose();
}
for(const mode of [32,64])for(const popping of [false,true])for(const index of [0,1,7]){
 const code=[0xdd,(popping?0xd8:0xd0)+index],test=program(code,mode),{vm}=test;
 for(let top=0;top<8;top++)for(const empty of [false,true])for(const cw of [0x37f,0x37e]){
  const before=seed();before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11);if(empty)before.tags|=3<<(top*2);fp(test,before);vm.ip=base;const expected=changed(before,code,{sw:empty?stackFault(before,false):before.sw&~0x200});if(!empty||cw&1){put(expected,index,empty?indefinite:before.regs[top]);if(popping)pop(expected);}
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`register store/${mode}/${popping}/${index}/${top}/${empty}/${cw}`);
 }vm.dispose();
}
for(const [code,size,int] of [[[0xd9,0x1b],4,false],[[0xdd,0x1b],8,false],[[0xdb,0x3b],10,false],[[0xdf,0x1b],2,true],[[0xdb,0x1b],4,true],[[0xdf,0x3b],8,true]]){
 const test=program(code),{vm}=test;vm.map(data,4096n);vm.register(3,data);
 for(const cw of [0x37f,0x37e]){const before=seed();before.cw=cw;before.tags=0xffff;fp(test,before);vm.ip=base;vm.write(data,new Uint8Array(size).fill(0xa5));const expected=changed(before,code,{sw:stackFault(before,false)},data);if(cw&1)pop(expected);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected);equal(imageRead(test,data,size),cw&1?encode(int?1n<<BigInt(size*8-1):size===4?0xffc00000n:size===8?0xfff8000000000000n:indefinite,size):new Uint8Array(size).fill(0xa5));}vm.dispose();
}
// High precision decimal ratios exercise RC independently of the loader table.
const constants=['1','3.321928094887362347870319429489390175864831393024580612054756','1.442695040888963407359924681001892137426645954152985934135449','3.141592653589793238462643383279502884197169399375105820974944','0.301029995663981195213738894724493026768189881462108541310427','0.693147180559945309417232121458176568075500134360255254120680','0'];
for(let index=0;index<constants.length;index++){
 const code=[0xd9,0xe8+index],test=program(code),{vm}=test,[whole,fraction='']=constants[index].split('.'),n=BigInt(whole+fraction),d=10n**BigInt(fraction.length);
 for(const pc of [0,1,2,3])for(let rc=0;rc<4;rc++){const before=seed();before.cw=0x7f|(pc<<8)|(rc<<10);before.tags=0xffff;fp(test,before);vm.ip=base;const expected=changed(before,code,{sw:before.sw&~0x3a00});expected.sw|=(((before.sw>>11)+7)&7)<<11;put(expected,0,rounded(n,d,false,0x37f|(rc<<10)).value);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`constant/${index}/${pc}/${rc}`);}vm.dispose();
}

for(const mode of [32,64])for(const [code,kind,a,b,clear] of [[[0xde,0xf9],'div',0n,0n,1],[[0xde,0xc1],'add',one,1n,2],[[0xde,0xf9],'div',one,0n,4],[[0xde,0xc9],'mul',samples[11],samples[5],8],[[0xde,0xc9],'mul',samples[12],0x3ffe8000000000000000n,16],[[0xde,0xc1],'add',one,one+1n,32]])for(const waiting of [[0x9b],[0x0f,0x6f,0xc1]]){
 const test=program([...code,0x04,1,...waiting],mode),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.cw=0x37f&~clear;before.sw&=~0x20;before.regs[top]=b;before.regs[(top+1)&7]=a;fp(test,before);vm.register(0,127n);const out=finish(before.sw,before.cw,binary(kind,a,b,before.cw)),expected=changed(before,code,out);if(out.commit){put(expected,1,out.value);pop(expected);}
 assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expected,`deferred/${mode}/${kind}/${clear}`);assert.equal(vm.register(0),128n);assert.equal(vm.flags,0xa92n);assert.equal(vm.ip,base+4n);assert.equal(vm.work,2n);assert.equal(vm.retired,2n);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(core.wire_exception(test.handle),17);assert.equal(vm.ip,base+4n);assert.equal(vm.work,2n);equal(fp(test),expected);vm.dispose();
}
const memoryForms=[[[0xd9,0x03],4,false,true],[[0xdd,0x03],8,false,true],[[0xdb,0x2b],10,false,true],[[0xdf,0x03],2,false,true],[[0xd8,0x03],4,false,false],[[0xdc,0x3b],8,false,false],[[0xda,0x13],4,false,false],[[0xd9,0x1b],4,true,false],[[0xdd,0x1b],8,true,false],[[0xdb,0x3b],10,true,false],[[0xdf,0x1b],2,true,false]];
for(const mode of [32,64])for(const [code,size,write,push] of memoryForms)for(const cross of [false,true]){
 const test=program([0x04,1,...code],mode),{vm}=test,before=seed();if(push)before.tags=0xffff;fp(test,before);vm.register(0,127n);vm.flags=0x602n;vm.map(data,8192n);const address=data+(cross?4095n:0n),input=encode(one,size);vm.write(address,input);vm.register(3,address);const page=data+(cross?4096n:0n);assert.equal(core.wire_protect(test.handle,page,4096n,cross?(write?2:1):0x104),0);
 assert.equal((await vm.run(10n)).reason,Exit.Fault);equal(fp(test),before,'failed memory operands preserve complete FP state');assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.numeric,0);assert.equal(core.wire_protect(test.handle,page,4096n,4),0);equal(imageRead(test,address,size),input,'failed stores leave every byte intact');assert.equal((await vm.run(1n)).reason,Exit.Budget);vm.dispose();
}
for(const [code,size,write] of [[[0xdb,0x2b],10,false],[[0xdb,0x3b],10,true],[[0xd9,0x03],4,false],[[0xdd,0x1b],8,true]]){
 const test=program(code),{vm}=test,before=seed();if(!write)before.tags=0xffff;fp(test,before);vm.map(data,8192n);vm.register(3,data+4096n-BigInt(size));vm.write(data+4096n-BigInt(size),encode(one,size));assert.equal(core.wire_protect(test.handle,data+4096n,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Budget,'operand does not widen into the adjacent guard');assert.equal(core.wire_load(test.handle,data+4096n,1,1),0n);assert.equal(core.wire_fault_kind(test.handle),4);vm.dispose();
}
for(const [code,size] of [[[0xd9,0x03],4],[[0xdb,0x2b],10],[[0xdd,0x1b],8]]){
 const test=program(code),{vm}=test,before=seed();before.cw=0x37e;before.sw|=1;fp(test,before);vm.map(data,4096n);vm.register(3,data);assert.equal(core.wire_protect(test.handle,data,4096n,0x104),0);assert.equal((await vm.run(1n)).reason,Exit.Exception);equal(fp(test),before);assert.equal(core.wire_exception(test.handle),17);assert.equal(test.calls.resolve+test.calls.fp_area+test.calls.numeric,0);before.cw|=1;fp(test,before);assert.equal((await vm.run(1n)).reason,Exit.Fault,'pending #MF does not consume the memory guard');assert.equal(core.wire_fault_kind(test.handle),4);vm.dispose();
}
for(const [code,size] of [[[0xd9,0x1b],4],[[0xdd,0x1b],8],[[0xdb,0x3b],10]]){
 const test=program([...code,0x04,1]),{vm}=test,before=seed();before.regs[(before.sw>>11)&7]=one;fp(test,before);vm.register(0,123n);vm.register(3,base+2n);assert.equal(core.wire_compile(test.handle),0);const ticket=core.wire_ticket(test.handle);assert.equal((await vm.run(1n)).reason,Exit.Budget);const expected=changed(before,code,{sw:before.sw&~0x200},base+2n);pop(expected);equal(fp(test),expected);equal(imageRead(test,base+2n,size),encode(size===10?one:store(one,before.cw,size*8).value,size));assert.equal(vm.register(0),123n);assert.equal(vm.work,1n);assert.equal(vm.ip,base+2n);assert.equal(core.wire_ready(test.handle,ticket),0);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xd8,0xc1,0xff,0xe2],mode),{vm}=test;vm.map(data,4096n,7);vm.write(data,[0xd9,0xc0,0xde,0xc1,0x83,0xe9,1,0x74,2,0xff,0xe6,0xcc]);vm.register(2,data);vm.register(6,base);vm.register(1,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){const before=seed(),top=(before.sw>>11)&7;before.tags=3<<(((top+7)&7)*2);before.regs[top]=one;before.regs[(top+1)&7]=one;fp(test,before);vm.ip=base;vm.register(1,BigInt(count));test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>equal(fp(test),before,'ordinary links keep FP backing at entry state');assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const expected={...before,regs:[...before.regs],ip:data+2n,opcode:0x6c1};let value=1n;for(let n=0;n<count;n++){value=(value+1n)*2n;put(expected,0,rounded(value,1n,false,before.cw).value);put(expected,7,rounded(value/2n,1n,false,before.cw).value);expected.tags|=3<<(((top+7)&7)*2);}equal(fp(test),expected);assert.equal(test.calls.begin,1);assert.equal(test.calls.link,2);assert.deepEqual(vm.stats(),stats);}
 vm.dispose();
}
const integerFlags=[{code:[0x04,1],al:127n,flags:0xe92n},{code:[0x2c,1],al:0n,flags:0x697n},{code:[0x30,0xc0],al:255n,flags:0x646n}];
for(const mode of [32,64])for(const quiet of [false,true])for(const popping of [false,true])for(const index of [0,1,7])for(const input of integerFlags){
 const code=[popping?0xdf:0xdb,(quiet?0xe8:0xf0)+index],test=program([...input.code,...code],mode),{vm}=test;
 for(const cw of [0x37f,0x37e,0x37d])for(let n=0;n<samples.length;n++)for(const pattern of [0,1,2]){
  const before=seed(),top=n&7,other=(top+index)&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11)|0x200;before.regs[top]=samples[n];before.regs[other]=samples[(n*7+3)%samples.length];if(pattern&1)before.tags|=3<<(top*2);if(pattern&2)before.tags|=3<<(other*2);fp(test,before);vm.ip=base;vm.flags=0x602n;vm.register(0,input.al);const empty=!!pattern,out=finish(before.sw,cw,comparison(before.regs[top],before.regs[other],cw,quiet,false,false)),expected=changed(before,code,out);expected.ip=base+2n;let commit=out.commit,cc=out.cc;if(empty){expected.sw=stackFault(before,false);commit=!!(cw&1);cc=0x4500;}if(commit&&popping)pop(expected);const flags=(input.flags&~(commit?0x8d5n:0x890n))|(commit?BigInt(cc>>8):0n);
  assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expected,`fcomi/${mode}/${quiet}/${popping}/${index}/${input.flags}/${cw}/${n}/${pattern}`);equal(vm.flags,flags);assert.equal(vm.mxcsr,0x1f80);
 }vm.dispose();
}
const moves=[{op:0xda,group:0,test:f=>!!(f&1)},{op:0xda,group:1,test:f=>!!(f&64)},{op:0xda,group:2,test:f=>!!(f&65)},{op:0xda,group:3,test:f=>!!(f&4)},{op:0xdb,group:0,test:f=>!(f&1)},{op:0xdb,group:1,test:f=>!(f&64)},{op:0xdb,group:2,test:f=>!(f&65)},{op:0xdb,group:3,test:f=>!(f&4)}];
for(const mode of [32,64])for(const move of moves)for(const index of [0,1,7]){
 const code=[move.op,0xc0+(move.group<<3)+index],test=program(code,mode),{vm}=test;
 for(const top of [0,3,7])for(let f=0;f<8;f++)for(const cw of [0x37f,0x37e])for(const pattern of [0,1,2,3]){
  const before=seed(),other=(top+index)&7,flags=0xe92|((f&1)?1:0)|((f&2)?4:0)|((f&4)?64:0);before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11)|0x200;before.regs[other]=samples[(f*5+top+index)%samples.length]|(0x123456n<<80n);if(pattern&1)before.tags|=3<<(top*2);if(pattern&2)before.tags|=3<<(other*2);fp(test,before);vm.ip=base;vm.flags=BigInt(flags);const expected=changed(before,code,{sw:pattern?stackFault(before,false):before.sw});if(pattern&&cw&1)put(expected,0,indefinite);else if(!pattern&&move.test(flags))put(expected,0,before.regs[other]);
  assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`fcmov/${mode}/${move.op}/${move.group}/${index}/${top}/${flags}/${cw}/${pattern}`);equal(vm.flags,BigInt(flags));
 }
 assert.equal(test.calls.numeric,0);vm.dispose();
}
for(const mode of [32,64])for(const move of moves)for(const input of [...integerFlags,{code:[0x04,1,0xf9],al:127n,flags:0xe93n}]){
 const code=[move.op,0xc1+(move.group<<3)],test=program([...input.code,...code],mode),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.regs[top]=one;before.regs[(top+1)&7]=samples[17];fp(test,before);vm.register(0,input.al);vm.flags=0x602n;const expected=changed(before,code,{sw:before.sw});expected.ip=base+BigInt(input.code.length);if(move.test(Number(input.flags)))put(expected,0,before.regs[(top+1)&7]);assert.equal((await vm.run(BigInt(input.code.length===3?3:2))).reason,Exit.Budget);equal(fp(test),expected);equal(vm.flags,input.flags);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xd9,0xe5],mode),{vm}=test;
 for(let n=0;n<samples.length;n++)for(const tagged of [0,1,2,3])for(const cw of [0x37f,0x37e,0x37d]){
  const before=seed(),top=n&7,value=samples[n],x=read(value);before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11)|0x200;before.tags=(before.tags&~(3<<(top*2)))|(tagged<<(top*2));before.regs[top]=value|(0xabcdefn<<80n);fp(test,before);vm.ip=base;vm.flags=0xed7n;const cc=tagged===3?0x4100:x.unsupported?0:x.nan?0x100:x.infinity?0x500:x.zero?0x4000:x.denormal?0x4400:0x400,expected=changed(before,[0xd9,0xe5],{sw:(before.sw&~0x4700)|cc|(x.sign?0x200:0)});assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`fxam/${mode}/${n}/${tagged}/${cw}`);assert.equal(vm.flags,0xed7n);
 }assert.equal(test.calls.numeric,0);vm.dispose();
}
const integralInputs=[...samples,0x3fffc000000000000000n,0xbfffc000000000000000n,0x4000a000000000000000n,0xc000a000000000000000n,0x403e8000000000000001n,0x403f8000000000000000n,0x43ff8000000000000000n];
for(const mode of [32,64]){
 const test=program([0xd9,0xfc],mode),{vm}=test;
 for(const cw of controls)for(let n=0;n<integralInputs.length;n++)for(const empty of [false,true]){
  const before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11);before.regs[top]=integralInputs[n];if(empty)before.tags|=3<<(top*2);fp(test,before);vm.ip=base;const out=finish(before.sw,cw,roundIntegral(integralInputs[n],cw)),expected=changed(before,[0xd9,0xfc],out);if(empty){expected.sw=stackFault(before,false);if(cw&1)put(expected,0,indefinite);}else if(out.commit)put(expected,0,out.value);assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`frndint/${mode}/${cw}/${n}/${empty}`);
 }vm.dispose();
}
for(const code of [[0xdb,0xf1],[0xdf,0xe9],[0xda,0xc1],[0xd9,0xe5],[0xd9,0xfc]]){
 const test=program([0x04,1,...code]),{vm}=test,before=seed();before.cw=0x37e;before.sw|=1;fp(test,before);vm.register(0,127n);vm.flags=0x602n;assert.equal((await vm.run(10n)).reason,Exit.Exception);equal(fp(test),before,'pending #MF precedes classification, conditions and FLAGS changes');assert.equal(vm.ip,base+2n);assert.equal(vm.work,1n);assert.equal(vm.retired,1n);assert.equal(vm.flags,0xe92n);assert.equal(test.calls.numeric,0);vm.dispose();
}
for(const code of [[0xdb,0xf1],[0xdf,0xe9],[0xd9,0xfc]]){
 const test=program([...code,0x14,0,0x9b]),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.cw=0x37e;before.regs[top]=samples[17];before.regs[(top+1)&7]=one;fp(test,before);vm.register(0,0n);vm.flags=0xed7n;assert.equal((await vm.run(2n)).reason,Exit.Budget);assert.equal(vm.register(0),1n,'integer carry consumer follows deferred unmasked x87 exception');const after=fp(test);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(vm.ip,base+4n);assert.equal(vm.work,2n);equal(fp(test),after);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0x04,1,0xff,0xe2],mode),{vm}=test;vm.map(data,4096n,7);vm.write(data,[0xdb,0xf1,0xda,0xc2,0x14,0,0x83,0xe9,1,0x74,2,0xff,0xe6,0xcc]);vm.register(2,data);vm.register(6,base);vm.register(1,2n);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats();
 for(const count of [12,2]){const before=seed(),top=count&7;before.sw=(before.sw&~0x3800)|(top<<11);before.regs[top]=one;before.regs[(top+1)&7]=samples[5];before.regs[(top+2)&7]=samples[6];fp(test,before);vm.ip=base;vm.register(0,127n);vm.register(1,BigInt(count));vm.flags=0x602n;test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>equal(fp(test),before);assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);const expected={...before,regs:[...before.regs],sw:before.sw&~0x200,ip:data+2n,opcode:0x2c2};put(expected,0,samples[6]);equal(fp(test),expected);assert.equal(vm.register(0),128n+BigInt(count));assert.equal(vm.flags,0x646n);assert.equal(test.calls.begin,1);assert.equal(test.calls.link,2);assert.deepEqual(vm.stats(),stats);}vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0x2c,1,0xff,0xe2],mode),{vm}=test;vm.map(data,4096n,7);vm.write(data,[0xdb,0xf1,0x14,0,0x9b,0xcc]);vm.register(2,data);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const before=seed(),top=(before.sw>>11)&7;before.cw=0x37e;before.regs[top]=samples[17];before.regs[(top+1)&7]=one;fp(test,before);vm.ip=base;vm.flags=0x602n;vm.register(0,0n);test.watch.onLink=()=>equal(fp(test),before);assert.equal((await vm.run(3n)).reason,Exit.Budget);test.watch.onLink=null;const expected={...before,regs:[...before.regs],sw:(before.sw&~0x200)|0x8081,ip:data,opcode:0x3f1};equal(fp(test),expected);assert.equal(vm.flags,0x607n,'unmasked invalid comparison preserves incoming resident CF/PF/ZF');assert.equal((await vm.run(1n)).reason,Exit.Budget);assert.equal(vm.register(0),0n);assert.equal(vm.flags,0x657n);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(vm.ip,data+4n);equal(fp(test),expected);vm.dispose();
}
for(const mode of [32,64])for(const [op,nearest] of [[0xf8,false],[0xf5,true],[0xfd,null]]){
 const code=[0xd9,op],test=program([0x04,1,...code],mode),{vm}=test,cases=nearest===null?scalePairs:remainderPairs;
 for(const cw of newControls)for(let n=0;n<cases.length;n+=3){const [a,b]=cases[n],top=n&7,before=seed();before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11)|0x200;before.regs[top]=a;before.regs[(top+1)&7]=b;fp(test,before);vm.ip=base;vm.register(0,127n);vm.flags=0x602n;const out=nearest===null?finish(before.sw,cw,scale(a,b,cw)):remainderResult(before.sw,cw,a,b,nearest),expected=changed(before,code,out);expected.ip=base+2n;if(out.commit)put(expected,0,out.value);assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expected,`instruction/${mode}/${op}/${cw}/${n}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f80);}vm.dispose();
}
for(const mode of [32,64]){
 const code=[0xd9,0xf4],test=program([0x04,1,...code],mode),{vm}=test;
 for(const cw of newControls)for(let n=0;n<samples.length;n++){const value=samples[n],before=seed(),top=n&7;before.cw=cw;before.sw=(before.sw&~0x3820)|(top<<11)|0x200;before.tags|=3<<(((top+7)&7)*2);before.regs[top]=value;fp(test,before);vm.ip=base;vm.register(0,127n);vm.flags=0x602n;const out=finish(before.sw,cw,extract(value,cw)),expected=changed(before,code,out);expected.ip=base+2n;if(out.commit){put(expected,0,out.value);expected.sw=(expected.sw&~0x3800)|(((top+7)&7)<<11);put(expected,0,out.pushed);}assert.equal((await vm.run(2n)).reason,Exit.Budget);equal(fp(test),expected,`extract/${mode}/${cw}/${n}`);assert.equal(vm.flags,0xe92n);assert.equal(vm.mxcsr,0x1f80);}vm.dispose();
}
for(const mode of [32,64])for(const op of [0xf8,0xf5,0xfd,0xf4]){
 const code=[0xd9,op],test=program(code,mode),{vm}=test;
 for(let top=0;top<8;top++)for(const cw of [0x37f,0x37e])for(const pattern of [0,1,2,3]){const before=seed(),next=(top+7)&7;before.cw=cw;before.sw=(before.sw&~0x3800)|(top<<11)|0x200;before.regs[top]=one;before.regs[(top+1)&7]=samples[5];if(op===0xf4){before.tags|=3<<(next*2);if(pattern&2)before.tags&=~(3<<(next*2));}else if(pattern&2)before.tags|=3<<(((top+1)&7)*2);if(pattern&1)before.tags|=3<<(top*2);fp(test,before);vm.ip=base;const overflow=op===0xf4&&!!(pattern&2),empty=!!(pattern&1)||op!==0xf4&&!!(pattern&2),fault=overflow||empty;let out;if(fault)out={sw:stackFault(before,overflow&&!empty)};else out=op===0xf4?finish(before.sw,cw,extract(one,cw)):op===0xfd?finish(before.sw,cw,scale(one,samples[5],cw)):remainderResult(before.sw,cw,one,samples[5],op===0xf5);const expected=changed(before,code,out);if(op===0xf8||op===0xf5)expected.sw&=~0x400;if(!fault||cw&1){put(expected,0,fault?indefinite:out.value);if(op===0xf4){expected.sw=(expected.sw&~0x3800)|(next<<11);put(expected,0,fault?indefinite:out.pushed);}}assert.equal((await vm.run(1n)).reason,Exit.Budget);equal(fp(test),expected,`stacks/${mode}/${op}/${top}/${cw}/${pattern}`);}vm.dispose();
}
for(const mode of [32,64])for(const op of [0xf8,0xf5]){
 const test=program([0xd9,op,0xdf,0xe0,0xf6,0xc4,4,0x75,0xf7],mode),{vm}=test,before=seed(),top=(before.sw>>11)&7,a=(0x4400n<<64n)|0xfedcba9876543210n,b=(0x3fffn<<64n)|0xb123456789abcdefn;before.regs[top]=a;before.regs[(top+1)&7]=b;fp(test,before);const expected={...before,regs:[...before.regs],ip:base,opcode:0x100|op};let rounds=0,value=a;do{const out=remainderResult(expected.sw,before.cw,value,b,op===0xf5);expected.sw=out.sw;value=out.value;put(expected,0,value);rounds++;assert.ok(rounds<100);}while(expected.sw&0x400);
 let result;for(let slice=0;slice<1000;slice++){result=await vm.run([1n,7n,17n][slice%3]);if(result.reason!==Exit.Budget)break;}assert.equal(result.reason,Exit.Unsupported);equal(fp(test),expected);assert.equal(vm.work,BigInt(rounds*4));assert.equal(vm.retired,vm.work);assert.equal(vm.register(0)&0xffffn,BigInt(expected.sw));vm.dispose();
}
for(const mode of [32,64])for(const op of [0xf8,0xf5]){
 const test=program([0xd9,op,0xff,0xe2],mode),{vm}=test;vm.map(data,4096n,7);vm.write(data,[0xdf,0xe0,0xf6,0xc4,4,0x74,2,0xff,0xe6,0xcc]);vm.register(2,data);vm.register(6,base);assert.equal((await vm.run(100n)).reason,Exit.Unsupported);const stats=vm.stats(),before=seed(),top=(before.sw>>11)&7,a=(0x4800n<<64n)|0xfedcba9876543210n,b=(0x3fffn<<64n)|0xb123456789abcdefn;before.regs[top]=a;before.regs[(top+1)&7]=b;fp(test,before);vm.ip=base;test.calls.begin=0;test.calls.link=0;test.watch.onLink=()=>equal(fp(test),before);const expected={...before,regs:[...before.regs],ip:base,opcode:0x100|op};let value=a,rounds=0;do{const out=remainderResult(expected.sw,before.cw,value,b,op===0xf5);expected.sw=out.sw;value=out.value;put(expected,0,value);rounds++;assert.ok(rounds<100);}while(expected.sw&0x400);const startWork=vm.work;assert.equal((await vm.run(1000n)).reason,Exit.Unsupported);equal(fp(test),expected);assert.equal(vm.work-startWork,BigInt(rounds*6-1));assert.equal(test.calls.begin,1);assert.equal(test.calls.link,2);assert.deepEqual(vm.stats(),stats);vm.dispose();
}
for(const mode of [32,64]){
 const test=program([0xd9,0xf4,0xd9,0xfd,0xdd,0xd9],mode),{vm}=test;
 for(const cw of [0x7f,0x27f,0x37f,0xf7f])for(const value of samples.filter(v=>!read(v).unsupported&&!read(v).nan)){const before=seed(),top=(before.sw>>11)&7;before.cw=cw;before.regs[top]=value;before.tags|=3<<(((top+7)&7)*2);fp(test,before);vm.ip=base;const first=extract(value,cw),second=scale(first.pushed,first.value,cw),expected={...before,regs:[...before.regs],sw:finish(finish(before.sw,cw,first).sw,cw,second).sw&~0x200,ip:base+4n,opcode:0x5d9};put(expected,0,second.value);put(expected,7,second.value);expected.tags|=3<<(((top+7)&7)*2);assert.equal((await vm.run(3n)).reason,Exit.Budget);equal(fp(test),expected,'extract/scale round trip retains physical stack ownership');}vm.dispose();
}
for(const [op,clear,a,b] of [[0xf8,1,samples[14],one],[0xf5,2,1n,one],[0xfd,8,samples[11],samples[5]],[0xfd,16,samples[12],integer80(-1)],[0xf4,4,0n,0n],[0xf4,2,1n,0n]]){
 const test=program([0xd9,op,0x04,1,0x9b]),{vm}=test,before=seed(),top=(before.sw>>11)&7;before.cw=0x37f&~clear;before.regs[top]=a;before.regs[(top+1)&7]=b;if(op===0xf4)before.tags|=3<<(((top+7)&7)*2);fp(test,before);vm.register(0,127n);assert.equal((await vm.run(2n)).reason,Exit.Budget);const out=op===0xf4?finish(before.sw,before.cw,extract(a,before.cw)):op===0xfd?finish(before.sw,before.cw,scale(a,b,before.cw)):remainderResult(before.sw,before.cw,a,b,op===0xf5),expected=changed(before,[0xd9,op],out);if(out.commit){put(expected,0,out.value);if(op===0xf4){expected.sw=(expected.sw&~0x3800)|(((top+7)&7)<<11);put(expected,0,out.pushed);}}equal(fp(test),expected);assert.equal(vm.register(0),128n);assert.equal((await vm.run(1n)).reason,Exit.Exception);assert.equal(vm.ip,base+4n);assert.equal(vm.work,2n);equal(fp(test),expected);vm.dispose();
}
for(const op of [0xf8,0xf5,0xfd,0xf4]){
 const test=program([0xd9,op]),{vm}=test,before=seed();before.cw=0x37e;before.sw|=1;fp(test,before);vm.flags=0xed7n;assert.equal((await vm.run(1n)).reason,Exit.Exception);equal(fp(test),before);assert.equal(vm.work,0n);assert.equal(vm.flags,0xed7n);assert.equal(test.calls.numeric,0);vm.dispose();
}
// The import vector now spans two LEB bytes when both numerical families occur.
const all=[];
for(const op of [0x58,0x5c,0x59,0x5e,0x5d,0x5f,0x51])for(const prefix of [0xf3,0xf2])all.push(prefix,0x0f,op,0xc1);
for(const prefix of [0xf3,0xf2])all.push(prefix,0x0f,0xc2,0xc1,0);
for(const op of [0x2e,0x2f])for(const prefix of [[],[0x66]])all.push(...prefix,0x0f,op,0xc1);
for(const op of [0x5a,0x2a,0x2d,0x2c])for(const prefix of [0xf3,0xf2])all.push(prefix,0x0f,op,0xc1);
for(const code of [[0x66,0x0f,0x5b,0xc1],[0xf2,0x0f,0xe6,0xc1],[0xf3,0x0f,0x5b,0xc1],[0x66,0x0f,0xe6,0xc1],[0x0f,0x5b,0xc1],[0xf3,0x0f,0xe6,0xc1]])all.push(...code);
for(const op of [0x7c,0x7d,0xd0])for(const prefix of [0xf2,0x66])all.push(prefix,0x0f,op,0xc1);
for(const op of [8,9,0x40,0x41])all.push(0x66,0x0f,0x3a,op,0xc1,0xff);
for(const code of [[0xd9,0x03],[0xdd,0x03],[0xdf,0x03],[0xd9,0x13],[0xdd,0x13],[0xdf,0x13],[0xdb,0x13],[0xdf,0x3b],[0xdf,0x0b],[0xdb,0x0b],[0xdd,0x0b]])all.push(...code);
for(let n=0;n<7;n++)all.push(0xd9,0xe8+n);
for(const op of [0xc1,0xc9,0xe1,0xf1])all.push(0xd8,op);
for(const op of [0xd8,0xdc,0xde,0xda])for(const index of [0,1,4,5,6,7,2])all.push(op,(index<<3)|3);
all.push(0xd9,0xfa,0xdd,0xe1,0xd8,0xd1,0xdb,0xf1,0xdb,0xe9,0xd9,0xfc,0xd9,0xf8,0xd9,0xf5,0xd9,0xfd,0xd9,0xf4,0xdf,0x23,0xdf,0x33);
for(const op of [0xf0,0xf1,0xf2,0xf3,0xf9,0xfb,0xfe,0xff])all.push(0xd9,op);
all.push(0x48,0xf7,0xf1,0x0f,0xf7,0xc1,0x48,0x0f,0xae,0x03,0xf2,0x0f,0x38,0xf1,0xc1,0x0f,0x31);
const combined=program(all);assert.equal(core.wire_compile(combined.handle),0);const combinedTicket=core.wire_ticket(combined.handle),bytes=new Uint8Array(core.memory.buffer,core.wire_module(combined.handle,combinedTicket),core.wire_module_len(combined.handle,combinedTicket)).slice(),module=new WebAssembly.Module(bytes),imports=WebAssembly.Module.imports(module);assert.ok(imports.length>127);assert.equal(imports.filter(i=>i.name.startsWith('wire_ext_')).length,70);assert.ok(imports.filter(i=>i.kind==='function').length>127,'chunk export requires a multi-byte function index');assert.equal(imports.filter(i=>i.module==='platform').length,1);assert.equal(imports.filter(i=>i.name.startsWith('wire_crc')).length,1);assert.equal(imports.filter(i=>i.name.startsWith('wire_f32_')||i.name.startsWith('wire_f64_')||i.name.startsWith('wire_i64_')||i.name.startsWith('wire_i32x_')).length,44);combined.vm.dispose();

console.log(`${checks} x87 checks passed: independent FP80 arithmetic/rounding, FLAGS comparisons, conditional moves, raw classification, deferred exceptions, precise faults, resident state and source revocation`);
