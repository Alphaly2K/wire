import assert from 'node:assert/strict';
import '../web/gpu.js';
let checks=0;const equal=(a,b)=>{assert.deepEqual(a,b);checks++;};
// The test device validates the same device identity required by a render pass.
function device(){
 const d={features:new Set(),lost:new Promise(()=>{}),addEventListener(){},destroy(){this.destroyed=true;},draws:0,submits:0,
  createShaderModule(){return{};},createRenderPipeline(){return{device:d,getBindGroupLayout(){return{};}};},createBindGroup(){return{};},
  createCommandEncoder(){let pipeline,view;return{beginRenderPass(desc){view=desc.colorAttachments[0].view;return{setPipeline(p){pipeline=p;},setBindGroup(){},draw(){equal(view.device,pipeline.device);d.draws++;},end(){}};},finish(){return{};}};}};
 d.queue={submit(){d.submits++;}};return d;
}
function canvas(){const context={device:null,configured:[],unconfigured:0,configure({device}){this.device=device;this.configured.push(device);},unconfigure(){this.unconfigured++;this.device=null;},getCurrentTexture(){assert.ok(this.device);const device=this.device;return{createView:()=>({device})};}};return{width:0,height:0,context,getContext(kind){equal(kind,'webgpu');return context;}};}
function owner(target){const d=device(),gpu={getPreferredCanvasFormat:()=> 'bgra8unorm'},o=new WireWebGPU(new WebAssembly.Memory({initial:1}),gpu,{info:{}},d,{canvas:target});o.put('texture',{width:8,height:8,format:{depth:false},layers:1,volume:false,texture:{createView(){return{};},destroy(){}}});return o;}
// Independent process owners can alternate on one host canvas and retire in either order.
for(const closeCurrent of[false,true]){
 const c=canvas(),a=owner(c),b=owner(c);a.present(1,1n,0);b.present(1,1n,0);a.present(1,1n,0);equal(c.context.configured,[a.device,b.device,a.device]);equal([c.width,c.height],[8,8]);
 if(closeCurrent){a.close();equal(c.context.unconfigured,1);b.present(1,1n,0);equal(c.context.device,b.device);b.close();equal(c.context.unconfigured,2);}
 else{b.close();equal(c.context.unconfigured,0);equal(c.context.device,a.device);a.present(1,1n,0);equal(a.presented,3);a.close();equal(c.context.unconfigured,1);}
 equal(a.device.destroyed,true);equal(b.device.destroyed,true);a.close();b.close();
}
// Evicting an owner's old surface must keep the current user's canvas configured.
{
 const shared=canvas(),a=owner(shared),b=owner(shared);a.present(1,1n,0);b.present(1,1n,0);
 for(let i=0;i<16;i++){const c=canvas();a.options.canvas=c;a.present(1,BigInt(i+2),0);}
 equal(a.surfaces.size,16);equal(shared.context.unconfigured,0);equal(shared.context.device,b.device);a.close();equal(shared.context.unconfigured,0);b.present(1,1n,0);equal(b.presented,2);b.close();equal(shared.context.unconfigured,1);
}
console.log(`${checks} canvas device identity/alternating owners/retirement/eviction checks passed`);
