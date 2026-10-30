// Consume the shared gpu_draw::Draw wire format, never a guest Windows ABI.
// Shader compilation/linking and WebGPU commands stay in this WebContent VM.
globalThis.WireWebGPUDraw = class WireWebGPUDraw {
    constructor(owner) {
        this.owner=owner;this.closed=false;this.clock=0;this.ownedBytes=0;this.cacheBytes=0;
        this.pipelines=new Map();this.linked=new Map();this.samplers=new Map();this.bounds=new Map();
        this.maps=[this.pipelines,this.linked,this.samplers,this.bounds];
        this.versions=new Map();this.parameters=null;this.parameterData=null;this.batchDraws=0;
        this.draws=0;this.pipelineHits=0;this.pipelineBuilds=0;this.indexReads=0;this.drains=0;
        this.floatBits=new DataView(new ArrayBuffer(4));
        this.vertexFormats=new Map([
            [2,["float32x4",16]],[3,["uint32x4",16]],[4,["sint32x4",16]],
            [6,["float32x3",12]],[7,["uint32x3",12]],[8,["sint32x3",12]],
            [10,["float16x4",8]],[11,["unorm16x4",8]],[12,["uint16x4",8]],[13,["snorm16x4",8]],[14,["sint16x4",8]],
            [16,["float32x2",8]],[17,["uint32x2",8]],[18,["sint32x2",8]],[24,["unorm10-10-10-2",4]],
            [28,["unorm8x4",4]],[30,["uint8x4",4]],[31,["snorm8x4",4]],[32,["sint8x4",4]],
            [34,["float16x2",4]],[35,["unorm16x2",4]],[36,["uint16x2",4]],[37,["snorm16x2",4]],[38,["sint16x2",4]],
            [41,["float32",4]],[42,["uint32",4]],[43,["sint32",4]],
            [49,["unorm8x2",2]],[50,["uint8x2",2]],[51,["snorm8x2",2]],[52,["sint8x2",2]]
        ]);
    }
    float(word) {this.floatBits.setUint32(0,word,true);return this.floatBits.getFloat32(0,true);}
    charge(bytes) {this.owner.reserve(bytes);this.owner.liveBytes+=bytes;this.ownedBytes+=bytes;}
    uncharge(bytes) {if(!this.closed){this.owner.liveBytes-=bytes;this.ownedBytes-=bytes;}}
    get(map,key) {const value=map.get(key);if(value)value.last=++this.clock;return value;}
    remove(map,key) {const value=map.get(key);if(value){map.delete(key);this.cacheBytes-=value.cost;this.uncharge(value.cost);}}
    reclaim(bytes) {
        // Drop reusable cache entries before refusing a new process resource.
        // Fixed parameter storage survives until the enclosing owner closes.
        while(this.owner.liveBytes+this.owner.stagingBytes+(this.owner.pending?.bytes||0)+bytes>this.owner.byteLimit){
            let oldest=null;
            for(const map of this.maps)for(const [key,value] of map)if(!oldest||value.last<oldest.value.last)oldest={map,key,value};
            if(!oldest)return;this.remove(oldest.map,oldest.key);
        }
    }
    space(map,cost,count) {
        if(cost>64*1024*1024)throw Error("WebGPU draw cache entry capacity");
        while(map.size>=count||this.cacheBytes+cost>64*1024*1024) {
            let oldest=null;
            for(const candidate of (map.size>=count?[map]:this.maps))for(const [key,value] of candidate)
                if(!oldest||value.last<oldest.value.last)oldest={map:candidate,key,value};
            if(!oldest)throw Error("WebGPU draw cache capacity");this.remove(oldest.map,oldest.key);
        }
    }
    async cached(map,key,cost,count,create,check) {
        this.space(map,cost,count);this.charge(cost);let retained=false;
        try {
            const value=await create();check();
            const entry={...value,cost,last:++this.clock};map.set(key,entry);this.cacheBytes+=cost;retained=true;return entry;
        } finally {if(!retained)this.uncharge(cost);}
    }
    check(d) {
        if(this.closed||this.owner.closed||this.owner.generation!==d.generation)throw Error("WebGPU draw cancelled");
        for(const [id,value] of d.resources)if(this.owner.objects.get(id)?.value!==value)throw Error("WebGPU draw resource released while pending");
        if(d.index&&(this.versions.get(d.indexID)||0)!==d.indexVersion)throw Error("WebGPU pending index input changed");
    }
    parse(input) {
        if(input.length<122*4||input.length>32*1024||input.length%4)throw Error("WebGPU draw packet layout/capacity");
        const view=new DataView(input.buffer,input.byteOffset,input.length),w=Array.from({length:input.length/4},(_,i)=>view.getUint32(i*4,true));
        const [nv,na,nb]=w.slice(119,122);
        if(nv>32||na>32||nb>316||w.length!==122+nv*4+na*6+nb*17)throw Error("WebGPU draw packet counts");
        const resources=[],object=(id,kind)=>{const value=this.owner.object(id,kind);resources.push([id,value]);return value;};
        const d={generation:this.owner.generation,resources,vsID:w[0],psID:w[1],vs:object(w[0],"shader"),ps:object(w[1],"shader"),target:object(w[2],"view"),depth:w[3]?object(w[3],"view"):null,
            indexID:w[4],index:w[4]?object(w[4],"buffer"):null,topology:w[5],indexFormat:w[6],indexOffset:w[7],indexed:w[8],count:w[9],first:w[10],baseVertex:w[11]|0,instances:w[12],firstInstance:w[13],
            viewport:w.slice(14,20).map(v=>this.float(v)),scissor:w.slice(20,24).map(v=>v|0),blend:w.slice(24,90),depthState:w.slice(90,103),raster:w.slice(103,113),blendFactor:w.slice(113,117).map(v=>this.float(v)),sampleMask:w[117],stencilRef:w[118],vertices:new Map(),attributes:[],descriptors:new Map()};
        if(d.vs.stage!==1||d.ps.stage!==2||d.target.kind!==1||d.target.format.depth||d.target.layers!==1||d.target.mips!==1||d.target.texture.volume||d.indexed>1||!d.count||d.count>16*1024*1024||!d.instances||d.instances>65536||d.first+d.count>0x100000000||d.firstInstance+d.instances>0x100000000)throw Error("WebGPU draw stages/target/count profile");
        d.width=Math.max(1,d.target.texture.width>>d.target.mip);d.height=Math.max(1,d.target.texture.height>>d.target.mip);
        if(d.depth&&(d.depth.kind!==2||!d.depth.format.depth||d.depth.layers!==1||d.depth.mips!==1||d.depth.texture.volume||Math.max(1,d.depth.texture.width>>d.depth.mip)!==d.width||Math.max(1,d.depth.texture.height>>d.depth.mip)!==d.height))throw Error("WebGPU draw depth attachment profile");
        if(d.indexed){
            d.indexBytes=d.indexFormat===57?2:d.indexFormat===42?4:0;
            if(!d.index||!d.indexBytes||d.indexOffset%d.indexBytes||d.indexOffset+(d.first+d.count)*d.indexBytes>d.index.size)throw Error("WebGPU draw index range/format");
            d.indexVersion=this.versions.get(d.indexID)||0;
            d.boundsKey=JSON.stringify([d.indexID,d.indexVersion,d.indexFormat,d.indexOffset,d.first,d.count,d.topology===3||d.topology===5]);
        }
        let at=122;
        for(let i=0;i<nv;i++,at+=4){const [slot,id,stride,offset]=w.slice(at,at+4);if(slot>=32||d.vertices.has(slot))throw Error("WebGPU draw vertex slot");d.vertices.set(slot,{slot,buffer:object(id,"buffer"),stride,offset});}
        const locations=new Set();
        for(let i=0;i<na;i++,at+=6){const [location,format,slot,offset,inputClass,step]=w.slice(at,at+6);if(location>=32||locations.has(location)||inputClass>1||step>1||!this.vertexFormats.has(format)||!d.vertices.has(slot))throw Error("WebGPU draw vertex attribute profile");locations.add(location);d.attributes.push({location,format,slot,offset,inputClass,step});}
        if(d.vs.inputs.some(location=>!locations.has(location)))throw Error("WebGPU input layout does not cover shader locations");
        for(let i=0;i<nb;i++,at+=17){const [set,binding,kind,id]=w.slice(at,at+4),key=set+":"+binding;if(set>1||kind>3||d.descriptors.has(key))throw Error("WebGPU draw descriptor profile/duplicate");const resource=kind===0?object(id,"buffer"):kind===1?object(id,"view"):kind===2?object(id,"bufferView"):w.slice(at+4,at+17);d.descriptors.set(key,{kind,resource});}
        this.describe(d);this.validateBindings(d);return d;
    }
    compare(value) {const names=["never","less","equal","less-equal","greater","not-equal","greater-equal","always"];if(value<1||value>8)throw Error("WebGPU draw comparison");return names[value-1];}
    stencil(value) {const names=["keep","zero","replace","increment-clamp","decrement-clamp","invert","increment-wrap","decrement-wrap"];if(value<1||value>8)throw Error("WebGPU draw stencil operation");return names[value-1];}
    factor(value,alpha) {
        const names={1:"zero",2:"one",3:alpha?"src-alpha":"src",4:alpha?"one-minus-src-alpha":"one-minus-src",5:"src-alpha",6:"one-minus-src-alpha",7:"dst-alpha",8:"one-minus-dst-alpha",9:alpha?"dst-alpha":"dst",10:alpha?"one-minus-dst-alpha":"one-minus-dst",11:alpha?"one":"src-alpha-saturated",14:"constant",15:"one-minus-constant"};
        if(!names[value])throw Error("WebGPU draw blend factor");return names[value];
    }
    blendOperation(value) {const names=["add","subtract","reverse-subtract","min","max"];if(value<1||value>5)throw Error("WebGPU draw blend operation");return names[value-1];}
    describe(d) {
        const r=d.raster,z=d.depthState,b=d.blend,limits=this.owner.device.limits;
        const topology={1:"point-list",2:"line-list",3:"line-strip",4:"triangle-list",5:"triangle-strip"}[d.topology];
        if(!topology||r[0]!==3||r[1]<1||r[1]>3||r.slice(2,3).some(v=>v>1)||r.slice(6).some(v=>v>1)||r[3]||this.float(r[4])!==0||this.float(r[5])!==0||([2,3].includes(d.topology)&&(r[8]||r[9])))throw Error("WebGPU draw topology/raster profile");
        if(!r[6]&&!this.owner.device.features.has("depth-clip-control"))throw Error("WebGPU depth clip control unavailable");
        const [x,y,width,height,minDepth,maxDepth]=d.viewport;
        if(d.viewport.some(v=>!Number.isFinite(v))||x<0||y<0||width<=0||height<=0||x+width>d.width||y+height>d.height||minDepth<0||maxDepth>1||minDepth>maxDepth)throw Error("WebGPU draw viewport profile");
        let [left,top,right,bottom]=r[7]?d.scissor:[0,0,d.width,d.height];left=Math.min(d.width,Math.max(0,left));top=Math.min(d.height,Math.max(0,top));right=Math.max(left,Math.min(d.width,right));bottom=Math.max(top,Math.min(d.height,bottom));d.scissorRect=[left,top,right-left,bottom-top];
        if(z[0]>1||z[1]>1||z[3]>1||b[0]>1||b[1]>1||b[2]>1||b[9]>15||d.blendFactor.some(v=>!Number.isFinite(v)))throw Error("WebGPU draw depth/blend BOOL/mask");
        const faces=[5,9].map(at=>({failOp:this.stencil(z[at]),depthFailOp:this.stencil(z[at+1]),passOp:this.stencil(z[at+2]),compare:this.compare(z[at+3])}));
        const compare=this.compare(z[2]);
        if(d.depth&&z[3]&&!d.depth.format.stencil)throw Error("WebGPU draw stencil aspect absent");
        d.depthDescription=d.depth?{format:d.depth.format.name,depthWriteEnabled:!!(z[0]&&z[1]),depthCompare:z[0]?compare:"always",...(d.depth.format.stencil?{stencilFront:z[3]?faces[0]:{},stencilBack:z[3]?faces[1]:{},stencilReadMask:z[3]?z[4]&255:0,stencilWriteMask:z[3]?(z[4]>>>8)&255:0}:{})}:undefined;
        d.targetDescription={format:d.target.format.name,writeMask:b[9]};
        if(b[2])d.targetDescription.blend={color:{srcFactor:this.factor(b[3],false),dstFactor:this.factor(b[4],false),operation:this.blendOperation(b[5])},alpha:{srcFactor:this.factor(b[6],true),dstFactor:this.factor(b[7],true),operation:this.blendOperation(b[8])}};
        d.primitive={topology,cullMode:["none","front","back"][r[1]-1],frontFace:r[2]?"ccw":"cw",unclippedDepth:!r[6]};
        if(d.indexed&&(d.topology===3||d.topology===5))d.primitive.stripIndexFormat=d.indexBytes===2?"uint16":"uint32";
        d.layouts=[];d.vertexBindings=[];
        for(const [slot,v] of [...d.vertices].sort((a,b)=>a[0]-b[0])) {
            const attributes=d.attributes.filter(a=>a.slot===slot);if(!attributes.length)continue;
            const first=attributes[0],stride=first.inputClass&&!first.step?0:v.stride;
            if(stride%4||stride>limits.maxVertexBufferArrayStride||v.offset>v.buffer.size||attributes.some(a=>a.inputClass!==first.inputClass||(a.inputClass&&!a.step?0:v.stride)!==stride))throw Error("WebGPU draw vertex stride/rate");
            const base=v.offset-v.offset%4;
            const entries=attributes.map(a=>{const [format,bytes]=this.vertexFormats.get(a.format),offset=a.offset+v.offset%4;if(offset%Math.min(4,bytes)||(stride&&offset+bytes>stride)||offset+bytes>limits.maxVertexBufferArrayStride)throw Error("WebGPU draw vertex attribute range/alignment");return {shaderLocation:a.location,format,offset};});
            d.layouts.push({arrayStride:stride,stepMode:first.inputClass?"instance":"vertex",attributes:entries});
            d.vertexBindings.push({buffer:v.buffer,offset:base,originalOffset:v.offset,stride,inputClass:first.inputClass,attributes});
        }
        if(d.layouts.length>limits.maxVertexBuffers||d.attributes.length>limits.maxVertexAttributes)throw Error("WebGPU draw vertex device limit");
        d.pipelineKey=JSON.stringify([d.vsID,d.psID,d.layouts,d.primitive,d.targetDescription,d.depthDescription,d.sampleMask]);
    }
    validateBindings(d) {
        const limits=this.owner.device.limits,reflected=[...d.vs.bindings,...d.ps.bindings];
        if(reflected.length!==d.descriptors.size)throw Error("WebGPU draw descriptors differ from shader reflection");
        d.bindings=reflected.map(binding=>{
            const value=d.descriptors.get(binding.set+":"+binding.binding);
            if(!value||binding.count!==1)throw Error("WebGPU draw descriptor absent/array");
            const {kind,resource}=value,type=binding.descriptor_type;
            if(kind===0&&(type===6||type===7)){
                const limit=type===6?limits.maxUniformBufferBindingSize:limits.maxStorageBufferBindingSize;
                if(resource.size<binding.min_size||resource.size>limit)throw Error("WebGPU draw constant buffer size");
            } else if(kind===1&&type===2){
                if(![0,3,4,5,6].includes(resource.kind)||resource.texture===d.target.texture||resource.texture===d.depth?.texture)throw Error("WebGPU draw SRV/attachment conflict");
            } else if(kind===2&&type===7){
                if(resource.format||resource.offset%limits.minStorageBufferOffsetAlignment||resource.size<binding.min_size||resource.size>limits.maxStorageBufferBindingSize)throw Error("WebGPU draw raw buffer view profile");
            } else if(kind===3&&type===0){this.samplerDescription(resource);}
            else throw Error("WebGPU draw descriptor type/texel buffer lowering profile");
            return {binding,value};
        });
    }
    samplerDescription(s) {
        if((s[0]&~0xd5)||((s[0]&0x40)&&(s[0]&0x15)!==0x15)||s.slice(1,4).some(v=>v<1||v>3)||this.float(s[4])!==0)throw Error("WebGPU sampler filter/address/LOD bias profile");
        const lodMinClamp=Math.max(0,this.float(s[11])),lodMaxClamp=this.float(s[12]);
        if(!Number.isFinite(lodMinClamp)||!Number.isFinite(lodMaxClamp)||lodMaxClamp<lodMinClamp||((s[0]&0x40)&&(!s[5]||s[5]>16)))throw Error("WebGPU sampler LOD/anisotropy");
        const address=v=>["repeat","mirror-repeat","clamp-to-edge"][v-1],compare=this.compare(s[6]);
        return {addressModeU:address(s[1]),addressModeV:address(s[2]),addressModeW:address(s[3]),minFilter:s[0]&0x10?"linear":"nearest",magFilter:s[0]&4?"linear":"nearest",mipmapFilter:s[0]&1?"linear":"nearest",maxAnisotropy:s[0]&0x40?s[5]:1,lodMinClamp,lodMaxClamp,...(s[0]&0x80?{compare}:{})};
    }
    sampler(s) {
        const key=JSON.stringify(s);let entry=this.get(this.samplers,key);
        if(!entry){const description=this.samplerDescription(s),cost=1024+key.length*3;this.space(this.samplers,cost,128);this.charge(cost);try{entry={sampler:this.owner.device.createSampler(description),cost,last:++this.clock};this.samplers.set(key,entry);this.cacheBytes+=cost;}catch(error){this.uncharge(cost);throw error;}}
        return entry.sampler;
    }
    async indexBounds(d) {
        const cached=this.get(this.bounds,d.boundsKey);if(cached)return cached;
        const offset=d.indexOffset+d.first*d.indexBytes,size=d.count*d.indexBytes,begin=offset-offset%4,end=Math.ceil((offset+size)/4)*4;
        const bytes=await this.owner.stageRead(end-begin,(e,s)=>e.copyBufferToBuffer(d.index.buffer,begin,s,0,end-begin));this.check(d);this.indexReads++;
        const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.length),strip=d.topology===3||d.topology===5;
        let min=0xffffffff,max=0;
        for(let i=0;i<d.count;i++){const at=offset-begin+i*d.indexBytes,value=d.indexBytes===2?view.getUint16(at,true):view.getUint32(at,true);if(strip&&value===(d.indexBytes===2?65535:0xffffffff))continue;min=Math.min(min,value);max=Math.max(max,value);}
        const cost=128+d.boundsKey.length*3;this.space(this.bounds,cost,128);this.charge(cost);
        const entry={min,max,cost,last:++this.clock,id:d.indexID};this.bounds.set(d.boundsKey,entry);this.cacheBytes+=cost;return entry;
    }
    validateVertices(d,bounds) {
        const max=d.indexed?bounds.max+d.baseVertex:d.first+d.count-1;
        if(d.indexed&&bounds.min!==0xffffffff&&bounds.min+d.baseVertex<0)throw Error("WebGPU draw negative indexed vertex");
        for(const v of d.vertexBindings){const last=v.inputClass?d.firstInstance+d.instances-1:max;for(const a of v.attributes){const size=this.vertexFormats.get(a.format)[1];if(v.originalOffset+v.stride*last+a.offset+size>v.buffer.size)throw Error("WebGPU draw vertex buffer range");}}
    }
    async linkedShader(d) {
        const key=d.vsID+":"+d.psID,cached=this.get(this.linked,key);if(cached)return cached;
        const compiler=this.owner.options.shaderCompiler;if(!compiler||typeof compiler.linkVertex!=="function")throw Error("Portable stage linker unavailable");
        const result=await compiler.linkVertex(d.vs,d.ps);this.check(d);
        const cost=result.code.length*3+result.spirv.length+4096;
        return this.cached(this.linked,key,cost,32,async()=>{
            const module=this.owner.device.createShaderModule({code:result.code}),info=await module.getCompilationInfo();
            const errors=info.messages.filter(m=>m.type==="error");if(errors.length)throw Error(errors.map(m=>m.message).join("\n"));
            return {module,entryPoint:result.entryPoint,vsID:d.vsID,psID:d.psID};
        },()=>this.check(d));
    }
    async pipeline(d) {
        const linked=await this.linkedShader(d);this.check(d);
        const cost=4096+d.pipelineKey.length*8;
        return this.cached(this.pipelines,d.pipelineKey,cost,64,async()=>{
            const pipeline=await this.owner.device.createRenderPipelineAsync({layout:"auto",vertex:{module:linked.module,entryPoint:linked.entryPoint,buffers:d.layouts},fragment:{module:d.ps.shader,entryPoint:d.ps.entryPoint,targets:[d.targetDescription]},primitive:d.primitive,depthStencil:d.depthDescription,multisample:{count:1,mask:d.sampleMask,alphaToCoverageEnabled:false}});
            this.pipelineBuilds++;return {pipeline,vsID:d.vsID,psID:d.psID};
        },()=>this.check(d));
    }
    parameterPool() {
        if(this.parameters)return;
        const alignment=Math.max(16,this.owner.device.limits.minUniformBufferOffsetAlignment);this.parameterStride=alignment*2;
        const bytes=this.parameterStride*128;this.charge(bytes+this.parameterStride);
        try {this.parameters=this.owner.device.createBuffer({size:bytes,usage:GPUBufferUsage.UNIFORM|GPUBufferUsage.COPY_DST});this.parameterData=new Uint8Array(this.parameterStride);this.parameterBytes=bytes+this.parameterStride;}
        catch(error){this.uncharge(bytes+this.parameterStride);throw error;}
    }
    encode(d,entry,bounds) {
        this.check(d);this.validateVertices(d,bounds);this.parameterPool();
        const data=new DataView(this.parameterData.buffer),offset=this.batchDraws*this.parameterStride;
        data.setInt32(0,d.indexed?d.baseVertex:d.first,true);data.setInt32(this.parameterStride/2,d.firstInstance,true);
        this.owner.device.queue.writeBuffer(this.parameters,offset,this.parameterData);
        const groups=new Map(),add=(set,value)=>{if(!groups.has(set))groups.set(set,[]);groups.get(set).push(value);};
        for(const {binding,value} of d.bindings){let resource;const {kind}=value,v=value.resource;
            if(kind===0)resource={buffer:v.buffer,offset:0,size:v.size};
            else if(kind===1)resource=v.view;
            else if(kind===2)resource={buffer:v.buffer.buffer,offset:v.offset,size:v.size};
            else resource=this.sampler(v);
            add(binding.set,{binding:binding.binding,resource});
        }
        for(const binding of d.vs.internalBindings)add(3,{binding:binding.binding,resource:{buffer:this.parameters,offset:offset+(binding.binding?this.parameterStride/2:0),size:4}});
        const bound=[],maxGroup=groups.size?Math.max(...groups.keys()):-1;
        for(let set=0;set<=maxGroup;set++)bound.push(this.owner.device.createBindGroup({layout:entry.pipeline.getBindGroupLayout(set),entries:groups.get(set)||[]}));
        const descriptor={colorAttachments:[{view:d.target.view,loadOp:"load",storeOp:"store"}]};
        if(d.depth)descriptor.depthStencilAttachment={view:d.depth.view,depthLoadOp:"load",depthStoreOp:"store",...(d.depth.format.stencil?{stencilLoadOp:"load",stencilStoreOp:"store"}:{})};
        const pass=this.owner.commands().beginRenderPass(descriptor);
        pass.setPipeline(entry.pipeline);bound.forEach((group,set)=>pass.setBindGroup(set,group));
        d.vertexBindings.forEach((v,slot)=>pass.setVertexBuffer(slot,v.buffer.buffer,v.offset));
        pass.setViewport(...d.viewport);pass.setScissorRect(...d.scissorRect);pass.setBlendConstant(d.blendFactor);pass.setStencilReference(d.stencilRef&255);
        if(d.indexed){pass.setIndexBuffer(d.index.buffer,d.indexBytes===2?"uint16":"uint32",d.indexOffset);pass.drawIndexed(d.count,d.instances,d.first,d.baseVertex,d.firstInstance);}
        else pass.draw(d.count,d.instances,d.first,d.firstInstance);
        pass.end();this.batchDraws++;this.draws++;return 0;
    }
    draw(input) {
        const pending=this.owner.pending;
        if(pending&&pending.drawSource&&(pending.drawSource.length!==input.length||!pending.drawSource.every((v,i)=>v===input[i])))throw Error("WebGPU pending draw input changed");
        const d=this.parse(input),cached=this.get(this.pipelines,d.pipelineKey),bounds=d.indexed?this.get(this.bounds,d.boundsKey):null;
        if(pending){const result=this.owner.job("draw",()=>{});return this.encode(d,result.entry,result.bounds);}
        if(cached&&(!d.indexed||bounds)&&this.batchDraws<128){this.pipelineHits++;return this.encode(d,cached,bounds);}
        return this.owner.job("draw",async()=>{
            this.owner.reserve(input.length);const source=input.slice();this.owner.pending.drawSource=source;this.owner.pending.bytes=source.length;
            const range=d.indexed?(bounds||await this.indexBounds(d)):null;this.check(d);this.validateVertices(d,range);
            const entry=cached||await this.pipeline(d);this.check(d);
            if(this.batchDraws===128){this.owner.submit();await this.owner.device.queue.onSubmittedWorkDone();this.check(d);this.batchDraws=0;this.drains++;}
            return {entry,bounds:range};
        });
    }
    invalidateBuffer(id) {this.versions.set(id,(this.versions.get(id)||0)+1);for(const [key,value] of this.bounds)if(value.id===id)this.remove(this.bounds,key);}
    release(id,kind) {
        if(kind==="buffer"){this.invalidateBuffer(id);this.versions.delete(id);}
        if(kind==="shader")for(const map of [this.pipelines,this.linked])for(const [key,value] of map)if(value.vsID===id||value.psID===id)this.remove(map,key);
    }
    stats() {return {draws:this.draws,pipeline_hits:this.pipelineHits,pipeline_builds:this.pipelineBuilds,index_reads:this.indexReads,drains:this.drains,pipelines:this.pipelines.size,linked_shaders:this.linked.size,samplers:this.samplers.size,index_ranges:this.bounds.size,owned_bytes:this.ownedBytes,cache_bytes:this.cacheBytes,parameter_bytes:this.parameterBytes||0};}
    close() {
        if(this.closed)return;
        if(this.parameters)this.parameters.destroy();this.parameters=null;this.parameterData=null;
        for(const map of this.maps)map.clear();this.versions.clear();this.owner.liveBytes-=this.ownedBytes;this.ownedBytes=0;this.cacheBytes=0;this.parameterBytes=0;this.closed=true;
    }
};
