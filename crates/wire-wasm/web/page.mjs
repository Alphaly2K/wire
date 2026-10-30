import {WasmApplication,Exit,validateInstallation} from './application.mjs';
import {readCPUTimeSample,readApplicationBytes as fetchBytes} from './platform.mjs';

const start=document.getElementById('start'),stop=document.getElementById('stop'),status=document.getElementById('status'),output=document.getElementById('output'),windows=document.getElementById('windows');
let manifest,core,application,compiler,context,stopping=false,active=false,reported=false,inputEnded=false,inputQueue=[],queuedBytes=0;
const decoders=new Map();
const script=url=>new Promise((resolve,reject)=>{const element=document.createElement('script');element.src=url;element.onload=resolve;element.onerror=()=>reject(Error('无法载入图形编译器'));document.head.appendChild(element);});
async function shaderCompiler() {
  let pending=null,closed=false;
  const get=()=>{
    if(closed)throw Error('shader compiler has been closed');
    if(!pending)pending=(async()=>{await script('/shader/dxbc.js');await script('/shader/compiler.js');const [dxbcWasm,nagaWasm]=await Promise.all([fetchBytes('/shader/dxbc.wasm'),fetchBytes('/shader/naga.wasm')]);const value=await WireShaderCompiler.create({dxbcFactory:WireDXBCModule,dxbcWasm,nagaWasm});if(closed){value.close();throw Error('shader compiler has been closed');}return value;})();
    return pending;
  };
  return {async compileDXBC(...args){return (await get()).compileDXBC(...args);},async linkVertex(...args){return (await get()).linkVertex(...args);},async close(){closed=true;if(pending)try{(await pending).close();}catch{}}};
}
async function post(path,bytes){const response=await fetch(path,{method:'POST',body:bytes});if(!response.ok)throw Error('宿主输出失败');}
async function report(path,bytes){reported=true;await post(path,bytes);}
function display(text){output.textContent=(output.textContent+text).slice(-1024*1024);}
async function session() {
  const response=await fetch('/session');if(!response.ok)throw Error('无法建立浏览器运行连接');
  const reader=response.body.getReader();
  const monitor=async()=>{for(;;)if((await reader.read()).done){if(!reported)throw Error('浏览器运行连接已断开');return;}};
  monitor().catch(error=>{if(!reported){reported=true;stopping=true;start.disabled=true;status.textContent=String(error.message || error);}});
}
async function drain() {
  for(const process of application.machine.processes())for(const endpoint of[1,2]){
    const key=process.id+':'+endpoint;let decoder=decoders.get(key);if(!decoder)decoders.set(key,decoder=new TextDecoder());
    for(;;){const bytes=application.readOutput(endpoint,65536,process.id);if(!bytes.length)break;display(decoder.decode(bytes,{stream:true}));await post('/output/'+endpoint,bytes);}
    if(process.exitCode!==null){display(decoder.decode());decoders.delete(key);}
  }
}
async function run() {
  if(active || reported)return;active=true;stopping=false;inputEnded=!!manifest.input;start.disabled=true;stop.disabled=false;
  let terminal='/cancel',message='';
  try {
    if(manifest.audio){context=new AudioContext({sampleRate:44100,latencyHint:'interactive'});await context.resume();if(context.state!=='running')throw Error('音频输出未能启动');}
    compiler=manifest.graphics?await shaderCompiler():null;
    application=await WasmApplication.load({core,manifest,read:fetchBytes,container:windows,runtime:{cpuTimes:readCPUTimeSample},graphics:manifest.graphics?{shaderCompiler:compiler}:null,audio:context?{context}:null});
    status.textContent='运行中';
    while(!stopping){
      const began=performance.now();let result;
      for(const input of inputQueue)application.writeInput(input.bytes,{end:input.end});inputQueue=[];queuedBytes=0;
      do{result=await application.step(100000n);if(![Exit.Budget,Exit.Dispatch].includes(result.reason))break;}while(performance.now()-began<8 && !stopping);
      application.present();await drain();
      if(result.reason===Exit.ProcessExited){const code=result.exitCode ?? 0;status.textContent='程序已退出：'+code;terminal='/exit';message=String(code);return;}
      if(![Exit.Budget,Exit.Dispatch,Exit.Waiting].includes(result.reason))throw Error('程序停止，退出原因 '+result.reason+'，位置 0x'+result.ip.toString(16));
      await new Promise(resolve=>setTimeout(resolve,result.reason===Exit.Waiting?16:0));
    }
    status.textContent='已停止';
  }catch(error){message=String(error.message || error);status.textContent=message;terminal='/error';}
  finally{
    try{application?.close();application=null;await compiler?.close();compiler=null;if(context){if(context.state!=='closed')await context.close();context=null;}}
    catch(error){terminal='/error';message=String(error.message || error);status.textContent=message;}
    inputQueue=[];queuedBytes=0;decoders.clear();active=false;stop.disabled=true;
    try{await report(terminal,message);}catch(error){status.textContent=String(error.message || error);}
  }
}
start.addEventListener('click',run);stop.addEventListener('click',()=>{stopping=true;});
document.getElementById('console').addEventListener('submit',event=>{event.preventDefault();if(!active || inputEnded)return;const input=document.getElementById('input'),bytes=new TextEncoder().encode(input.value+'\n');if(queuedBytes+bytes.length>8*1024*1024){status.textContent='待发送输入已满';return;}inputQueue.push({bytes,end:false});queuedBytes+=bytes.length;input.value='';});
document.getElementById('eof').addEventListener('click',()=>{if(active && !inputEnded){inputQueue.push({bytes:new Uint8Array(),end:true});inputEnded=true;}});
try {
  const response=await fetch('/manifest.json');if(!response.ok)throw Error('无法读取安装清单');manifest=await response.json();validateInstallation(manifest);
  await session();
  core=await WebAssembly.compile(await fetchBytes('/core.wasm'));status.textContent='就绪';start.disabled=false;
  if(manifest.autostart && !manifest.audio)await run();
}catch(error){status.textContent=String(error.message || error);try{await report('/error',String(error.message || error));}catch{}}
addEventListener('pagehide',()=>{stopping=true;if(!reported)navigator.sendBeacon('/cancel','');});
