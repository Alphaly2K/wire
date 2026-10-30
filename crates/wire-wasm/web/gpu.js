// Same-WebContent WebGPU owner. No App messages, native renderer or guest ABI.
// Resources carry process-local IDs; WebGPU sees only checked host descriptions.
globalThis.WireWebGPU = class WireWebGPU {
    static contexts = new WeakMap();
    static async create(memory, options = {}) {
        const gpu = options.gpu || (globalThis.navigator && navigator.gpu);
        if (!gpu) throw Error("WebGPU unavailable in this execution environment");
        const adapter = await gpu.requestAdapter(options.adapterOptions || {});
        if (!adapter) throw Error("WebGPU adapter unavailable");
        const optional = ["texture-compression-bc", "timestamp-query", "depth-clip-control", "float32-blendable", "rg11b10ufloat-renderable", "depth32float-stencil8"];
        const requiredLimits = {};
        for (const name of ["maxBindGroups","maxBindingsPerBindGroup","maxUniformBuffersPerShaderStage","maxSampledTexturesPerShaderStage","maxSamplersPerShaderStage","maxVertexBuffers","maxVertexAttributes","maxTextureDimension2D","maxTextureDimension3D","maxTextureArrayLayers","maxBufferSize","maxUniformBufferBindingSize","maxStorageBufferBindingSize","maxStorageBuffersPerShaderStage","maxStorageTexturesPerShaderStage","minUniformBufferOffsetAlignment","minStorageBufferOffsetAlignment"])
            if (adapter.limits[name] !== undefined) requiredLimits[name] = adapter.limits[name];
        const device = await adapter.requestDevice({requiredFeatures: optional.filter(f => adapter.features.has(f)), requiredLimits});
        try { return new WireWebGPU(memory, gpu, adapter, device, options); }
        catch (error) { device.destroy(); throw error; }
    }
    constructor(memory, gpu, adapter, device, options) {
        this.memory = memory; this.gpu = gpu; this.adapter = adapter; this.device = device;
        this.options = options; this.objects = new Map(); this.nextID = 1;
        this.liveBytes = 0; this.stagingBytes = 0;
        this.byteLimit = options.resourceBudgetBytes ?? 512 * 1024 * 1024;
        if (!Number.isSafeInteger(this.byteLimit) || this.byteLimit < 1 || this.byteLimit > 2147483648)
            throw Error("Invalid WebGPU resource budget");
        this.encoder = null; this.pending = null; this.generation = 0;
        this.failure = null; this.closed = false; this.calls = 0; this.submissions = 0;
        this.readbacks = 0; this.presented = 0; this.presentPipeline = null;
        this.surfaces = new Map(); this.drawBackend = null;
        device.lost.then(info => { if (!this.closed) this.failure = `WebGPU device lost: ${info.message || info.reason}`; });
        device.addEventListener("uncapturederror", event => { if (!this.closed) this.failure = `WebGPU validation: ${event.error.message}`; });
        this.formats = new Map();
        // DXGI storage formats; unsupported reinterpretations and optional formats
        // are not included in the advertised profile.
        const entries = [
            [2,"rgba32float",16],[3,"rgba32uint",16],[4,"rgba32sint",16],[10,"rgba16float",8],[12,"rgba16uint",8],[14,"rgba16sint",8],[16,"rg32float",8],[17,"rg32uint",8],[18,"rg32sint",8],
            [24,"rgb10a2unorm",4],[26,"rg11b10ufloat",4],
            [27,"rgba8unorm",4],[28,"rgba8unorm",4],[29,"rgba8unorm-srgb",4],[30,"rgba8uint",4],[31,"rgba8snorm",4],[32,"rgba8sint",4],
            [34,"rg16float",4],[36,"rg16uint",4],[38,"rg16sint",4],[39,"r32float",4],[40,"depth32float",4],
            [41,"r32float",4],[42,"r32uint",4],[43,"r32sint",4],[44,"depth24plus-stencil8",4],[45,"depth24plus-stencil8",4],
            [49,"rg8unorm",2],[50,"rg8uint",2],[51,"rg8snorm",2],[52,"rg8sint",2],[53,"r16float",2],[54,"r16float",2],[55,"depth16unorm",2],[57,"r16uint",2],[59,"r16sint",2],[61,"r8unorm",1],[62,"r8uint",1],[63,"r8snorm",1],[64,"r8sint",1],
            [87,"bgra8unorm",4],[90,"bgra8unorm",4],[91,"bgra8unorm-srgb",4]
        ];
        for (const [id, name, bytes] of entries) this.formats.set(id, {name, bytes, block: 1, depth: name.startsWith("depth"), stencil: name.includes("stencil")});
        // Aspect views name the WebGPU aspect format, not a second allocation.
        for (const [id,name,bytes] of [[46,"depth24plus",4],[47,"stencil8",1],[56,"depth16unorm",2]])
            this.formats.set(id,{name,bytes,block:1,depth:id!==47,stencil:id===47,viewOnly:true});
        if(device.features.has("depth32float-stencil8")) {
            for(const id of [19,20])this.formats.set(id,{name:"depth32float-stencil8",bytes:8,block:1,depth:true,stencil:true});
            for(const [id,name,bytes] of [[21,"depth32float",4],[22,"stencil8",1]])
                this.formats.set(id,{name,bytes,block:1,depth:id===21,stencil:id===22,viewOnly:true});
        }
        if (device.features.has("texture-compression-bc")) {
            for (const [id,name,bytes] of [[71,"bc1-rgba-unorm",8],[72,"bc1-rgba-unorm-srgb",8],[74,"bc2-rgba-unorm",16],[75,"bc2-rgba-unorm-srgb",16],[77,"bc3-rgba-unorm",16],[78,"bc3-rgba-unorm-srgb",16],[80,"bc4-r-unorm",8],[81,"bc4-r-snorm",8],[83,"bc5-rg-unorm",16],[84,"bc5-rg-snorm",16],[95,"bc6h-rgb-ufloat",16],[96,"bc6h-rgb-float",16],[98,"bc7-rgba-unorm",16],[99,"bc7-rgba-unorm-srgb",16]])
                this.formats.set(id, {name,bytes,block:4,depth:false,stencil:false});
        }
    }
    profile() {
        const info = this.adapter.info || {};
        let name = `Wire WebGPU ${info.description || info.device || info.architecture || "adapter"}`;
        const encode = s => new TextEncoder().encode(s);
        while (encode(name).length > 256) name = name.slice(0, -1);
        const encoded = encode(name), formats = [];
        const vertices = new Set([2,3,4,6,7,8,10,11,12,13,14,16,17,18,24,28,30,31,32,34,35,36,37,38,41,42,43,49,50,51,52]);
        for (const [id,f] of this.formats) {
            let mask = 0x20 | 0x100 | 0x200 | 0x1000;
            if (f.depth) mask = 0x20 | 0x100 | 0x200 | 0x400 | 0x1000 | 0x10000;
            else if (f.block === 1) {
                if (f.name !== "rg11b10ufloat" || this.device.features.has("rg11b10ufloat-renderable")) mask |= 0x4000;
                if (!/uint$|sint$/.test(f.name) && (![2,16,41].includes(id) || this.device.features.has("float32-blendable"))) mask |= 0x8000;
                if (/uint$|sint$/.test(f.name)) mask &= ~0x200;
            }
            if (/snorm$/.test(f.name)) mask &= ~0xc000;
            if(f.viewOnly)mask=0x200|0x1000;
            if ([19,27,39,44,53,90].includes(id)) mask = 0x320;
            if (vertices.has(id)) mask |= 3;
            if ([42,57].includes(id)) mask |= 5;
            if ([28,87].includes(id)) mask |= 0x80000;
            formats.push([id,mask]); vertices.delete(id);
        }
        for (const id of vertices) formats.push([id,3 | ([42,57].includes(id) ? 4 : 0)]);
        if (!formats.some(([id])=>id===57)) formats.push([57,5]);
        const data = new Uint8Array(72 + formats.length * 8 + encoded.length), words = new DataView(data.buffer);
        words.setUint32(0,1,true);words.setUint32(4,formats.length,true);words.setUint32(8,encoded.length,true);
        // A virtual adapter identity and explicit process budget, not physical VRAM.
        data.set([0x57,0x49,0x52,0x45,0x57,0x45,0x42,0x47,0x50,0x55,1,0,0,0,0,0],16);
        words.setBigUint64(48,BigInt(this.byteLimit),true);
        formats.forEach(([id,mask],i)=>{words.setUint32(72+i*8,id,true);words.setUint32(76+i*8,mask,true);});
        data.set(encoded,72+formats.length*8);return data;
    }
    checkRange(at, length) {
        if (!Number.isSafeInteger(at) || !Number.isSafeInteger(length) || at < 0 || length < 0 || at > this.memory.buffer.byteLength || length > this.memory.buffer.byteLength - at) throw Error("WebGPU bridge memory range");
    }
    dispatch(op, pointer) {
        pointer >>>= 0; let header;
        try {
            this.checkRange(pointer,88); header = new Uint32Array(this.memory.buffer,pointer,22);
            const [inputAt,inputLength,outputAt,outputLength] = header;
            this.checkRange(inputAt,inputLength);this.checkRange(outputAt,outputLength);
            if (this.closed) throw Error("WebGPU owner closed");
            if (this.failure && op !== 16) throw Error(this.failure);
            this.calls++;
            const result = this.operation(op,Array.from(header.subarray(6)),new Uint8Array(this.memory.buffer,inputAt,inputLength),new Uint8Array(this.memory.buffer,outputAt,outputLength));
            return result === undefined ? 0 : result;
        } catch (error) {
            if (error === WireWebGPU.PENDING) return -2;
            if (header) {
                const at=header[4], length=header[5];
                try {this.checkRange(at,length);const target=new Uint8Array(this.memory.buffer,at,length);target.fill(0);if(length)new TextEncoder().encodeInto(String(error.message || error),target.subarray(0,length-1));} catch (_) {}
            }
            return -1;
        }
    }
    reserve(bytes = 0) {
        if(this.objects.size>=4096||this.nextID>=0x7fffffff||!Number.isSafeInteger(bytes)||bytes<0||bytes>256*1024*1024)throw Error("WebGPU resource capacity exceeded");
        if(this.drawBackend&&this.liveBytes+this.stagingBytes+(this.pending?.bytes||0)+bytes>this.byteLimit)this.drawBackend.reclaim(bytes);
        if (this.liveBytes + this.stagingBytes + (this.pending?.bytes || 0) + bytes > this.byteLimit)
            throw Error("WebGPU resource capacity exceeded");
    }
    put(kind, value, bytes = 0) {const id=this.nextID++;this.objects.set(id,{kind,value,bytes});this.liveBytes+=bytes;return id;}
    object(id, kind) {const entry=this.objects.get(id);if(!entry || entry.kind!==kind)throw Error(`Invalid WebGPU ${kind} resource`);return entry.value;}
    commands() {return this.encoder || (this.encoder=this.device.createCommandEncoder());}
    submit() {if(this.encoder){this.device.queue.submit([this.encoder.finish()]);this.encoder=null;this.submissions++;}}
    subresource(texture, sub) {
        const mip = sub % texture.mips, layer = Math.floor(sub / texture.mips);
        if (!Number.isInteger(sub) || sub < 0 || mip >= texture.mips || layer >= (texture.volume ? 1 : texture.layers)) throw Error("WebGPU texture subresource range");
        const width=Math.max(1,texture.width>>mip),height=Math.max(1,texture.height>>mip),depth=texture.volume?Math.max(1,texture.layers>>mip):1;
        const block=texture.format.block,physicalWidth=Math.ceil(width/block)*block,physicalHeight=Math.ceil(height/block)*block;
        // Compressed mip tails keep their logical size, but WebGPU transfers
        // whole storage blocks even for a 1x1 or 2x2 logical subresource.
        return {mip,layer,width,height,depth,physicalWidth,physicalHeight,row:physicalWidth/block*texture.format.bytes,rows:physicalHeight/block};
    }
    imageView(b,formatID,mip,mips,layer,layers,kind) {
        let format=this.formats.get(formatID),aspect="all";
        if(!format||![mip,mips,layer,layers,kind].every(n=>Number.isInteger(n)&&n>=0)||!mips||!layers||mip>=b.mips||mips>b.mips-mip||layer>=(b.volume?1:b.layers)||layers>(b.volume?1:b.layers)-layer||kind>6)
            throw Error("WebGPU image view profile/range");
        const sampled=kind===0||kind>=3,srv=kind===0||kind===6;
        if((sampled&&!(b.bind&8))||(kind===1&&(!(b.bind&0x20)||mips!==1||format.depth||format.stencil))||(kind===2&&(!(b.bind&0x40)||mips!==1||!format.depth)))
            throw Error("WebGPU image view usage/aspect");
        let compatible=formatID===b.formatID;
        if(b.bind&0x400)compatible||=([28,29].includes(b.formatID)&&[28,29].includes(formatID))||([87,91].includes(b.formatID)&&[87,91].includes(formatID));
        if(b.formatID===27)compatible=formatID>=28&&formatID<=32;
        if(b.formatID===90)compatible=formatID===87||formatID===91;
        if(b.formatID===39||b.formatID===53) {
            const wide=b.formatID===39,depthID=wide?40:55,sampleID=wide?41:56;
            if(b.format.depth) {
                compatible=(kind===2&&formatID===depthID)||(srv&&formatID===sampleID);
                format=b.format;aspect=kind===2?"all":"depth-only";
            } else compatible=wide?formatID>=41&&formatID<=43:formatID===54||(formatID>=56&&formatID<=59);
        }
        if(b.formatID===19||b.formatID===44) {
            const wide=b.formatID===19,depthID=wide?20:45,depthSample=wide?21:46,stencilSample=wide?22:47;
            compatible=(kind===2&&formatID===depthID)||(srv&&(formatID===depthSample||formatID===stencilSample));
            aspect=kind===2?"all":formatID===depthSample?"depth-only":"stencil-only";
        } else if(b.formatID===20&&kind!==2)compatible=false;
        if(!compatible)throw Error("WebGPU image view format family");
        // WebGPU only permits sRGB-pair reinterpretation. Other typeless color
        // aliases need a separate lowering, never a hidden shadow allocation.
        const expected=aspect==="depth-only"?b.format.name.replace(/-stencil8$/,""):aspect==="stencil-only"?"stencil8":b.format.name;
        if(format.name.replace(/-srgb$/,"")!==expected.replace(/-srgb$/,""))throw Error("WebGPU image view format reinterpretation unavailable");
        let dimension=layers>1?"2d-array":"2d";
        if(b.volume) {
            if(kind!==5||layer!==0||layers!==1)throw Error("WebGPU volume view range/dimension");
            dimension="3d";
        } else if(kind===5)throw Error("WebGPU volume view requires volume texture");
        else if(kind===6)dimension="2d-array";
        else if(kind===3||kind===4) {
            if(!(b.bind&0x100)||layer%6||layers%6||(kind===3&&layers!==6))throw Error("WebGPU cube view range");
            dimension=kind===3?"cube":"cube-array";
        }
        const description={format:format.name,aspect,dimension,baseMipLevel:mip,mipLevelCount:mips,...(b.volume?{}:{baseArrayLayer:layer,arrayLayerCount:layers})};
        return {format,description};
    }
    job(key, create) {
        if (this.pending) {
            if (this.pending.key!==key) throw Error("Another WebGPU operation is pending");
            if (!this.pending.done) throw WireWebGPU.PENDING;
            const pending=this.pending;this.pending=null;
            if (pending.error) throw pending.error;
            return pending.value;
        }
        const generation=this.generation;
        const pending={key,done:false};this.pending=pending;
        try {pending.promise=Promise.resolve(create()).then(value=>{if(generation!==this.generation)return;pending.value=value;pending.done=true;},error=>{if(generation!==this.generation)return;pending.error=error;pending.done=true;});}
        catch(error){this.pending=null;throw error;}
        throw WireWebGPU.PENDING;
    }
    prepareJob(key, create) {
        if(this.pending){
            if(this.pending.key!==key)throw Error("Another WebGPU operation is pending");
            if(!this.pending.done)throw WireWebGPU.PENDING;
            if(this.pending.error){const error=this.pending.error;this.pending=null;throw error;}
            return 0;
        }
        return this.job(key,create);
    }
    async stageRead(size, encode) {
        const allocation=Math.ceil(size/4)*4;
        if(this.drawBackend&&allocation<=256*1024*1024&&this.liveBytes+this.stagingBytes+(this.pending?.bytes||0)+allocation>this.byteLimit)this.drawBackend.reclaim(allocation);
        if (allocation > 256*1024*1024 || this.liveBytes+this.stagingBytes+(this.pending?.bytes||0)+allocation>this.byteLimit) throw Error("WebGPU readback capacity exceeded");
        this.stagingBytes+=allocation;
        let staging;
        try {
            staging=this.device.createBuffer({size:allocation,usage:GPUBufferUsage.COPY_DST|GPUBufferUsage.MAP_READ});
            encode(this.commands(),staging);this.submit();this.readbacks++;
            await staging.mapAsync(GPUMapMode.READ);
            return new Uint8Array(staging.getMappedRange()).slice();
        } finally {if(staging){if(staging.mapState==="mapped")staging.unmap();staging.destroy();}this.stagingBytes-=allocation;}
    }
    async flush() {this.submit();if(this.pending)await this.pending.promise;if(this.failure)throw Error(this.failure);}
    operation(op,a,input,output) {
        const u=GPUBufferUsage,t=GPUTextureUsage;
        switch(op) {
        case 1: {
            const size=Math.ceil(input.length/4)*4;this.reserve(size);
            if(size>this.device.limits.maxBufferSize)throw Error("WebGPU buffer device limit");
            this.submit();const buffer=this.device.createBuffer({size,usage:u.COPY_SRC|u.COPY_DST|u.VERTEX|u.INDEX|u.UNIFORM|u.STORAGE,mappedAtCreation:true});
            new Uint8Array(buffer.getMappedRange()).set(input);buffer.unmap();
            return this.put("buffer",{buffer,size:input.length,allocation:size},size);
        }
        case 2: {
            const b=this.object(a[0],"buffer"),offset=a[1];
            if(offset>b.size||input.length>b.size-offset)throw Error("WebGPU buffer write range");
            if(!input.length)return 0;
            if(this.drawBackend)this.drawBackend.invalidateBuffer(a[0]);
            if(offset%4===0&&input.length%4===0){this.submit();this.device.queue.writeBuffer(b.buffer,offset,input);return 0;}
            const begin=offset&~3,end=Math.ceil((offset+input.length)/4)*4;
            const key=`write:${a[0]}:${offset}:${input.length}`;
            if(this.pending)return this.job(key,()=>{});
            const data=input.slice();return this.job(key,async()=>{const old=await this.stageRead(end-begin,(e,s)=>e.copyBufferToBuffer(b.buffer,begin,s,0,end-begin));old.set(data,offset-begin);this.device.queue.writeBuffer(b.buffer,begin,old);return 0;});
        }
        case 3: case 17: {
            const b=this.object(a[0],"buffer"),offset=a[1],size=op===17?a[2]:output.length;
            if(offset>b.size||size>b.size-offset)throw Error("WebGPU buffer read range");if(!size)return 0;
            const begin=offset&~3,end=Math.ceil((offset+size)/4)*4;
            const key=`buffer:${a[0]}:${offset}:${size}`,create=()=>this.stageRead(end-begin,(e,s)=>e.copyBufferToBuffer(b.buffer,begin,s,0,end-begin));
            if(op===17)return this.prepareJob(key,create);
            const bytes=this.job(key,create);
            output.set(bytes.subarray(offset-begin,offset-begin+size));return 0;
        }
        case 4: {
            const [width,height,mips,layers,formatID,bind]=a,volume=!!(bind&0x200);
            const storageID=formatID===39?(bind&0x40?40:41):formatID===53?(bind&0x40?55:54):formatID;
            const format=this.formats.get(storageID);
            if(!format||format.viewOnly||![width,height,mips,layers,bind].every(n=>Number.isInteger(n)&&n>=0)||!width||!height||!mips||!layers||bind&~0x768||width>this.device.limits.maxTextureDimension2D||height>this.device.limits.maxTextureDimension2D||mips>1+Math.floor(Math.log2(Math.max(width,height,volume?layers:1)))||(!volume&&layers>this.device.limits.maxTextureArrayLayers))throw Error("WebGPU texture descriptor/device limit");
            if((bind&0x100)&&(volume||width!==height||layers%6))throw Error("WebGPU cube texture dimensions/flags");
            if((formatID===19&&!(bind&0x40))||((bind&0x40)&&!format.depth)||((bind&0x20)&&format.depth)||(bind&0x20&&/snorm$/.test(format.name)))throw Error("WebGPU texture attachment usage/format");
            if(format.block>1&&(width%format.block||height%format.block))throw Error("WebGPU compressed texture base extent alignment");
            if(volume&&(format.depth||format.block!==1||Math.max(width,height,layers)>this.device.limits.maxTextureDimension3D))throw Error("Unsupported WebGPU volume format/extent");
            let bytes=0;for(let mip=0;mip<mips;mip++)bytes+=Math.ceil(Math.max(1,width>>mip)/format.block)*Math.ceil(Math.max(1,height>>mip)/format.block)*format.bytes*(volume?Math.max(1,layers>>mip):layers);
            this.reserve(bytes);
            let usage=t.COPY_SRC|t.COPY_DST|t.TEXTURE_BINDING;
            if(bind&0x60){if(format.block!==1||volume)throw Error("Unsupported WebGPU attachment format/dimension");usage|=t.RENDER_ATTACHMENT;}
            const viewFormats = ["rgba8unorm","rgba8unorm-srgb"].includes(format.name)?["rgba8unorm","rgba8unorm-srgb"]:["bgra8unorm","bgra8unorm-srgb"].includes(format.name)?["bgra8unorm","bgra8unorm-srgb"]:[];
            const texture=this.device.createTexture({size:[width,height,layers],dimension:volume?"3d":"2d",format:format.name,mipLevelCount:mips,usage,viewFormats});
            return this.put("texture",{texture,width,height,mips,layers,format,formatID,bind,volume},bytes);
        }
        case 5: {
            const b=this.object(a[0],"texture"),s=this.subresource(b,a[1]);
            if(b.format.depth)throw Error("WebGPU depth upload is not supported");
            if(input.length!==s.row*s.rows*s.depth)throw Error("WebGPU texture upload size");
            this.submit();this.device.queue.writeTexture({texture:b.texture,mipLevel:s.mip,origin:[0,0,s.layer]},input,{bytesPerRow:s.row,rowsPerImage:s.rows},[s.physicalWidth,s.physicalHeight,s.depth]);return 0;
        }
        case 6: case 18: {
            const b=this.object(a[0],"texture"),s=this.subresource(b,a[1]);
            if(b.format.stencil||b.format.name==="depth16unorm")throw Error("WebGPU depth/stencil readback format unavailable");
            const length=op===18?a[2]:output.length;
            if(length!==s.row*s.rows*s.depth)throw Error("WebGPU texture readback size");
            const row=Math.ceil(s.row/256)*256,size=row*s.rows*s.depth;
            const key=`texture:${a[0]}:${a[1]}:${length}`,create=()=>this.stageRead(size,(e,staging)=>e.copyTextureToBuffer({texture:b.texture,mipLevel:s.mip,origin:[0,0,s.layer],aspect:b.format.depth?"depth-only":"all"},{buffer:staging,bytesPerRow:row,rowsPerImage:s.rows},[s.physicalWidth,s.physicalHeight,s.depth]));
            if(op===18)return this.prepareJob(key,create);
            const bytes=this.job(key,create);
            for(let y=0;y<s.rows*s.depth;y++)output.set(bytes.subarray(y*row,y*row+s.row),y*s.row);return 0;
        }
        case 7: {
            const dst=this.object(a[0],"texture"),src=this.object(a[1],"texture"),d=this.subresource(dst,a[2]),s=this.subresource(src,a[3]);
            const [x,y,left,top,right,bottom]=a.slice(4,10),width=right-left,height=bottom-top;
            if(dst.volume!==src.volume||s.depth>d.depth||(dst===src&&a[2]===a[3])||width<=0||height<=0||right>s.width||bottom>s.height||x+width>d.width||y+height>d.height||dst.format.name.replace(/-srgb$/,"")!==src.format.name.replace(/-srgb$/,""))throw Error("WebGPU texture copy range/format");
            const block=src.format.block;if(x%block||y%block||left%block||top%block||(width%block&&right!==s.width)||(height%block&&bottom!==s.height))throw Error("WebGPU compressed copy origin/extent");
            if(src.format.depth&&(left||top||x||y||width!==s.width||height!==s.height||width!==d.width||height!==d.height))throw Error("WebGPU depth copy requires complete subresource");
            this.commands().copyTextureToTexture({texture:src.texture,mipLevel:s.mip,origin:[left,top,s.layer]},{texture:dst.texture,mipLevel:d.mip,origin:[x,y,d.layer]},[Math.ceil(width/block)*block,Math.ceil(height/block)*block,s.depth]);return 0;
        }
        case 8: {
            const b=this.object(a[0],"texture");const [mip,mips,layer,layers,kind]=a.slice(2,7);
            const {format,description}=this.imageView(b,a[1],mip,mips,layer,layers,kind);
            this.reserve();
            return this.put("view",{view:b.texture.createView(description),texture:b,format,mip,mips,layer,layers,kind,description});
        }
        case 9: case 10: {
            const v=this.object(a[0],"view");if(v.mips!==1||v.texture.volume||(op===9&&(v.format.depth||v.kind!==1))||(op===10&&(!v.format.depth||v.kind!==2)))throw Error("WebGPU clear attachment profile");
            const float=bits=>new Float32Array(new Uint32Array([bits]).buffer)[0];
            const flags=a[1], clearDepth=!!(flags&1), clearStencil=!!(flags&2)&&v.format.stencil;
            let depthValue=0;
            if(op===10){
                if(flags&~3||a[3]>255)throw Error("WebGPU depth clear flags/value");
                if(!clearDepth&&!clearStencil)return 0;
                if(clearDepth){depthValue=float(a[2]);if(Number.isNaN(depthValue))throw Error("Invalid depth clear value");depthValue=Math.min(1,Math.max(0,depthValue));}
            }
            for(let layer=0;layer<v.layers;layer++) {
                const view=v.layers===1?v.view:v.texture.texture.createView({...v.description,dimension:"2d",baseArrayLayer:v.layer+layer,arrayLayerCount:1});
                let descriptor;
                if(op===9)descriptor={colorAttachments:[{view,loadOp:"clear",storeOp:"store",clearValue:a.slice(1,5).map(float)}]};
                else {const depth={view,depthLoadOp:clearDepth?"clear":"load",depthStoreOp:"store",depthClearValue:depthValue};if(v.format.stencil)Object.assign(depth,{stencilLoadOp:clearStencil?"clear":"load",stencilStoreOp:"store",stencilClearValue:a[3]});descriptor={colorAttachments:[],depthStencilAttachment:depth};}
                this.commands().beginRenderPass(descriptor).end();
            }return 0;
        }
        case 11: {const buffer=this.object(a[0],"buffer");if(a[2]>buffer.size||a[3]>buffer.size-a[2])throw Error("WebGPU buffer view range");this.reserve();return this.put("bufferView",{buffer,format:a[1],offset:a[2],size:a[3]});}
        case 12: return this.compileShader(a[0],input,output);
        case 13: {
            if(this.options.drawBackend)return this.options.drawBackend.draw(this,new Uint32Array(input.buffer,input.byteOffset,input.length/4));
            if(!globalThis.WireWebGPUDraw)throw Error("WebGPU D3D draw backend is not installed");
            if(!this.drawBackend)this.drawBackend=new WireWebGPUDraw(this);
            return this.drawBackend.draw(input);
        }
        case 14: return this.present(a[0],BigInt(a[1])+(BigInt(a[2])<<32n),a[3]);
        case 15: {
            if(!this.device.features.has("timestamp-query"))throw Error("WebGPU timestamp queries unavailable");
            const data=this.job("timestamp",async()=>{const query=this.device.createQuerySet({type:"timestamp",count:2});let resolve;
                try{resolve=this.device.createBuffer({size:256,usage:u.QUERY_RESOLVE|u.COPY_SRC});return await this.stageRead(8,(e,s)=>{e.beginComputePass({timestampWrites:{querySet:query,beginningOfPassWriteIndex:0,endOfPassWriteIndex:1}}).end();e.resolveQuerySet(query,1,1,resolve,0);e.copyBufferToBuffer(resolve,0,s,0,8);});}finally{query.destroy();if(resolve)resolve.destroy();}});
            if(output.length!==16)throw Error("WebGPU timestamp output layout");output.set(data.subarray(0,8));new DataView(output.buffer,output.byteOffset,16).setBigUint64(8,1000000000n,true);return 0;
        }
        case 16: {
            const entry=this.objects.get(a[0]);if(!entry)throw Error("Invalid WebGPU release");
            try {this.submit();}
            finally {
                try {if(this.drawBackend)this.drawBackend.release(a[0],entry.kind);if(entry.kind==="buffer")entry.value.buffer.destroy();if(entry.kind==="texture")entry.value.texture.destroy();}
                finally {this.objects.delete(a[0]);this.liveBytes-=entry.bytes;}
            }return 0;
        }
        default: throw Error("Unknown WebGPU bridge operation");
        }
    }
    compileShader(stage,input,output) {
        if(!this.options.shaderCompiler || typeof this.options.shaderCompiler.compileDXBC!=="function")throw Error("A portable DXBC to WGSL compiler is not installed");
        if(![1,2,4].includes(stage)||input.length<32||input.length>1024*1024)throw Error("WebGPU shader input capacity/stage");
        if(output.length!==8+128*16+32*4)throw Error("WebGPU shader reflection output layout");
        const key=`shader:${stage}:${input.length}`;
        if(this.pending&&this.pending.shaderSource&&!this.pending.shaderSource.every((v,i)=>v===input[i]))throw Error("WebGPU pending shader input changed");
        const generation=this.generation;
        const result=this.job(key,async()=>{
            this.reserve(input.length);
            const source=input.slice(),pending=this.pending;pending.shaderSource=source;pending.bytes=source.length;
            const compiled=await this.options.shaderCompiler.compileDXBC(source,stage);
            if(this.closed||generation!==this.generation)throw Error("WebGPU shader compilation cancelled");
            const result=this.shaderPackage(compiled,stage,source.length);
            // Charge pending output as soon as it is retained, before the guest
            // retries the suspended API. Consumption transfers it to liveBytes.
            pending.bytes=0;this.reserve(result.cost);pending.bytes=result.cost;
            const shader=this.device.createShaderModule({code:result.code});const info=await shader.getCompilationInfo();
            if(this.closed||generation!==this.generation)throw Error("WebGPU shader compilation cancelled");
            const errors=info.messages.filter(m=>m.type==="error");if(errors.length)throw Error(errors.map(m=>m.message).join("\n"));
            return {...result,shader};
        });
        const words=new DataView(output.buffer,output.byteOffset,output.length);output.fill(0);words.setUint32(0,result.bindings.length,true);words.setUint32(4,result.inputs.length,true);
        result.bindings.forEach((b,i)=>[b.set,b.binding,b.descriptor_type,b.count].forEach((v,n)=>words.setUint32(8+i*16+n*4,v,true)));
        result.inputs.forEach((v,i)=>words.setUint32(8+128*16+i*4,v,true));this.reserve(result.cost);return this.put("shader",result,result.cost);
    }
    shaderPackage(result,stage,sourceBytes) {
        if(!result||result.stage!==stage||typeof result.code!=="string"||result.code.length>16*1024*1024||typeof result.entryPoint!=="string"||!result.entryPoint.length||result.entryPoint.length>256||!(result.spirv instanceof Uint8Array)||result.spirv.length<20||result.spirv.length>16*1024*1024||result.spirv.length%4)
            throw Error("Invalid portable shader compilation result");
        for(const [name,limit] of [["bindings",128],["inputs",32],["internalBindings",2],["inputInterface",32],["outputInterface",32],["workgroupSize",3]])
            if(!Array.isArray(result[name])||result[name].length>limit)throw Error("Invalid portable shader reflection");
        const integer=n=>Number.isInteger(n)&&n>=0&&n<=0xffffffff;
        const set=stage===1?0:stage===2?1:2;
        const seen=new Set();
        for(const b of result.bindings) {
            if(!b||b.set!==set||!integer(b.binding)||b.binding>165||!integer(b.descriptor_type)||!integer(b.count)||b.count!==1||!integer(b.min_size)||typeof b.shape!=="string"||b.shape.length>256||seen.has(b.binding))throw Error("Invalid portable shader binding");
            seen.add(b.binding);
        }
        if(result.inputs.some(n=>!integer(n)||n>=32)||new Set(result.inputs).size!==result.inputs.length)throw Error("Invalid portable shader input");
        const metadata={entryPoint:result.entryPoint,stage,bindings:result.bindings,inputs:result.inputs,internalBindings:result.internalBindings,inputInterface:result.inputInterface,outputInterface:result.outputInterface,workgroupSize:result.workgroupSize};
        const encoded=JSON.stringify(metadata);
        if(encoded.length>256*1024)throw Error("WebGPU shader metadata capacity");
        // Keep only the public compiler contract; do not retain arbitrary
        // compiler objects. JSON copies reflection; SPIR-V has its own owner.
        const cost=sourceBytes+result.code.length*3+result.spirv.length+encoded.length*8+4096;
        this.reserve(cost);
        return {...JSON.parse(encoded),code:result.code,spirv:result.spirv.slice(),cost};
    }
    present(id,window,interval) {
        if(interval>4)throw Error("WebGPU present interval");
        const source=this.object(id,"texture");const canvas=this.options.getCanvas?this.options.getCanvas(window):this.options.canvas;
        if(!canvas)throw Error("WebGPU presentation canvas is not configured");
        if(source.format.depth||source.volume||source.layers!==1)throw Error("WebGPU presentation texture profile");
        const format=this.gpu.getPreferredCanvasFormat(),context=canvas.getContext("webgpu");
        if(!context)throw Error("WebGPU canvas context unavailable");
        if(canvas.width!==source.width)canvas.width=source.width;if(canvas.height!==source.height)canvas.height=source.height;
        let surface=this.surfaces.get(canvas);
        if(surface)this.surfaces.delete(canvas);
        else if(this.surfaces.size>=16){const oldest=this.surfaces.keys().next().value;this.unconfigure(oldest,this.surfaces.get(oldest));this.surfaces.delete(oldest);}
        if(!surface||surface.width!==canvas.width||surface.height!==canvas.height||surface.format!==format||WireWebGPU.contexts.get(canvas)!==this)
            context.configure({device:this.device,format,alphaMode:"opaque"});
        WireWebGPU.contexts.set(canvas,this);
        this.surfaces.set(canvas,{context,width:canvas.width,height:canvas.height,format});
        if(!this.presentPipeline){const code=`@group(0) @binding(0) var image: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f {let p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3));return vec4f(p[i],0,1);}
@fragment fn fs(@builtin(position) p:vec4f)->@location(0) vec4f{return textureLoad(image,vec2i(p.xy),0);}`;
            const shader=this.device.createShaderModule({code});this.presentPipeline=this.device.createRenderPipeline({layout:"auto",vertex:{module:shader,entryPoint:"vs"},fragment:{module:shader,entryPoint:"fs",targets:[{format}]},primitive:{topology:"triangle-list"}});}
        const view=source.texture.createView({dimension:"2d",baseMipLevel:0,mipLevelCount:1});
        const group=this.device.createBindGroup({layout:this.presentPipeline.getBindGroupLayout(0),entries:[{binding:0,resource:view}]});
        const pass=this.commands().beginRenderPass({colorAttachments:[{view:context.getCurrentTexture().createView(),loadOp:"clear",storeOp:"store"}]});pass.setPipeline(this.presentPipeline);pass.setBindGroup(0,group);pass.draw(3);pass.end();this.submit();this.presented++;return 0;
    }
    unconfigure(canvas,surface) {
        if(WireWebGPU.contexts.get(canvas)!==this)return;
        try {surface.context.unconfigure();} finally {WireWebGPU.contexts.delete(canvas);}
    }
    close() {
        if(this.closed)return;this.closed=true;this.generation++;
        this.encoder=null;this.pending=null;
        if(this.drawBackend)this.drawBackend.close();
        for(const entry of this.objects.values()){if(entry.kind==="buffer")entry.value.buffer.destroy();if(entry.kind==="texture")entry.value.texture.destroy();}
        this.objects.clear();this.liveBytes=0;
        for(const [canvas,surface] of this.surfaces){try{this.unconfigure(canvas,surface);}catch(_){}}
        this.surfaces.clear();this.presentPipeline=null;this.device.destroy();
    }
};
WireWebGPU.PENDING = Symbol("WebGPU pending");
