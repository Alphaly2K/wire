import {physicalKey,keyParameter,keyText,mouseFlags,packedPoint} from './input.mjs';

/** Browser windows and input for a borrowed WasmMachine process family. */
export class BrowserPresenter {
  #machine;
  #container;
  #windows = new Map();
  #pending = new Map();
  #running = false;
  #closed = false;
  #failure = null;

  constructor(machine, container) {
    if (!container?.ownerDocument || typeof container.appendChild !== 'function') throw new TypeError('expected a DOM window container');
    this.#machine=machine;this.#container=container;
  }

  #assertOpen() { if(this.#closed) throw Error('presenter has been closed'); }
  #identity(window, process) {
    if(typeof window!=='bigint' || window<0n || window>0xffffffffffffffffn || !Number.isInteger(process) || process<1 || process>0xffffffff) throw new TypeError('invalid presentation window identity');
    return process+':'+window;
  }
  #window(window, process) {
    const key=this.#identity(window,process);
    let record=this.#windows.get(key);
    if(record)return record;
    const document=this.#container.ownerDocument,element=document.createElement('section'),header=document.createElement('header'),title=document.createElement('span'),close=document.createElement('button'),surface=document.createElement('div');
    element.className='wire-window';surface.className='wire-surface';close.type='button';close.textContent='×';close.setAttribute('aria-label','Close window');
    header.appendChild(title);header.appendChild(close);element.appendChild(header);element.appendChild(surface);this.#container.appendChild(element);
    record={key,window,process,element,title,surface,width:0,height:0,origin:[0,0],point:null,keys:new Map(),buttons:0,wheel:[0,0],software:null,gpu:null,active:false};
    close.addEventListener('click',()=>this.#queue(record,[{message:0x10}]));
    this.#windows.set(key,record);return record;
  }

  #queue(record, events) {
    if(this.#closed || this.#failure)return;
    let pending=this.#pending.get(record.process);
    if(!pending)this.#pending.set(record.process,pending=[]);
    const batch={};
    for(const event of events) {
      const previous=pending.at(-1);
      if(event.message===0x200 && previous?.message===0x200 && previous.window===record.window) pending.pop();
      if(pending.length>=4096){this.#failure=Error('browser input queue capacity exceeded');return;}
      pending.push({window:record.window,...event,batch});
    }
  }

  #canvas(record, gpu) {
    const name=gpu?'gpu':'software';if(record[name])return record[name];
    const canvas=this.#container.ownerDocument.createElement('canvas');canvas.tabIndex=0;canvas.setAttribute('aria-label','Application display');record.surface.appendChild(canvas);record[name]=canvas;
    canvas.addEventListener('contextmenu',event=>event.preventDefault());
    canvas.addEventListener('focus',()=>{
      if(record.active)return;record.active=true;
      this.#queue(record,[{message:0x1c,wparam:1n},{message:6,wparam:1n},{message:7}]);
    });
    canvas.addEventListener('blur',()=>this.#blur(record));
    for(const down of[true,false])canvas.addEventListener(down?'keydown':'keyup',event=>{
      const key=physicalKey(event.code);if(!key || event.isComposing)return;
      const old=record.keys.get(event.code),system=old && !down?old.system:(event.altKey || key.vk===18 || key.vk===121);
      const lparam=keyParameter(key,{down,previous:!!old || event.repeat,alt:event.altKey});
      if(down)record.keys.set(event.code,{key,system,alt:event.altKey});else record.keys.delete(event.code);
      const events=[{message:(system?0x104:0x100)+(down?0:1),wparam:BigInt(key.vk),lparam}];
      if(down){const text=keyText(event);for(let n=0;n<text.length;n++)events.push({message:system?0x106:0x102,wparam:BigInt(text.charCodeAt(n)),lparam});}
      this.#queue(record,events);event.preventDefault();
    });
    const point=event=>{
      const rect=canvas.getBoundingClientRect();
      if(!rect.width || !rect.height)return null;
      const x=Math.floor((event.clientX-rect.left)*record.width/rect.width),y=Math.floor((event.clientY-rect.top)*record.height/rect.height);
      const screen=[x+record.origin[0],y+record.origin[1]].map(n=>Math.max(-2147483648,Math.min(2147483647,n)));
      return record.point={x,y,screen};
    };
    canvas.addEventListener('pointermove',event=>{
      const p=point(event);if(!p)return;this.#queue(record,[{message:0x200,wparam:BigInt(mouseFlags(record.buttons,event)),lparam:packedPoint(p.x,p.y),point:p.screen}]);event.preventDefault();
    });
    for(const down of[true,false])canvas.addEventListener(down?'pointerdown':'pointerup',event=>{
      const masks=[1,16,2,32,64],messages=[0x201,0x207,0x204,0x20b,0x20b],mask=masks[event.button],p=point(event);if(!mask || !p)return;
      if(down){canvas.focus();canvas.setPointerCapture(event.pointerId);record.buttons|=mask;}else record.buttons&=~mask;
      this.#queue(record,[{message:messages[event.button]+(down?0:1),wparam:BigInt(mouseFlags(record.buttons,event)|(event.button>=3?(event.button-2)<<16:0)),lparam:packedPoint(p.x,p.y),point:p.screen}]);event.preventDefault();
    });
    canvas.addEventListener('lostpointercapture',()=>this.#releaseButtons(record));
    canvas.addEventListener('wheel',event=>{
      const p=point(event);if(!p)return;
      const scale=event.deltaMode===1?120:event.deltaMode===2?Math.max(1,record.height):1,values=[-event.deltaY,event.deltaX];
      const events=[];
      for(let axis=0;axis<2;axis++){
        const value=record.wheel[axis]+values[axis]*scale;record.wheel[axis]=Number.isFinite(value)?Math.max(-262144,Math.min(262136,value)):0;
        while(Math.abs(record.wheel[axis])>=1){const delta=Math.max(-32768,Math.min(32767,Math.trunc(record.wheel[axis])));record.wheel[axis]-=delta;events.push({message:axis?0x20e:0x20a,wparam:BigInt((mouseFlags(record.buttons,event)|(delta&65535)<<16)>>>0),lparam:packedPoint(...p.screen),point:p.screen});}
      }
      this.#queue(record,events);event.preventDefault();
    },{passive:false});
    return canvas;
  }

  #releaseButtons(record) {
    const masks=[1,16,2,32,64],messages=[0x202,0x208,0x205,0x20c,0x20c],events=[];
    const p=record.point;
    for(let n=0;n<masks.length;n++)if(record.buttons&masks[n]){record.buttons&=~masks[n];events.push({message:messages[n],wparam:BigInt(record.buttons|(n>=3?(n-2)<<16:0)),...(p?{lparam:packedPoint(p.x,p.y),point:p.screen}:{})});}
    this.#queue(record,events);
  }
  #blur(record) {
    if(!record.active)return;
    const events=[];
    for(const {key,system,alt}of record.keys.values())events.push({message:system?0x105:0x101,wparam:BigInt(key.vk),lparam:keyParameter(key,{down:false,alt})});
    record.keys.clear();this.#queue(record,events);this.#releaseButtons(record);record.active=false;
    this.#queue(record,[{message:8},{message:6},{message:0x1c}]);
  }

  // Present invokes this while the core owns a service; all work here is host-local.
  getCanvas = (window, process) => {
    this.#assertOpen();const record=this.#window(window,process),canvas=this.#canvas(record,true);
    if(record.software)record.software.hidden=true;canvas.hidden=false;return canvas;
  };

  sync() {
    this.#assertOpen();if(this.#running)throw Error('presenter is already running');
    const live=new Set();
    for(const process of this.#machine.processes())for(const window of this.#machine.windows(process.id)){
      const record=this.#window(window.handle,process.id);live.add(record.key);record.width=window.width;record.height=window.height;record.origin=this.#machine.windowOrigin(window.handle,process.id) ?? [0,0];
      record.title.textContent=window.title;record.element.hidden=!window.visible;record.element.style.zIndex=window.topmost?'1':'0';
      if(!window.visible || !window.width || !window.height || record.gpu)continue;
      const frame=this.#machine.windowPixels(window.handle,process.id);if(!frame)continue;
      const canvas=this.#canvas(record,false);if(canvas.width!==frame.width)canvas.width=frame.width;if(canvas.height!==frame.height)canvas.height=frame.height;
      const context=canvas.getContext('2d');if(!context)throw Error('browser 2D canvas is unavailable');
      const image=context.createImageData(frame.width,frame.height);image.data.set(frame.rgba);context.putImageData(image,0,0);
    }
    for(const [key,record]of this.#windows)if(!live.has(key)){record.element.remove();this.#windows.delete(key);}
    for(const [process,events]of this.#pending){const retained=events.filter(event=>live.has(this.#identity(event.window,process)));if(retained.length)this.#pending.set(process,retained);else this.#pending.delete(process);}
  }

  flushInput() {
    this.#assertOpen();if(this.#running)throw Error('presenter is already running');if(this.#failure)throw this.#failure;
    for(const [process,events]of this.#pending){
      while(events.length){let count=Math.min(256,events.length);while(count<events.length && events[count-1].batch===events[count].batch)count--;this.#machine.queueWindowEvents(events.slice(0,count).map(({batch,...event})=>event),process);events.splice(0,count);}
      this.#pending.delete(process);
    }
  }
  async run(budget) {
    this.#assertOpen();if(this.#running)throw Error('presenter is already running');
    budget=BigInt(budget);if(budget<0n || budget>0xffffffffffffffffn)throw new RangeError('invalid work budget');
    this.flushInput();this.#running=true;
    let result;
    try{result=await this.#machine.run(budget);}
    finally{this.#running=false;}
    if(!this.#closed)this.sync();return result;
  }
  close() {
    if(this.#closed)return;this.#closed=true;for(const record of this.#windows.values())record.element.remove();this.#windows.clear();this.#pending.clear();
  }
}
