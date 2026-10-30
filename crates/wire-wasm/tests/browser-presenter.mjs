import assert from 'node:assert/strict';
import {BrowserPresenter} from '../web/presenter.mjs';
let checks=0;const equal=(a,b)=>{assert.deepEqual(a,b);checks++;};
class Element {
 constructor(document,tag){this.ownerDocument=document;this.tagName=tag;this.children=[];this.listeners=new Map();this.style={};this.hidden=false;this.width=300;this.height=150;this.rect={left:100,top:200,width:8,height:6};}
 appendChild(node){node.parent=this;this.children.push(node);return node;}
 remove(){if(this.parent)this.parent.children.splice(this.parent.children.indexOf(this),1);this.removed=true;}
 setAttribute(){}
 addEventListener(type,callback){if(!this.listeners.has(type))this.listeners.set(type,[]);this.listeners.get(type).push(callback);}
 emit(type,values={}){const event={clientX:102,clientY:203,pointerId:1,button:0,deltaMode:0,deltaX:0,deltaY:0,preventDefault(){this.prevented=true;},...values};for(const callback of this.listeners.get(type)||[])callback(event);return event;}
 focus(){if(this.ownerDocument.activeElement===this)return;this.ownerDocument.activeElement?.emit('blur');this.ownerDocument.activeElement=this;this.emit('focus');}
 setPointerCapture(id){this.captured=id;}
 getBoundingClientRect(){return this.rect;}
 getContext(kind){equal(kind,'2d');return{createImageData:(w,h)=>({data:new Uint8ClampedArray(w*h*4),width:w,height:h}),putImageData:image=>{this.image=image;}};}
}
const document={createElement(tag){return new Element(this,tag);}},container=()=>document.createElement('main');
const window=(handle=1n,options={})=>({handle,title:'Display',width:16,height:12,visible:true,topmost:false,...options});
const pixel=[64,128,191,255],frame={width:16,height:12,stride:64,rgba:Uint8Array.from({length:16*12*4},(_,n)=>pixel[n%4])};
function setup(){
 const state={processes:[{id:1},{id:2}],windows:new Map([[1,[window()]],[2,[window()]]]),events:[],queries:0,reject:false,executing:false};
 const check=()=>{assert.ok(!state.executing,'presentation callback queried the core');state.queries++;};
 const vm={processes(){check();return state.processes;},windows(pid){check();return state.windows.get(pid)||[];},windowOrigin(){check();return[-10,20];},windowPixels(){check();return frame;},queueWindowEvents(events,pid){check();if(state.reject){state.reject=false;throw Error('input unavailable');}state.events.push([pid,events.map(({batch,...event})=>event)]);},async run(){check();state.executing=true;await Promise.resolve();state.executing=false;return {reason:1};}};
 const root=container(),presenter=new BrowserPresenter(vm,root);presenter.sync();return{state,vm,root,presenter};
}
const canvas=t=>t.root.children[0].children[1].children[0];
{
 const t=setup(),{presenter,root,state}=t;equal(root.children.length,2);equal(canvas(t).image.data.slice(0,4),new Uint8ClampedArray(pixel));equal([canvas(t).width,canvas(t).height],[16,12]);
 state.windows.get(2)[0]=window(1n,{title:'Other',visible:false,topmost:true});presenter.sync();equal(root.children[1].hidden,true);equal(root.children[1].children[0].children[0].textContent,'Other');equal(root.children[1].style.zIndex,'1');
 const before=state.queries;state.executing=true;const gpu=presenter.getCanvas(1n,1);equal(presenter.getCanvas(1n,1),gpu);equal(state.queries,before);state.executing=false;equal(canvas(t).hidden,true);equal(gpu.hidden,false);
 presenter.sync();equal(gpu.image,undefined);equal(root.children[0].children[1].children.length,2);
 state.windows.set(1,[]);presenter.sync();equal(root.children.length,1);presenter.close();equal(root.children.length,0);presenter.close();assert.throws(()=>presenter.sync(),/closed/);assert.throws(()=>presenter.getCanvas(1n,1),/closed/);
}
{
 const t=setup(),{presenter,state,root}=t,c=canvas(t);
 c.emit('keydown',{code:'KeyA',key:'A',shiftKey:true});c.emit('keydown',{code:'KeyA',key:'A',repeat:true});c.emit('keyup',{code:'KeyA',key:'A'});
 c.emit('keydown',{code:'ArrowLeft',key:'ArrowLeft'});c.emit('keyup',{code:'ArrowLeft',key:'ArrowLeft'});
 c.emit('keydown',{code:'KeyB',key:'𝄞'});c.emit('keyup',{code:'KeyB',key:'𝄞'});
 c.emit('keydown',{code:'KeyC',key:'c',ctrlKey:true});c.emit('keyup',{code:'KeyC',key:'c',ctrlKey:true});
 c.emit('keydown',{code:'KeyD',key:'d',isComposing:true});
 presenter.flushInput();const events=state.events[0][1];equal(events.map(e=>e.message),[0x100,0x102,0x100,0x102,0x101,0x100,0x101,0x100,0x102,0x102,0x101,0x100,0x101]);
 equal(events[0].lparam,0x1e0001n);equal(events[2].lparam,0x401e0001n);equal(events[4].lparam,0xc01e0001n);equal(events[5].lparam,0x14b0001n);equal(events.slice(8,10).map(e=>e.wparam),[0xd834n,0xdd1en]);
 state.events=[];const other=root.children[1].children[1].children[0];other.emit('pointerdown',{button:2,shiftKey:true});other.emit('pointermove',{clientX:103});other.emit('pointermove',{clientX:104});other.emit('wheel',{deltaY:0.4});other.emit('wheel',{deltaY:0.7});other.emit('pointerup',{button:2});
 presenter.flushInput();equal(state.events[0][0],2);const mouse=state.events[0][1];equal(mouse.map(e=>e.message),[0x1c,6,7,0x204,0x200,0x20a,0x205]);equal(mouse[3].wparam,6n);equal(mouse[3].lparam,0x60004n);equal(mouse[3].point,[-6,26]);equal(mouse[4].lparam,0x60008n);equal(mouse[5].wparam,0xffff0002n);equal(mouse[5].lparam,0x1afffan);equal(other.captured,1);
 presenter.close();
}
{
 const t=setup(),{presenter,state}=t,c=canvas(t);c.focus();c.emit('keydown',{code:'AltLeft',key:'Alt',altKey:true});c.emit('keydown',{code:'KeyE',key:'é',altKey:true});c.emit('pointerdown');c.emit('blur');presenter.flushInput();const events=state.events[0][1];equal(events.map(e=>e.message),[0x1c,6,7,0x104,0x104,0x106,0x201,0x105,0x105,0x202,8,6,0x1c]);equal(events[5].wparam,0xe9n);equal(events[9].lparam,events[6].lparam);
 state.events=[];t.root.children[0].children[0].children[1].emit('click');state.reject=true;assert.throws(()=>presenter.flushInput(),/input unavailable/);presenter.flushInput();equal(state.events[0][1],[{window:1n,message:0x10}]);
 presenter.close();
}
// A batch boundary cannot separate a key-down from its UTF-16 text, and failed flushes retain input.
{
 const t=setup(),{presenter,state}=t,c=canvas(t),close=t.root.children[0].children[0].children[1];for(let n=0;n<255;n++)close.emit('click');c.emit('keydown',{code:'KeyA',key:'𝄞'});presenter.flushInput();equal(state.events.map(([,events])=>events.length),[255,3]);equal(state.events[1][1].map(e=>e.message),[0x100,0x102,0x102]);
 const pending=presenter.run(1n);assert.throws(()=>presenter.flushInput(),/already running/);assert.throws(()=>presenter.sync(),/already running/);equal((await pending).reason,1);presenter.close();
}
console.log(`${checks} browser presentation/PID/canvas/input/coordinates/lifetime checks passed`);
